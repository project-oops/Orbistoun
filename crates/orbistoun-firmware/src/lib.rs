//! A skeleton of the console's firmware address space.
//!
//! # Why this crate exists, and what it deliberately is not
//!
//! Most guests use the platform through its **named interface**: they import `sceKernelDlsym`
//! or call `malloc`, and [`orbistoun_kernel`](../orbistoun_kernel/index.html) and its
//! neighbours answer. That is high-level emulation, and for an ordinary title it is the whole
//! job.
//!
//! A small class of guests does not. The open-toolchain payloads - `elfldr`, `pldmgr` and the
//! rest - are post-exploitation agents: they resolve one function, take its address, add a
//! **firmware-version-specific offset measured in tens of megabytes**, and read or write
//! through the result. They are reaching past the interface into the raw memory image the
//! interface is implemented on top of. An emulator that answers every named call perfectly
//! still hands them nothing at those addresses (the sibling decision log's D403, D404).
//!
//! **This is not, and can never be, a real firmware.** No dump, no keys, no vendor bytes - the
//! whole project stays distributable precisely because it holds none of those (CLAUDE.md
//! principle 1). What this provides is a *skeleton*: a region of real, mapped, observable
//! memory where those computed addresses land, so a guest that reaches into the firmware image
//! finds **something honest** - zeroed placeholder memory this project owns - rather than an
//! unmapped sentinel it faults on or, worse, silently misreads.
//!
//! # What it buys, in order of how much it is worth
//!
//! 1. **Debuggability.** Every access a guest makes into the firmware region is an access into
//!    memory this crate owns and can watch. "The payload read firmware+0x2885e00" becomes a
//!    thing that can be observed and reported, where before it was a fault at an address that
//!    named nothing.
//! 2. **Accuracy, honestly bounded.** A guest reaching into the image reads zeroes, which is a
//!    *stated placeholder* and not a guess dressed as data - exactly the discipline the rest of
//!    the project already holds itself to (principle 3). It does not pretend to be the console's
//!    memory; it refuses to pretend, out loud, by being obviously blank.
//! 3. **A base to hand over.** The address arithmetic needs a base, and a base pointing into
//!    this region is one where the arithmetic at least lands in mapped memory rather than in a
//!    marker range.
//!
//! # What it does not do yet, and must not pretend to
//!
//! It does not know the console's real layout - where any particular module sits, what any
//! particular offset points at. Those are measurements this project does not have, and inventing
//! them is the failure it exists to avoid. So the region answers zeroes and says so; filling any
//! part of it with a specific value is a later, evidence-driven step, one measured offset at a
//! time, and each such value will carry its provenance exactly as every other measured fact does.

use std::sync::{Mutex, OnceLock};

use orbistoun_mem::{AddressSpace, MemError, Protection};

/// Where the firmware skeleton is mapped.
///
/// # Why here
///
/// Clear of the guest image (which loads around `0x4000_0000_0000`), of the runtime thunks
/// (`0x7000_0000_0000`), and of every marker range this project uses to name unmapped things
/// (`0x0000_5E27..` and up). A guest that reaches the firmware image lands at an address that
/// is recognisably *firmware* on sight, the same way a sentinel is recognisably a sentinel.
///
/// Not a real console address. The console's own layout is a measurement this project does not
/// have; this is a home of the project's choosing for a region of the project's own making, and
/// choosing a memorable one costs nothing.
pub const FIRMWARE_BASE: u64 = 0x0000_00F0_0000_0000;

/// How much of it is mapped.
///
/// Two gibibytes - comfortably past the tens-of-megabytes offsets the payloads compute, with
/// room for arithmetic that reaches further, and small enough that reserving it is cheap and a
/// stray access still lands inside rather than off the end.
pub const FIRMWARE_SIZE: u64 = 2 * 1024 * 1024 * 1024;

/// The base a guest's firmware arithmetic should be handed.
///
/// The middle of the region rather than its start, so an offset *below* the base - the payloads
/// compute `base - 0xd50000` among others - still lands inside mapped memory instead of just
/// underneath it.
#[must_use]
pub const fn handed_base() -> u64 {
    FIRMWARE_BASE + FIRMWARE_SIZE / 2
}

/// The skeleton firmware image: a mapped region, and a record of what has been reached in it.
#[derive(Debug)]
pub struct Firmware {
    /// Owns the mapping, so the region stays valid exactly as long as this does.
    _space: AddressSpace,
    base: u64,
    len: u64,
}

