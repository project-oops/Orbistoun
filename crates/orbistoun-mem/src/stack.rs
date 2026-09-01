//! A stack for guest code to run on.
//!
//! # Why not just use the host thread's stack
//!
//! It would work, briefly. A guest expects megabytes and grows downwards without
//! asking; the host thread's stack is sized for host code and has the host's own frames
//! below it. A guest that overruns would corrupt the frames of whatever called into it,
//! and the crash would appear inside the emulator rather than in the guest - pointing
//! at the wrong code entirely.
//!
//! # The guard page is the point
//!
//! Without one, a stack overflow runs quietly into whatever is mapped next and corrupts
//! it. With one, it faults immediately at an address adjacent to the stack, which says
//! what happened. The guard is reserved as part of the same span so nothing else can
//! ever be placed in the gap.
//!
//! ```text
//!   low                                                                    high
//!   +--------+-----------------------------------------------+-------------+
//!   | guard  |                usable stack                    | read-ahead |
//!   | (none) |                                                | (readable) |
//!   +--------+-----------------------------------------------+-------------+
//!            ^                                                ^
//!            overflow faults here              initial stack pointer
//! ```
//!
//! # Why there is a *readable* guard above the stack too
//!
//! The lower guard catches an overflow by faulting. The upper one is the opposite: it is mapped
//! and readable, and it exists because a process reads its argument block - argc, argv, the
//! environment, the auxiliary vector - through the pointer it is handed at entry, and a runtime
//! reading it does not know how long it is, so it reads a fixed number of words with room to
//! spare. On a real machine those words land in the stack region above the block and read back
//! whatever is there; with the block placed flush against the top of the mapping, the same
//! read runs one word past the end and faults - which is exactly how obSCEne's handoff probe
//! (`136-kernel/handoff`, twenty words) died under orbistoun where it passes on a console. A
//! page of readable zeroes above the initial pointer makes a modest over-read land on mapped
//! memory, as it does on hardware, rather than on a fault with no relation to the cause (D445).

use orbistoun_core::GUEST_PAGE_SIZE;

use crate::{AddressSpace, MemError, Protection};

/// Default usable stack, before the guard.
///
/// Matches the eight megabytes a thread conventionally gets on the systems the target
/// derives from. Guessing smaller would make a deep call chain look like a guest bug.
pub const DEFAULT_STACK_SIZE: u64 = 8 * 1024 * 1024;

/// Bytes of unmapped space below the stack, so an overflow faults instead of spreading.
pub const GUARD_SIZE: u64 = GUEST_PAGE_SIZE;

/// Bytes of *readable* space above the initial stack pointer, so a process reading a few words
/// past its argument block lands on mapped memory instead of faulting - see the module note.
pub const READAHEAD_GUARD: u64 = GUEST_PAGE_SIZE;

/// The alignment System V requires of the stack pointer at a call site.
pub const STACK_ALIGN: u64 = 16;

/// A guest stack, mapped and guarded.
#[derive(Debug)]
pub struct GuestStack {
    space: AddressSpace,
    base: u64,
    len: u64,
}

impl GuestStack {
    /// Reserves a stack of `len` usable bytes at `base`, with a guard page below it.
    ///
    /// `base` is the lowest address of the whole span - the guard sits at `base`, and
    /// usable memory starts one page above it.
    pub fn reserve(base: u64, len: u64) -> Result<Self, MemError> {
        let usable = len.max(GUEST_PAGE_SIZE).div_ceil(GUEST_PAGE_SIZE) * GUEST_PAGE_SIZE;
        // The lower guard (faults) and the upper read-ahead guard (stays readable) are both part
        // of the one span, so nothing else can ever be placed in either gap.
        let total = usable
            .saturating_add(GUARD_SIZE)
            .saturating_add(READAHEAD_GUARD);

        let mut space = AddressSpace::new();
        space.reserve(base, total, Protection::READ_WRITE)?;

        // Reserved as part of the span, then made inaccessible: reserving it separately
        // would leave a window in which something else could take the address, and the
        // guard would silently not exist.
        space.protect(
            base,
            GUARD_SIZE,
            Protection {
                read: false,
                write: false,
                execute: false,
            },
        )?;

        Ok(Self {
            space,
            base,
            len: usable,
        })
    }

