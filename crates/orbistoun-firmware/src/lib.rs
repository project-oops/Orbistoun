//! A skeleton of the platform firmware's address space.
//!
//! Most guests use the platform through its named interface, which HLE answers. The
//! open-toolchain payloads also resolve one function, add a firmware-version-specific offset
//! and read, write or call through the result, reaching the memory image beneath the interface.
//! This crate maps a region of zeroed memory this project owns where those computed addresses
//! land, so such an access is observable and reportable instead of a fault at an address that
//! names nothing (D404). It holds no firmware bytes: libkernel exports are placed at measured
//! vaddrs as stubs, and everything else reads as a stated zero placeholder.

use std::sync::{Mutex, OnceLock};

use orbistoun_mem::{AddressSpace, MemError, Protection};

/// Where the firmware skeleton is mapped.
///
/// Clear of the guest image (around `0x4000_0000_0000`), the runtime thunks
/// (`0x7000_0000_0000`) and the marker ranges (`0x0000_5E27..` and up), so a firmware address
/// is recognisable on sight. A base of this project's choosing, not a hardware address.
pub const FIRMWARE_BASE: u64 = 0x0000_00F0_0000_0000;

/// How much of it is mapped: two gibibytes, well past the tens-of-megabytes offsets payloads
/// compute, and cheap to reserve.
pub const FIRMWARE_SIZE: u64 = 2 * 1024 * 1024 * 1024;

/// The base a guest's firmware arithmetic should be handed.
///
/// The middle of the region, so an offset below the base (payloads compute `base - 0xd50000`)
/// still lands in mapped memory.
#[must_use]
pub const fn handed_base() -> u64 {
    FIRMWARE_BASE + FIRMWARE_SIZE / 2
}

/// The platform's own libkernel base, where a freestanding first-party payload calls its
/// syscall gadget when it cannot resolve libkernel by name.
///
/// The first-party SDK routes every syscall through the raw `syscall` instruction ten bytes
/// into libkernel's `getpid`. Without a handoff block or a resolvable `getpid`, the runtime
/// falls back to a hardcoded address: libkernel at `0x8_0000_0000`, `getpid` at `0x4e0`, so it
/// issues `callq *0x8000004ea` for every system call. The address is baked into the guest, so
/// this project maps a page here and puts a trampoline into this run's syscall gadget at
/// [`CONSOLE_SYSCALL_GADGET_VADDR`].
pub const CONSOLE_SYSCALL_GADGET_BASE: u64 = 0x0000_0008_0000_0000;

/// Where the syscall gadget sits within the libkernel page: `getpid` (`0x4e0`) plus ten, where
/// the raw `syscall` instruction is.
///
/// The current-generation offset. The previous generation places `getpid` at `0x5b0`, so its
/// gadget is `+0x5ba`; only the offset a guest reaches is mapped.
pub const CONSOLE_SYSCALL_GADGET_VADDR: u64 = 0x4ea;

/// How much is mapped for it: one 16 KiB page, the platform's page size.
pub const CONSOLE_SYSCALL_GADGET_SIZE: u64 = 0x4000;

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
    /// If the host will not give this exact range, which means something already holds it: a
    /// fault in whoever laid the address space out.
    pub fn reserve() -> Result<Self, MemError> {
        Self::reserve_at(FIRMWARE_BASE, FIRMWARE_SIZE)
    }

    /// Reserves a region of a given size at a given base, so a test can use a small one.
    pub fn reserve_at(base: u64, len: u64) -> Result<Self, MemError> {
        let mut space = AddressSpace::new();
        // Readable, writable and executable: a guest reads and writes through computed pointers
        // and also calls functions it computes there (a payload's CRT jumps to `getpid` at
        // `base + 0x5b0`), so the region holds both the blank skeleton and the export stubs. The
        // only writable-executable mapping in this project; separating code from data would need
        // libkernel's own segment layout. A jump into the blank part faults, since zeroes do not
        // decode to a `ret`.
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

    /// Whether an address falls inside the skeleton, which is how a fault reporter tells a
    /// firmware access from any other unmapped access.
    #[must_use]
    pub const fn contains(&self, address: u64) -> bool {
        address >= self.base && address < self.end()
    }

    /// The offset of an address within the image, for saying where a guest reached.
    ///
    /// `None` for an address outside the region, so a wrapped offset is never reported as inside.
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
/// The start of the region, so `firmware+<vaddr>` and `libkernel+<vaddr>` are the same
/// address.
pub const LIBKERNEL_BASE: u64 = FIRMWARE_BASE;

/// `getpid`'s vaddr in the 12.40 `libkernel_sys.sprx`, the anchor of the layout.
///
/// A payload loaded by elfldr receives `getpid`'s runtime address as `payload_args[0]` and
/// nothing else; its CRT computes `libkernel_base = args[0] - 0x5b0` and reaches every other
/// export at `base + vaddr`. Placing `getpid` at `LIBKERNEL_BASE + 0x5b0` and handing that
/// address makes the payload's own arithmetic land on this project's functions (D407). The
/// vaddrs are in `data/libkernel-vaddrs.txt`.
pub const GETPID_VADDR: u64 = 0x5b0;

/// The libkernel exports laid out in the firmware region, at their measured vaddrs.
///
/// Loaded from `data/libkernel-vaddrs.txt` (D407).
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
/// A vaddr is trusted only when obSCEne calls `base + vaddr` on hardware and the function
/// behaves as itself (its `139-exports` checks); where the number was read from carries no
/// weight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provenance {
    /// Hardware was observed calling `base + vaddr` and the function behaved as itself.
    Confirmed,
    /// A hypothesis, not yet behaviourally confirmed. The default.
    Candidate,
}