impl Firmware {
    /// Reserves and zeroes the skeleton region.
    ///
    /// # Errors
    ///
    /// If the host will not give this exact range - almost always because something already
    /// holds it, which is a bug in whoever laid the address space out, not a runtime condition.
    pub fn reserve() -> Result<Self, MemError> {
        Self::reserve_at(FIRMWARE_BASE, FIRMWARE_SIZE)
    }

    /// Reserves a region of a given size at a given base. Split out so a test can use a small
    /// one without standing up two gibibytes.
    pub fn reserve_at(base: u64, len: u64) -> Result<Self, MemError> {
        let mut space = AddressSpace::new();
        // Readable, writable, and executable. A guest reaching the image reads and writes
        // through computed pointers - it thinks it is editing the kernel - and it also *calls*
        // functions it computes there: a payload's CRT reaches `getpid` at `base + 0x5b0` and
        // jumps to it. So the region carries both the blank skeleton and the export stubs, and
        // must permit both a write and an instruction fetch.
        //
        // This is the one place this project maps writable-executable memory, and it is a
        // considered exception: the region models a firmware image, where code and mutable data
        // share an address space, and separating them would mean modelling libkernel's own
        // segment layout - a measurement not yet had. A jump into the blank part still faults
        // usefully, because zeroes decode to instructions that fault rather than to a `ret`.
        space.reserve(
            base,
            len,
            Protection::READ_EXECUTE.union(Protection::READ_WRITE),
        )?;
        Ok(Self {
            _space: space,
            base,
            len,
        })
    }

    /// The first address of the mapped region.
    #[must_use]
    pub const fn base(&self) -> u64 {
        self.base
    }

    /// One past the last.
    #[must_use]
    pub const fn end(&self) -> u64 {
        self.base + self.len
    }

    /// Whether an address falls inside the skeleton - which is how a fault reporter tells
    /// "the guest reached into the firmware image" from any other unmapped access.
    #[must_use]
    pub const fn contains(&self, address: u64) -> bool {
        address >= self.base && address < self.end()
    }

    /// The offset of an address within the image, for saying *where* a guest reached.
    ///
    /// Returns `None` for an address outside the region, so a caller cannot accidentally report
    /// a negative or wrapped offset as if it were inside.
    #[must_use]
    pub const fn offset_of(&self, address: u64) -> Option<u64> {
        if self.contains(address) {
            Some(address - self.base)
        } else {
            None
        }
    }
}

/// Where libkernel sits within the firmware region.
///
/// The module base a payload's `payload_args[0]` is measured against. It is the start of the
/// firmware region, so `firmware+<vaddr>` and `libkernel+<vaddr>` are the same address and a
/// fault reporter's phrasing stays true.
pub const LIBKERNEL_BASE: u64 = FIRMWARE_BASE;

/// `getpid`'s vaddr in the 12.40 `libkernel_sys.sprx`, and the anchor the whole scheme turns on.
///
/// # How this is known
///
/// **Measured, not guessed.** obSCEne pulled the real `/system/common/lib/libkernel_sys.sprx`
/// off a 12.40 console over FTP, and selfish read it as the plain decrypted ELF it arrives as -
/// 1,867 exports with their vaddrs, the platform's own file drawing every number (obSCEne D209).
/// `getpid` sits at `0x5b0`.
///
/// # Why it anchors everything
///
/// A payload loaded by elfldr is handed `getpid`'s runtime address as `payload_args[0]` and
/// nothing else - elfldr resolves no imports. Its CRT computes `libkernel_base = args[0] - 0x5b0`
/// and reaches every other export at `base + vaddr`. So placing `getpid` at `LIBKERNEL_BASE +
/// 0x5b0` and handing that address as word 0 makes the payload's own arithmetic land on this
/// project's functions (D407).
pub const GETPID_VADDR: u64 = 0x5b0;