    /// Fills the usable stack with a byte before the guest runs.
    ///
    /// # What this is for, and why zero is not a neutral choice
    ///
    /// The host hands back zeroed pages, so guest memory nobody has written reads as zero -
    /// consistently, every run. That is comfortable and misleading. A stub that was supposed to
    /// fill a caller's buffer and did not leaves the guest reading a plausible, stable zero, and
    /// **there is no signature to recognise in a trace because nothing was written to
    /// recognise** (D171).
    ///
    /// Running twice with different fills answers the question directly: if the two runs
    /// disagree, the guest read memory nobody wrote. If they agree, they did not, and a whole
    /// class of explanation is eliminated rather than argued about.
    ///
    /// Not a fix and not a default. Zero remains the ordinary case - it is what the host does
    /// and what the target most likely does - and this exists so the assumption can be tested
    /// instead of relied upon.
    pub fn fill(&mut self, byte: u8) -> Result<(), MemError> {
        if byte == 0 {
            // Already zero, and rewriting eight megabytes to no effect would make the
            // instrumented run needlessly slower than the one it is compared against.
            return Ok(());
        }
        let start = self.base.saturating_add(GUARD_SIZE);
        let (Ok(at), Ok(len)) = (usize::try_from(start), usize::try_from(self.len)) else {
            return Err(MemError::HostRefused(format!(
                "stack span {start:#x}+{:#x} does not fit a host pointer",
                self.len
            )));
        };
        // SAFETY: `start..start + len` is the usable span this type reserved as read-write,
        // above the guard, and the guest is not running yet - nothing else can observe it.
        unsafe {
            std::ptr::write_bytes(std::ptr::with_exposed_provenance_mut::<u8>(at), byte, len);
        }
        Ok(())
    }

    /// The initial stack pointer: the top of usable memory, aligned down.
    ///
    /// Aligned because System V requires `rsp % 16 == 0` at a call site. An unaligned
    /// stack does nothing at all until some callee executes an aligned vector
    /// instruction against a stack slot, and then faults far from the cause.
    pub const fn initial_pointer(&self) -> u64 {
        let top = self
            .base
            .saturating_add(GUARD_SIZE)
            .saturating_add(self.len);
        top / STACK_ALIGN * STACK_ALIGN
    }

    /// Lowest usable address - one page above the guard.
    pub const fn lowest_usable(&self) -> u64 {
        self.base.saturating_add(GUARD_SIZE)
    }

    /// Where the guard page sits. An overflow faults here.
    pub const fn guard(&self) -> u64 {
        self.base
    }

    /// Usable bytes, excluding the guard.
    pub const fn len(&self) -> u64 {
        self.len
    }