/// The provenance of an export's vaddr, defaulting to [`Provenance::Candidate`].
///
/// Read from the third column of `data/libkernel-vaddrs.txt` (`name vaddr confirmed`).
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
/// The export table packs functions `0x20` bytes apart, so the slot stays under that or it
/// overruns its neighbour (D407). Twenty-three: `mov eax, N` (5) padded to offset 10, then a
/// 13-byte jump.
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
    /// gadget at +10, so a payload's `getpid + 10` convention works (D407).
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
    /// The next export its stub overruns, if any: a real overlap, not an alias.
    pub collides_with: Option<(String, u64)>,
}

/// Plans the libkernel layout without touching any memory.
///
/// Pure, so a test can assert the anchor fits the packing and a real overlap is reported, and
/// `orbistoun-cli firmware layout` can print the plan (D407). `is_implemented` answers whether
/// this project has a thunk for a name; the caller supplies it because this crate cannot see
/// the thunk table. Two names at the same vaddr (`listen` and `_listen`) are aliases sharing
/// one stub; only a next export starting after this one and before its stub ends is an overlap.
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
/// The bytes are a thunk emitted elsewhere, copied verbatim: this project's thunks are
/// position-independent (absolute call targets, self-relative internal jump).
///
/// # Errors
///
/// If no region is reserved, or the vaddr plus the stub would run past its end: the caller
/// laying the region out wrong.
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
    // SAFETY: `at`..`end` was just checked to lie inside the region this process reserved,
    // and `thunk` is a valid slice for its own length. The region outlives the run.
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
/// Idempotent: a second call with the region already mapped succeeds and changes nothing.
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
/// `false` when no skeleton is present: the region is opt-in and its absence is an ordinary
/// state.
#[must_use]
pub fn is_firmware_address(address: u64) -> bool {
    cell()
        .lock()
        .is_ok_and(|held| held.as_ref().is_some_and(|f| f.contains(address)))
}

/// The syscall-gadget page this process holds, once reserved.
///
/// Separate from [`cell`]: a freestanding payload reaches the gadget without reaching the
/// rest of the image, so a run serves it whether or not the skeleton is present.
fn console_gadget_cell() -> &'static Mutex<Option<Firmware>> {
    static CELL: OnceLock<Mutex<Option<Firmware>>> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(None))
}

/// Reserves the syscall-gadget page and writes `trampoline` at
/// [`CONSOLE_SYSCALL_GADGET_VADDR`].
///
/// `trampoline` is the run's own jump into this process's syscall gadget, built by the caller
/// so this crate does not depend on the gadget's owner. Idempotent: a second call rewrites the
/// trampoline in the page already mapped.
///
/// # Errors
///
/// Propagates a reservation failure from [`Firmware::reserve_at`], or refuses a trampoline that
/// would run past the page.
pub fn present_console_gadget(trampoline: &[u8]) -> Result<(), MemError> {
    let mut held = console_gadget_cell()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if held.is_none() {
        *held = Some(Firmware::reserve_at(
            CONSOLE_SYSCALL_GADGET_BASE,
            CONSOLE_SYSCALL_GADGET_SIZE,
        )?);
    }
    let Some(page) = held.as_ref() else {
        return Err(MemError::HostRefused(
            "no console gadget page is reserved".to_owned(),
        ));
    };
    let at = CONSOLE_SYSCALL_GADGET_BASE.saturating_add(CONSOLE_SYSCALL_GADGET_VADDR);
    let end = at.saturating_add(trampoline.len() as u64);
    if at < page.base() || end > page.end() {
        return Err(MemError::HostRefused(format!(
            "the console syscall gadget at {at:#x} would fall outside its page"
        )));
    }
    let Ok(dest) = usize::try_from(at) else {
        return Err(MemError::HostRefused(
            "gadget address does not fit".to_owned(),
        ));
    };
    // SAFETY: `at`..`end` was just checked to lie inside the page this process reserved
    // read-write-execute, and `trampoline` is a valid slice for its own length. The page outlives
    // the run.
    unsafe {
        std::ptr::copy_nonoverlapping(
            trampoline.as_ptr(),
            std::ptr::with_exposed_provenance_mut::<u8>(dest),
            trampoline.len(),
        );
    }
    Ok(())
}