/// The libkernel exports laid out in the firmware region, at their measured vaddrs.
///
/// Loaded from `data/libkernel-vaddrs.txt`, read off the real 12.40 `libkernel_sys.sprx` (D407).
/// Every entry is a vaddr read from the real file, never a guess.
#[must_use]
pub fn libkernel_exports() -> &'static [(&'static str, u64)] {
    static EXPORTS: OnceLock<Box<[(&'static str, u64)]>> = OnceLock::new();
    EXPORTS.get_or_init(|| {
        let text = include_str!("../data/libkernel-vaddrs.txt");
        let mut list = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.split_whitespace();
            let Some(name) = parts.next() else {
                continue;
            };
            let Some(vaddr_str) = parts.next() else {
                continue;
            };
            let vaddr = if let Some(hex) = vaddr_str
                .strip_prefix("0x")
                .or_else(|| vaddr_str.strip_prefix("0X"))
            {
                u64::from_str_radix(hex, 16).expect("valid hex vaddr in libkernel-vaddrs.txt")
            } else {
                vaddr_str
                    .parse::<u64>()
                    .expect("valid integer vaddr in libkernel-vaddrs.txt")
            };
            list.push((name, vaddr));
        }
        list.into_boxed_slice()
    })
}

/// The address a payload's `payload_args[0]` must hold: `getpid`, in the laid-out region.
#[must_use]
pub const fn getpid_address() -> u64 {
    LIBKERNEL_BASE + GETPID_VADDR
}

/// How an export's vaddr came to be trusted.
///
/// The vaddr table began as numbers scanned out of a firmware file, which is a borderline source
/// and not reproducible the way a hardware record is. The provenance-clean answer is
/// behavioural: obSCEne calls `base + vaddr` on a console and checks the function behaved
/// (its `139-exports`). A vaddr's *source* carries no weight; only that confirmation does. This
/// tracks which vaddrs have earned it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provenance {
    /// A console was watched calling `base + vaddr` and the function behaved as itself.
    Confirmed,
    /// A hypothesis, not yet behaviourally confirmed - useful to point the layout at, but not a
    /// measurement. The default, because most of the table is still awaiting confirmation.
    Candidate,
}

/// The provenance of an export's vaddr, defaulting to [`Provenance::Candidate`].
///
/// Read from the third column of `data/libkernel-vaddrs.txt` (`name vaddr confirmed`); a line
/// without it is a candidate, which is the honest default for a value read off a firmware file
/// and not yet reproduced by behaviour.
#[must_use]
pub fn libkernel_provenance(name: &str) -> Provenance {
    static CONFIRMED: OnceLock<std::collections::BTreeSet<&'static str>> = OnceLock::new();
    let confirmed = CONFIRMED.get_or_init(|| {
        include_str!("../data/libkernel-vaddrs.txt")
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    return None;
                }
                let mut parts = line.split_whitespace();
                let name = parts.next()?;
                let _vaddr = parts.next()?;
                (parts.next() == Some("confirmed")).then_some(name)
            })
            .collect()
    });
    if confirmed.contains(name) {
        Provenance::Confirmed
    } else {
        Provenance::Candidate
    }
}

/// Bytes the anchor slot occupies: `getpid`, which must also expose the syscall gadget at +10.
///
/// The real export table packs functions `0x20` bytes apart, so this must stay under that or it
/// runs over its neighbour - which is exactly the collision that corrupted `getpid` before it was
/// made compact (D407). Twenty-three: `mov eax, N` (5) padded to offset 10, then a 13-byte jump.
pub const ANCHOR_SLOT_LEN: usize = 23;

/// Bytes an ordinary implemented export's slot occupies: a `mov r11, imm64; jmp r11` trampoline.
pub const TRAMPOLINE_SLOT_LEN: usize = 13;

/// Bytes an unimplemented export's slot occupies: `mov edi, vaddr` to name itself, then a jump to
/// the loud-unimplemented handler.
pub const UNIMPLEMENTED_SLOT_LEN: usize = 18;

/// What kind of stub an export's vaddr gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotKind {
    /// `getpid`, the anchor. A compact slot that both calls getpid and exposes the syscall
    /// gadget at +10, so a payload's `getpid + 10` convention works (D400, D407).
    Anchor,
    /// An export this project implements: a trampoline to its real thunk.
    Trampoline,
    /// An export with a measured vaddr but no implementation: a stub that names itself and
    /// answers unimplemented, so a payload reaching it produces a work-list line.
    Unimplemented,
}

impl SlotKind {
    /// How many bytes this slot occupies.
    #[must_use]
    pub const fn size(self) -> usize {
        match self {
            Self::Anchor => ANCHOR_SLOT_LEN,
            Self::Trampoline => TRAMPOLINE_SLOT_LEN,
            Self::Unimplemented => UNIMPLEMENTED_SLOT_LEN,
        }
    }
}

