//! A stack for guest code to run on.
//!
//! A guest expects megabytes and grows downwards without asking; on the host thread's stack
//! an overrun would corrupt the emulator's own frames. The span has an unmapped guard page
//! below, so an overflow faults next to the stack, and a readable page of zeroes above the
//! initial pointer, because a process reads a fixed number of words through its argument
//! block pointer and on hardware those words land on mapped memory (D445).
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

use orbistoun_core::GUEST_PAGE_SIZE;

use crate::{AddressSpace, MemError, Protection};

/// Default usable stack, before the guard.
///
/// The eight megabytes a thread conventionally gets on the systems the target derives from.
pub const DEFAULT_STACK_SIZE: u64 = 8 * 1024 * 1024;

/// Bytes of unmapped space below the stack, so an overflow faults instead of spreading.
pub const GUARD_SIZE: u64 = GUEST_PAGE_SIZE;

/// Bytes of readable space above the initial stack pointer, so a process reading a few words
/// past its argument block lands on mapped memory - see the module note.
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
    /// `base` is the lowest address of the whole span: the guard sits at `base`, and usable
    /// memory starts one page above it.
    pub fn reserve(base: u64, len: u64) -> Result<Self, MemError> {
        let usable = len.max(GUEST_PAGE_SIZE).div_ceil(GUEST_PAGE_SIZE) * GUEST_PAGE_SIZE;
        // Both guards are part of the one span, so nothing else can be placed in either gap.
        let total = usable
            .saturating_add(GUARD_SIZE)
            .saturating_add(READAHEAD_GUARD);

        let mut space = AddressSpace::new();
        space.reserve(base, total, Protection::READ_WRITE)?;

        // Reserved as part of the span, then made inaccessible, so no other mapping can take the
        // address in between.
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
    /// The host hands back zeroed pages, so memory nobody wrote reads as a stable, plausible zero
    /// (D171). Two runs with different fills disagree exactly when the guest read memory nobody
    /// wrote. Zero stays the default, as it is what the host does.
    pub fn fill(&mut self, byte: u8) -> Result<(), MemError> {
        if byte == 0 {
            // Already zero; rewriting eight megabytes would only slow the instrumented run.
            return Ok(());
        }
        let start = self.base.saturating_add(GUARD_SIZE);
        let (Ok(at), Ok(len)) = (usize::try_from(start), usize::try_from(self.len)) else {
            return Err(MemError::HostRefused(format!(
                "stack span {start:#x}+{:#x} does not fit a host pointer",
                self.len
            )));
        };
        // SAFETY: `start..start + len` is the usable span this type reserved read-write, above the
        // guard, and the guest is not running yet.
        unsafe {
            std::ptr::write_bytes(std::ptr::with_exposed_provenance_mut::<u8>(at), byte, len);
        }
        Ok(())
    }

    /// The initial stack pointer: the top of usable memory, aligned down.
    ///
    /// System V requires `rsp % 16 == 0` at a call site; a misaligned stack faults later, on the
    /// first aligned vector access to a stack slot.
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
    /// Tests run on parallel threads and reserve real host memory at fixed addresses, so a shared
    /// base fails intermittently. Bases come from this crate's range in `crate::test_bases`.
    fn unique_base() -> u64 {
        crate::unique_test_base()
    }

    #[test]
    fn the_initial_pointer_is_at_the_top_and_correctly_aligned() {
        // Stacks grow downwards, and System V requires the alignment at a call site.
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
        // The read-ahead guard keeps an over-read past the entry pointer on mapped, readable memory
        // (D445). The lower guard is unmapped so an overflow faults.
        use super::READAHEAD_GUARD;
        let stack = GuestStack::reserve(unique_base(), GUEST_PAGE_SIZE * 4).expect("reserve");
        let top = stack.initial_pointer();

        // The reserved span extends a read-ahead guard above the initial pointer.
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

        // And it is readable, unlike the lower guard: an unmapped page would fault the test.
        for offset in [0_u64, READAHEAD_GUARD - 1] {
            let at = usize::try_from(top + offset).expect("addressable");
            // SAFETY: `[top, top + READAHEAD_GUARD)` was reserved read-write by `reserve`, is not
            // the lower guard, and the stack outlives this read.
            let byte = unsafe { std::ptr::with_exposed_provenance::<u8>(at).read_volatile() };
            assert_eq!(byte, 0, "the read-ahead guard reads as zero");
        }
    }

    #[test]
    fn the_span_to_dereference_is_the_one_the_stack_reports_not_the_one_it_was_asked_for() {
        // The readable window is the one the stack built, not `(base, requested_len)`: the guard
        // page at `base` shifts the real span a page higher at both ends, and the top page holds
        // the most recent frames.
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

        // The guard is inaccessible; declaring it readable would let a dump fault inside the
        // emulator (D194).
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
        // SAFETY: the reservation covers this address with write permission, and it is eight bytes
        // below the top of usable memory.
        unsafe { p.write_volatile(0x1234_5678) };
        // SAFETY: same address, just written.
        assert_eq!(unsafe { p.read_volatile() }, 0x1234_5678);
    }

    #[test]
    fn a_tiny_request_is_rounded_up_to_a_whole_page() {
        // The host cannot reserve a partial page, and handing back less than asked is worse than
        // rounding.
        let stack = GuestStack::reserve(unique_base(), 1).expect("reserve");
        assert_eq!(stack.len(), GUEST_PAGE_SIZE);
        assert!(!stack.is_empty());
    }

    #[test]
    fn the_default_size_matches_a_conventional_thread_stack() {
        // A smaller default would make an ordinary deep call chain look like a guest fault.
        assert_eq!(DEFAULT_STACK_SIZE, 8 * 1024 * 1024);
    }
}