/// The address a first-party payload calls its syscall gadget at: the base plus the gadget vaddr.
#[must_use]
pub const fn console_gadget_address() -> u64 {
    CONSOLE_SYSCALL_GADGET_BASE + CONSOLE_SYSCALL_GADGET_VADDR
}

/// Whether the syscall-gadget page has been reserved in this process.
#[must_use]
pub fn is_console_gadget_present() -> bool {
    console_gadget_cell()
        .lock()
        .is_ok_and(|held| held.is_some())
}

/// The offset of an address within the reserved skeleton, or `None`.
///
/// Turns a raw fault address into "firmware+0x2885e00".
#[must_use]
pub fn firmware_offset(address: u64) -> Option<u64> {
    cell()
        .lock()
        .ok()
        .and_then(|held| held.as_ref().and_then(|f| f.offset_of(address)))
}

/// The offset of an address within the firmware region, decoded from the constants alone.
///
/// Unlike [`firmware_offset`], this answers whether or not a skeleton was reserved, so a fault
/// reporter can name a firmware address after the guest has stopped, in any process.
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

    /// The handed base is inside the region, far enough from either edge that payload offsets
    /// below and above it stay inside.
    #[test]
    fn the_handed_base_leaves_room_both_ways() {
        let base = handed_base();
        assert!(base > FIRMWARE_BASE, "room below for a negative offset");
        assert!(
            base + 64 * 1024 * 1024 < FIRMWARE_BASE + FIRMWARE_SIZE,
            "room above for the largest offsets observed"
        );
        // The largest below-offset a payload uses is 0xd50000.
        assert!(
            base - 0x00d5_0000 > FIRMWARE_BASE,
            "a below-offset stays mapped"
        );
    }

    /// `contains` and `offset_of` agree, and both refuse an address outside the region.
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

    /// The syscall-gadget page reserves, holds the trampoline it was handed, and reads it back at
    /// the gadget address.
    #[test]
    fn the_console_gadget_page_holds_the_trampoline_it_was_given() {
        use super::{
            CONSOLE_SYSCALL_GADGET_BASE, CONSOLE_SYSCALL_GADGET_VADDR, console_gadget_address,
            is_console_gadget_present, present_console_gadget,
        };
        // `mov r11, imm64; jmp r11`, the shape a run writes.
        let trampoline: [u8; 13] = [
            0x49, 0xBB, 0xEF, 0xBE, 0xAD, 0xDE, 0x00, 0x00, 0x00, 0x00, 0x41, 0xFF, 0xE3,
        ];
        present_console_gadget(&trampoline).expect("the console gadget page reserves and places");
        assert!(
            is_console_gadget_present(),
            "the page reports itself present"
        );
        assert_eq!(
            console_gadget_address(),
            CONSOLE_SYSCALL_GADGET_BASE + CONSOLE_SYSCALL_GADGET_VADDR,
            "the address a payload calls is the base plus the gadget vaddr"
        );
        let at = usize::try_from(console_gadget_address()).expect("the gadget address fits");
        // SAFETY: `present_console_gadget` reserved this page read-write-execute and wrote the
        // trampoline at exactly this address, so reading the same 13 bytes back is in bounds.
        let read_back = unsafe {
            std::slice::from_raw_parts(
                std::ptr::with_exposed_provenance::<u8>(at),
                trampoline.len(),
            )
        };
        assert_eq!(
            read_back, &trampoline,
            "the trampoline reads back byte for byte"
        );
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

    /// With the real 0x20-byte packing the compact anchor fits and does not collide (D407).
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
        // Two names at one vaddr are aliases sharing a stub, not a collision; a distinct neighbour
        // inside the stub's length is a real overlap.
        let exports = vec![
            ("listen".to_owned(), 0xcd0),
            ("_listen".to_owned(), 0xcd0),
            ("tight".to_owned(), 0xcd4), // 4 bytes after: inside a 13-byte trampoline
        ];
        let plan = super::plan_layout(&exports, |_| true);
        let listen = plan.iter().find(|p| p.name == "listen").expect("listen");
        // listen at 0xcd0, next distinct export tight at 0xcd4 (4 < 13): a real overlap.
        assert_eq!(
            listen.collides_with,
            Some(("tight".to_owned(), 0xcd4)),
            "a distinct neighbour inside the stub length overlaps"
        );
        let alias = plan.iter().find(|p| p.name == "_listen").expect("_listen");
        // The alias's own next is also an overlap; the alias pair is not reported against each
        // other.
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