/// One export's place in the laid-out region: what goes there, and whether it runs over its
/// neighbour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placement {
    /// The export's name.
    pub name: String,
    /// Its offset from the libkernel base.
    pub vaddr: u64,
    /// What kind of stub it gets.
    pub kind: SlotKind,
    /// The next export its stub overruns, if any - a *real* overlap, not an alias.
    pub collides_with: Option<(String, u64)>,
}

/// Plans the libkernel layout without touching any memory.
///
/// # Why this is pure and separate from placing the bytes
///
/// The layout is where a whole class of bug lives - a stub that runs over its neighbour and
/// corrupts it, silently, as `getpid`'s 64-byte thunk did to `mount` at `+0x20` (D407). Deciding
/// the layout in a pure function makes that decision **testable and diff-able**: a unit test can
/// assert the anchor fits the packing and that a real overlap is reported, and
/// `orbistoun-cli firmware layout` can print the plan, where before the only way to find a
/// collision was to run a guest into it.
///
/// `is_implemented` answers whether this project has a real thunk for a name; the caller supplies
/// it because this crate cannot see the thunk table.
///
/// # Aliases are not collisions
///
/// Two names at the *same* vaddr are aliases for one function - `listen` and `_listen` both at
/// `0xcd0` - and share a stub rather than fighting over the address. Only a next export that
/// starts **after** this one but **before** its stub ends is a real overlap; a same-vaddr
/// neighbour is not, and reporting it as one is the false positive that first made the collision
/// output hard to read.
#[must_use]
pub fn plan_layout(
    exports: &[(String, u64)],
    is_implemented: impl Fn(&str) -> bool,
) -> Vec<Placement> {
    let mut sorted = exports.to_vec();
    sorted.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));

    sorted
        .iter()
        .enumerate()
        .map(|(i, (name, vaddr))| {
            let kind = if name == "getpid" || *vaddr == GETPID_VADDR {
                SlotKind::Anchor
            } else if is_implemented(name) {
                SlotKind::Trampoline
            } else {
                SlotKind::Unimplemented
            };
            let end = vaddr.saturating_add(kind.size() as u64);
            let collides_with = sorted.get(i + 1).and_then(|(next_name, next_vaddr)| {
                // Strictly between this vaddr and its stub's end: a same-vaddr alias is excluded.
                (*next_vaddr > *vaddr && *next_vaddr < end)
                    .then(|| (next_name.clone(), *next_vaddr))
            });
            Placement {
                name: name.clone(),
                vaddr: *vaddr,
                kind,
                collides_with,
            }
        })
        .collect()
}

/// Places a function's stub at a libkernel export vaddr, so a payload reaching it by
/// `base + vaddr` lands on this project's dispatch.
///
/// The bytes are a thunk emitted elsewhere - this crate holds no code generator, only the
/// memory. They are copied verbatim because this project's thunks are position-independent
/// (absolute call targets, self-relative internal jump), so a thunk works wherever it is put.
///
/// # Errors
///
/// If no region is reserved, or the vaddr plus the stub would run past its end - either is a
/// caller laying the region out wrong, not a runtime condition.
pub fn place_export(vaddr: u64, thunk: &[u8]) -> Result<(), MemError> {
    let held = cell()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(fw) = held.as_ref() else {
        return Err(MemError::HostRefused(
            "no firmware region is reserved to place an export in".to_owned(),
        ));
    };
    let at = LIBKERNEL_BASE.saturating_add(vaddr);
    let end = at.saturating_add(thunk.len() as u64);
    if at < fw.base() || end > fw.end() {
        return Err(MemError::HostRefused(format!(
            "libkernel export at {at:#x} would fall outside the firmware region"
        )));
    }
    let Ok(dest) = usize::try_from(at) else {
        return Err(MemError::HostRefused(
            "export address does not fit".to_owned(),
        ));
    };
    // SAFETY: `at`..`end` was just checked to lie inside the region this process reserved
    // read-write, and `thunk` is a valid slice for its own length. The region outlives the run.
    unsafe {
        std::ptr::copy_nonoverlapping(
            thunk.as_ptr(),
            std::ptr::with_exposed_provenance_mut::<u8>(dest),
            thunk.len(),
        );
    }
    Ok(())
}