    /// Whether the stack has no usable bytes.
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The address space holding the stack.
    pub const fn space(&self) -> &AddressSpace {
        &self.space
    }
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_STACK_SIZE, GUARD_SIZE, GuestStack, STACK_ALIGN};
    use orbistoun_core::GUEST_PAGE_SIZE;

    /// A base no other test in this binary will use.
    ///
    /// # Why this is not a constant somebody picks
    ///
    /// Tests in one binary run on parallel threads, and these reserve **real host memory at
    /// fixed addresses** - so two tests sharing a base race, and one of them fails. That is
    /// exactly what happened: a test added here chose `TEST_BASE + 0x300_0000` by reading
    /// the file and picking a gap, and an existing test three functions later was already
    /// using it. It failed roughly one run in ten, which is the worst frequency - often
    /// enough to be real, rare enough to be dismissed as flakiness.
    ///
    /// Handing out addresses removes the choice rather than documenting it. The **cross-crate**
    /// half of the same hazard is closed by taking from this crate's own range rather than
    /// from a bare constant - see `crate::test_bases`.
    fn unique_base() -> u64 {
        use crate::test_bases::{Range, crates};
        static RANGE: Range = Range::nth(crates::MEM);
        RANGE.take()
    }

    #[test]
    fn the_initial_pointer_is_at_the_top_and_correctly_aligned() {
        // Stacks grow downwards, so starting anywhere but the top wastes most of it.
        // The alignment is what System V requires at a call site.
        let base = unique_base();
        let stack = GuestStack::reserve(base, GUEST_PAGE_SIZE * 4).expect("reserve");
        assert_eq!(stack.initial_pointer() % STACK_ALIGN, 0);
        assert_eq!(
            stack.initial_pointer(),
            base + GUARD_SIZE + GUEST_PAGE_SIZE * 4
        );
    }

    #[test]
    fn the_guard_sits_below_the_usable_range_not_above_it() {
        // Above the stack it would guard nothing: overflow goes downwards.
        let stack = GuestStack::reserve(unique_base(), GUEST_PAGE_SIZE).expect("reserve");
        assert!(stack.guard() < stack.lowest_usable());
        assert_eq!(stack.lowest_usable() - stack.guard(), GUARD_SIZE);
    }

    #[test]
    fn a_read_just_past_the_initial_pointer_lands_on_mapped_memory() {
        // The property that fixes the handoff wall: a process over-reads its argument block
        // past the entry pointer, and the read-ahead guard keeps that on mapped, readable
        // memory rather than faulting one word past the top (D445). Distinct from the lower
        // guard, which is unmapped so an *overflow* faults.
        use super::READAHEAD_GUARD;
        let stack = GuestStack::reserve(unique_base(), GUEST_PAGE_SIZE * 4).expect("reserve");
        let top = stack.initial_pointer();

        // The whole span the stack reserved must extend a read-ahead guard above the initial
        // pointer, so the over-read has somewhere mapped to land.
        let span_top = stack
            .space()
            .regions()
            .iter()
            .map(|r| r.base.saturating_add(r.len))
            .max()
            .expect("a reserved region");
        assert!(
            span_top >= top + READAHEAD_GUARD,
            "the span must cover a read-ahead guard above the initial pointer"
        );

        // And it must actually be *readable* - unlike the lower guard. Reading it back proves
        // the mapping is there; an unmapped page would fault the test instead of returning.
        for offset in [0_u64, READAHEAD_GUARD - 1] {
            let at = usize::try_from(top + offset).expect("addressable");
            // SAFETY: `[top, top + READAHEAD_GUARD)` was reserved read-write by `reserve` and is
            // not the lower guard, and the stack outlives this read.
            let byte = unsafe { std::ptr::with_exposed_provenance::<u8>(at).read_volatile() };
            assert_eq!(byte, 0, "the read-ahead guard reads as zero");
        }
    }

    #[test]
    fn the_span_to_dereference_is_the_one_the_stack_reports_not_the_one_it_was_asked_for() {
        // **The exact mistake this is here to stop.** `orbistoun-worker` told the dump path
        // that guest memory was safe to read across `(base, requested_len)` - the two values
        // it had passed to `reserve` - rather than asking the stack what it had built. Those
        // are not the same span: a guard page sits at `base`, so the real one is a page
        // higher at both ends.
        //
        // The window was therefore shifted down by a page. It offered the one page mapped
        // specifically to fault, and refused the top page of real stack - which is where the
        // lead argument on the `image+0xafc959` wall lives, so every dump of it came back
        // empty and the pointer read as a count (D217).
        let requested = 8 * GUEST_PAGE_SIZE;
        let stack = GuestStack::reserve(unique_base(), requested).expect("reserve");

        let asked_for = (stack.guard(), requested);
        let actual = (stack.lowest_usable(), stack.len());
        assert_ne!(asked_for, actual, "if these ever match, this test is a lie");

        let contains = |(base, len): (u64, u64), at: u64| at >= base && at < base + len;

        // The last usable byte is real memory and must be readable.
        let last_usable = stack.lowest_usable() + stack.len() - 1;
        assert!(contains(actual, last_usable), "the top page is usable");
        assert!(
            !contains(asked_for, last_usable),
            "and the span the worker used to declare excluded it"
        );

        // The guard is mapped inaccessible. Declaring it readable invites a dump that
        // faults inside the emulator, turning a diagnostic into a crash (D194).
        assert!(
            !contains(actual, stack.guard()),
            "the guard is not readable"
        );
        assert!(
            contains(asked_for, stack.guard()),
            "and the span the worker used to declare offered it"
        );
    }

    #[test]
    fn writing_to_the_usable_range_works() {
        let stack = GuestStack::reserve(unique_base(), GUEST_PAGE_SIZE).expect("reserve");
        let at = stack.initial_pointer() - 8;
        let p = std::ptr::with_exposed_provenance_mut::<u64>(usize::try_from(at).expect("fits"));
        // SAFETY: the reservation covers this address with write permission, and it is
        // eight bytes below the top of usable memory so the whole write is in range.
        unsafe { p.write_volatile(0x1234_5678) };
        // SAFETY: same address, just written.
        assert_eq!(unsafe { p.read_volatile() }, 0x1234_5678);
    }

    #[test]
    fn a_tiny_request_is_rounded_up_to_a_whole_page() {
        // Reserving a partial page is not something the host can do, and silently
        // handing back less than asked for is worse than rounding.
        let stack = GuestStack::reserve(unique_base(), 1).expect("reserve");
        assert_eq!(stack.len(), GUEST_PAGE_SIZE);
        assert!(!stack.is_empty());
    }

    #[test]
    fn the_default_size_matches_a_conventional_thread_stack() {
        // Guessing smaller would make an ordinary deep call chain look like a guest bug.
        assert_eq!(DEFAULT_STACK_SIZE, 8 * 1024 * 1024);
    }
}