/// The one firmware skeleton this process holds, once reserved.
fn cell() -> &'static Mutex<Option<Firmware>> {
    static CELL: OnceLock<Mutex<Option<Firmware>>> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(None))
}

/// Reserves the skeleton for this process, if it is not already present.
///
/// Idempotent: a second call with the region already mapped is a success that changes nothing,
/// so a caller need not track whether it was the one to set it up.
///
/// # Errors
///
/// Propagates a reservation failure from [`Firmware::reserve`].
pub fn present() -> Result<(), MemError> {
    let mut held = cell()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if held.is_none() {
        *held = Some(Firmware::reserve()?);
    }
    Ok(())
}

/// Whether a firmware region has been reserved in this process.
#[must_use]
pub fn is_present() -> bool {
    cell().lock().is_ok_and(|held| held.is_some())
}

/// Whether an address is inside the firmware skeleton this process reserved.
///
/// Answers `false` when no skeleton is present, so a run that never stood one up reads every
/// address as "not firmware" rather than erroring - the region is an opt-in, and its absence is
/// an ordinary state.
#[must_use]
pub fn is_firmware_address(address: u64) -> bool {
    cell()
        .lock()
        .is_ok_and(|held| held.as_ref().is_some_and(|f| f.contains(address)))
}

/// The offset of an address within the reserved skeleton, or `None`.
///
/// What a fault reporter calls to turn a raw address into "firmware+0x2885e00" - the phrase that
/// makes a payload's arithmetic legible.
#[must_use]
pub fn firmware_offset(address: u64) -> Option<u64> {
    cell()
        .lock()
        .ok()
        .and_then(|held| held.as_ref().and_then(|f| f.offset_of(address)))
}

/// The offset of an address within the firmware region, decoded from the constants alone.
///
/// # Why a second, stateless decoder
///
/// [`firmware_offset`] answers only when a skeleton has actually been reserved in this process,
/// which is right for a caller acting on live memory. A fault *reporter* is different: it names
/// an address after the guest has stopped, often in a process that never reserved the region,
/// and it should still be able to say "that was a firmware address" from the number alone -
/// exactly as the sentinel decoder reads a sentinel without the block being present. This
/// decodes from [`FIRMWARE_BASE`] and [`FIRMWARE_SIZE`], so it names the range whether or not
/// one was mapped.
#[must_use]
pub const fn firmware_slot(address: u64) -> Option<u64> {
    if address >= FIRMWARE_BASE && address < FIRMWARE_BASE + FIRMWARE_SIZE {
        Some(address - FIRMWARE_BASE)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{FIRMWARE_BASE, FIRMWARE_SIZE, Firmware, handed_base};

    /// The handed base is inside the region, and far enough from either edge that the payloads'
    /// offsets - which reach both below and tens of megabytes above it - stay inside.
    #[test]
    fn the_handed_base_leaves_room_both_ways() {
        let base = handed_base();
        assert!(base > FIRMWARE_BASE, "room below for a negative offset");
        assert!(
            base + 64 * 1024 * 1024 < FIRMWARE_BASE + FIRMWARE_SIZE,
            "room above for the largest offsets observed"
        );
        // The largest below-offset a payload was seen to use is 0xd50000; it must stay inside.
        assert!(
            base - 0x00d5_0000 > FIRMWARE_BASE,
            "a below-offset stays mapped"
        );
    }

    /// `contains` and `offset_of` agree, and both refuse an address outside the region.
    ///
    /// A small region, so the test does not reserve two gibibytes - the boundary logic is what
    /// is under test, not the size.
    #[test]
    fn membership_and_offset_agree_at_the_edges() {
        let base = 0x0000_00F0_0000_0000;
        let len = 0x1000;
        let fw = Firmware::reserve_at(base, len).expect("a small region reserves");
        assert!(fw.contains(base), "the first byte is inside");
        assert_eq!(fw.offset_of(base), Some(0));
        assert!(fw.contains(base + len - 1), "the last byte is inside");
        assert_eq!(fw.offset_of(base + len - 1), Some(len - 1));
        assert!(!fw.contains(base + len), "one past the end is outside");
        assert_eq!(fw.offset_of(base + len), None, "and has no offset");
        assert!(!fw.contains(base - 1), "one before the start is outside");
    }

    /// The libkernel exports table loads from the measured data file and parses correctly.
    #[test]
    fn libkernel_exports_table_loads_and_contains_anchors() {
        let exports = super::libkernel_exports();
        assert!(!exports.is_empty(), "exports table must not be empty");
        assert!(
            exports
                .iter()
                .any(|&(name, v)| name == "getpid" && v == super::GETPID_VADDR),
            "getpid must be present at GETPID_VADDR"
        );
        assert!(
            exports
                .iter()
                .any(|&(name, v)| name == "sceKernelWrite" && v == 0x16e00),
            "sceKernelWrite must be present at 0x16e00"
        );
    }

    /// **The getpid collision, as a test.** With the real 0x20-byte packing, a 64-byte anchor
    /// would run over its neighbour; the anchor is compact for exactly this reason, so it must
    /// fit and not collide (D407). This is the check that would have caught the bug before a
    /// guest ran into it.
    #[test]
    fn the_getpid_anchor_fits_the_real_packing() {
        use super::{GETPID_VADDR, SlotKind};
        // The measured spacing: getpid, mount, unmount, each 0x20 apart.
        let exports = vec![
            ("getpid".to_owned(), GETPID_VADDR),
            ("mount".to_owned(), GETPID_VADDR + 0x20),
            ("unmount".to_owned(), GETPID_VADDR + 0x40),
        ];
        let plan = super::plan_layout(&exports, |_| false);
        let getpid = &plan[0];
        assert_eq!(getpid.name, "getpid");
        assert_eq!(getpid.kind, SlotKind::Anchor);
        assert!(
            getpid.kind.size() <= 0x20,
            "the anchor must fit the 0x20-byte gap - a 64-byte one is what corrupted getpid"
        );
        assert_eq!(
            getpid.collides_with, None,
            "the compact anchor must not run over mount"
        );
    }

    /// A real overlap is reported; a same-vaddr alias is not.
    #[test]
    fn a_real_overlap_reports_and_an_alias_does_not() {
        // Two names at one vaddr are aliases and share a stub - not a collision.
        // A close-but-distinct neighbour inside the stub's length is a real overlap.
        let exports = vec![
            ("listen".to_owned(), 0xcd0),
            ("_listen".to_owned(), 0xcd0),
            ("tight".to_owned(), 0xcd4), // 4 bytes after: inside a 13-byte trampoline
        ];
        let plan = super::plan_layout(&exports, |_| true);
        let listen = plan.iter().find(|p| p.name == "listen").expect("listen");
        // listen at 0xcd0, next distinct export tight at 0xcd4 (4 < 13) - a real overlap.
        assert_eq!(
            listen.collides_with,
            Some(("tight".to_owned(), 0xcd4)),
            "a distinct neighbour inside the stub length overlaps"
        );
        let alias = plan.iter().find(|p| p.name == "_listen").expect("_listen");
        // The alias sits at the same vaddr as listen; its own next is tight, also an overlap,
        // but the point is the *alias pair* is not reported against each other.
        assert_ne!(
            alias.collides_with.as_ref().map(|(n, _)| n.as_str()),
            Some("listen"),
            "an alias at the same vaddr must not be a collision"
        );
    }

    /// getpid and sceKernelWrite are behaviourally confirmed; an arbitrary export is a candidate.
    #[test]
    fn provenance_marks_the_confirmed_exports() {
        use super::Provenance;
        assert_eq!(super::libkernel_provenance("getpid"), Provenance::Confirmed);
        assert_eq!(
            super::libkernel_provenance("sceKernelWrite"),
            Provenance::Confirmed
        );
        assert_eq!(super::libkernel_provenance("mount"), Provenance::Candidate);
    }

    /// The kind is chosen by name and implementation status.
    #[test]
    fn slot_kind_follows_name_and_implementation() {
        use super::SlotKind;
        let exports = vec![
            ("getpid".to_owned(), super::GETPID_VADDR),
            ("sceKernelWrite".to_owned(), 0x16e00),
            ("obscure_export".to_owned(), 0x20000),
        ];
        let plan = super::plan_layout(&exports, |name| name == "sceKernelWrite");
        let by = |n: &str| plan.iter().find(|p| p.name == n).unwrap().kind;
        assert_eq!(by("getpid"), SlotKind::Anchor);
        assert_eq!(by("sceKernelWrite"), SlotKind::Trampoline);
        assert_eq!(by("obscure_export"), SlotKind::Unimplemented);
    }
}
