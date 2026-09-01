//! Placing a container's segments into reserved memory.
//!
//! Step two of loading, after parsing and before relocation. It takes a parsed
//! container and a reserved span and copies each loadable segment to where the guest
//! expects to find it.
//!
//! # Two sizes per segment, and the difference matters
//!
//! A segment has `p_filesz` bytes in the container and occupies `p_memsz` bytes in
//! memory. When `memsz` exceeds `filesz` the remainder is `.bss` and **must be
//! zeroed** - the guest is entitled to assume it is. Leaving it as whatever the
//! allocator returned produces a guest that works or fails depending on what ran
//! before it, which is the least debuggable failure there is.

use orbistoun_elf::Container;
use orbistoun_mem::{AddressSpace, Protection};

use crate::LoadError;

/// One segment as placed in memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlacedSegment {
    /// Program header index.
    pub index: usize,
    /// Address the segment starts at.
    pub address: u64,
    /// Bytes copied from the container.
    pub copied: u64,
    /// Bytes zeroed beyond what the container held.
    pub zeroed: u64,
    /// The segment's `p_flags`, kept so protection can be applied after relocation.
    pub flags: u32,
}

impl PlacedSegment {
    /// Bytes the segment occupies in memory - what it was copied plus its `.bss`.
    pub const fn memsz(&self) -> u64 {
        self.copied.saturating_add(self.zeroed)
    }

    /// The access the segment's header asked for.
    pub const fn protection(&self) -> Protection {
        Protection::from_elf_flags(self.flags)
    }
}

/// A container placed in memory, ready for relocation.
///
/// Holds the address space, so the mapping lives exactly as long as the image does.
#[derive(Debug)]
pub struct Image {
    space: AddressSpace,
    base: u64,
    entry: u64,
    span: (u64, u64),
    segments: Vec<PlacedSegment>,
}

/// Writes each eight-byte slot of `.bss` with a marker holding its own guest address.
///
/// # What this buys over a constant byte
///
/// A constant establishes *that* a guest reads uninitialised `.bss` - which is what found
/// the payload wall (D359). It cannot say **which** global, and "an unknown number of
/// globals" was the reason that route looked worse than working out the handoff structure.
///
/// A marker carrying the slot's own address answers it: the guest loads one, uses it, and
/// faults on a value that reads back as `bss+offset`. One boot names the global.
///
/// The top byte is the fill byte, so a marker is recognisable on sight and cannot be
/// confused with a real guest address.
fn mark_bss(tail: usize, len: u64, byte: u8) {
    let byte = u64::from(byte);
    let slots = usize::try_from(len / 8).unwrap_or(0);
    for slot in 0..slots {
        let offset = (slot as u64).saturating_mul(8);
        // Recognisable prefix, then the address this slot occupies in the guest.
        let Ok(at) = usize::try_from(u64::try_from(tail).unwrap_or(0).saturating_add(offset))
        else {
            return;
        };
        let marker = (byte << 56) | (at as u64 & 0x00FF_FFFF_FFFF_FFFF);
        // SAFETY: inside the same `.bss` tail just filled, which the reservation covers.
        unsafe { std::ptr::write(std::ptr::with_exposed_provenance_mut::<u64>(at), marker) };
    }
}

/// What `.bss` is filled with - zero, unless a run asked for something it can recognise.
///
/// # Why this is worth being able to change
///
/// Zero is **correct**: C guarantees it and the guest is entitled to assume it. But zero is
/// also what an uninitialised function pointer looks like, and what a guest that never ran
/// its runtime start looks like. A guest that jumps to null and a guest that reads a global
/// nobody set produce the identical fault, and no amount of staring at `0x0` separates them.
///
/// Filling with something else does. Entering the payloads at `main` skips `__crt_start`
/// (D343), and both of them then jumped to null out of their own `find_pid`. Three
/// candidates were eliminated by experiment - the `sysctl` refusal, `signal`'s return value,
/// the zeroed data-import storage - and this is what found it: with `.bss` filled the fault
/// changed completely, which is only possible if the guest was reading it (D359).
///
/// **A diagnostic, and a run under it is not an ordinary run.** Zeroed `.bss` is the
/// contract; this deliberately breaks it to see who notices.
///
/// Read once. A fill that changed part-way through a load would make the run
/// unreproducible in the one dimension it exists to measure.
/// The fill byte, read once.
static BSS_BYTE: std::sync::OnceLock<u8> = std::sync::OnceLock::new();

fn bss_byte() -> u8 {
    *BSS_BYTE.get_or_init(|| parse_fill(orbistoun_env::BSS_FILL.get().as_deref()))
}

/// Reads a fill byte from what a run asked for.
///
/// Pure, so the one thing that can go wrong here - a value that does not parse silently
/// becoming a fill nobody asked for - is testable without an environment.
///
/// **Zero for anything unusable**, which is the contract: an unparseable value means the
/// diagnostic is off, and off is `.bss` behaving exactly as C promises. The alternative is
/// a run that quietly poisons memory because somebody typed `0xzz`.
fn parse_fill(asked: Option<&str>) -> u8 {
    asked
        .and_then(|raw| u8::from_str_radix(raw.trim_start_matches("0x"), 16).ok())
        .unwrap_or(0)
}

impl Image {
    /// Where the image was placed.
    pub const fn base(&self) -> u64 {
        self.base
    }

    /// The guest entry point, adjusted for the placement base.
    pub const fn entry(&self) -> u64 {
        self.entry
    }

    /// The reserved span as `(start, length)`.
    pub const fn span(&self) -> (u64, u64) {
        self.span
    }

    /// Every segment as placed.
    pub fn segments(&self) -> &[PlacedSegment] {
        &self.segments
    }

    /// The address space backing this image.
    pub const fn space(&self) -> &AddressSpace {
        &self.space
    }

    /// The address space backing this image, mutably.
    ///
    /// Needed to re-protect after population: the image is written as read-write and
    /// only then made executable.
    pub const fn space_mut(&mut self) -> &mut AddressSpace {
        &mut self.space
    }

    /// The segment containing `address`, if any.
    pub fn segment_containing(&self, address: u64) -> Option<&PlacedSegment> {
        self.segments
            .iter()
            .find(|s| address >= s.address && address < s.address.saturating_add(s.memsz()))
    }

    /// Whether the entry point lies in a segment the guest may execute.
    ///
    /// Checked before handing over control, because jumping to an address that is not
    /// executable is certain to fault - and the fault alone says nothing about why. A
    /// refusal here names the actual problem: the entry is outside every executable
    /// segment, which usually means the container declares one this loader did not
    /// place, or the entry was never adjusted for the placement base.
    pub fn entry_is_executable(&self) -> bool {
        self.is_executable(self.entry)
    }

    /// Whether `address` lies inside a segment the guest may execute.
    ///
    /// Generalised from [`Self::entry_is_executable`] because a run may be told to start
    /// somewhere other than the declared entry - a diagnostic, and one that must be
    /// refused just as loudly when it points at data (D326).
    pub fn is_executable(&self, address: u64) -> bool {
        self.segment_containing(address)
            .is_some_and(|s| s.protection().execute)
    }

    /// Total bytes copied from the container.
    pub fn bytes_copied(&self) -> u64 {
        self.segments.iter().map(|s| s.copied).sum()
    }

    /// Total bytes zeroed as `.bss`.
    pub fn bytes_zeroed(&self) -> u64 {
        self.segments.iter().map(|s| s.zeroed).sum()
    }
}

/// `PT_LOAD`.
const PT_LOAD: u32 = 1;

/// Places every loadable segment of a container at `base`.
///
/// The whole span is reserved once rather than per segment (D054), then written
/// through.
pub fn place(whole: &[u8], base: u64, page: u64) -> Result<Image, LoadError> {
    let container = Container::parse(whole)?;
    let headers = container.program_headers()?;

    let loadable: Vec<(usize, orbistoun_elf::Elf64ProgramHeader)> = headers
        .iter()
        .enumerate()
        .filter(|(_, ph)| ph.p_type.get() == PT_LOAD && ph.memsz.get() > 0)
        .map(|(i, ph)| (i, *ph))
        .collect();

    if loadable.is_empty() {
        return Err(LoadError::NothingToLoad);
    }

    let lowest = loadable
        .iter()
        .map(|(_, ph)| base.saturating_add(ph.vaddr.get()))
        .min()
        .unwrap_or(base);
    let highest = loadable
        .iter()
        .map(|(_, ph)| {
            base.saturating_add(ph.vaddr.get())
                .saturating_add(ph.memsz.get())
        })
        .max()
        .unwrap_or(base);

    // The span base must satisfy the **host allocation granularity**, which is
    // coarser than the guest page on Windows (64 KiB). Rounding only to the page size
    // passes on Unix and is refused on Windows - the worst kind of platform
    // difference, and one that only showed up by running it.
    let granularity = orbistoun_mem::allocation_granularity().max(page);
    let span_base = lowest / granularity * granularity;
    let span_len = highest.div_ceil(page).saturating_mul(page) - span_base;

    let mut space = AddressSpace::new();
    // Read-write for population; per-segment protection is applied afterwards, once
    // the bytes are in place.
    space.reserve(span_base, span_len, Protection::READ_WRITE)?;

    let mut segments = Vec::new();
    for (index, ph) in loadable {
        let address = base.saturating_add(ph.vaddr.get());
        let memsz = ph.memsz.get();
        let data = container.segment_data(whole, index)?.unwrap_or(&[]);
        let copied = u64::try_from(data.len()).unwrap_or(0).min(ph.filesz.get());

        let dest = usize::try_from(address).map_err(|_| LoadError::AddressTooLarge(address))?;
        let copy_len = usize::try_from(copied).unwrap_or(0);

        // SAFETY: the span reserved above covers `address .. address + memsz` by
        // construction, so this writes into memory this function exclusively owns. The
        // source is a distinct borrow of the container and cannot overlap a freshly
        // mapped region.
        unsafe {
            std::ptr::copy_nonoverlapping(
                data.as_ptr(),
                std::ptr::with_exposed_provenance_mut::<u8>(dest),
                copy_len,
            );
        }

        // `.bss`: everything beyond what the container held must read as zero.
        let zeroed = memsz.saturating_sub(copied);
        if zeroed > 0 {
            let tail = dest.saturating_add(copy_len);
            // SAFETY: same reservation, and `copied + zeroed == memsz`, so this stays
            // inside the segment and therefore inside the span.
            unsafe {
                std::ptr::write_bytes(
                    std::ptr::with_exposed_provenance_mut::<u8>(tail),
                    bss_byte(),
                    usize::try_from(zeroed).unwrap_or(0),
                );
            }
            // **Markers that name themselves.** A constant byte says only *that* the guest
            // depends on `.bss`; a marker carrying its own address says *which* global,
            // because the fault reports the value the guest used (D360). The identity
            // mapping (D014) makes the host address the guest sees the same one.
            if bss_byte() != 0 {
                mark_bss(tail, zeroed, bss_byte());
            }
        }

        segments.push(PlacedSegment {
            index,
            address,
            copied,
            zeroed,
            flags: ph.flags.get(),
        });
    }

    Ok(Image {
        space,
        base,
        entry: base.saturating_add(container.entry()),
        span: (span_base, span_len),
        segments,
    })
}

#[cfg(test)]
mod tests {
    /// **A marker decodes back to the address it sits at.**
    ///
    /// The whole point of marking rather than filling with a constant: a guest that loads a
    /// global and uses it as an address faults on a value that says *which* global. It has
    /// not fired on a guest yet - both payloads read `.bss` and derive something else - so
    /// this is what shows the mechanism is right rather than merely present (D360).
    #[test]
    fn a_bss_marker_names_the_slot_it_occupies() {
        const SLOTS: usize = 4;
        let mut region = [0_u64; SLOTS];
        let at = std::ptr::from_mut(&mut region).cast::<u8>() as usize;

        super::mark_bss(at, (SLOTS * 8) as u64, 0xB5);

        for (slot, held) in region.iter().enumerate() {
            assert_eq!(
                held >> 56,
                0xB5,
                "slot {slot} must be recognisable as a marker on sight"
            );
            assert_eq!(
                held & 0x00FF_FFFF_FFFF_FFFF,
                (at + slot * 8) as u64 & 0x00FF_FFFF_FFFF_FFFF,
                "and must name its own address"
            );
        }
    }

    /// **An unusable value leaves `.bss` zeroed**, rather than poisoning it by accident.
    ///
    /// Zeroed is the contract C guarantees; the fill deliberately breaks it to find a guest
    /// that depends on a global nobody initialised (D359). A typo silently turning that on
    /// would make an ordinary run behave like a diagnostic one, which is the worst of both.
    #[test]
    fn a_fill_is_only_applied_when_it_was_actually_asked_for() {
        assert_eq!(super::parse_fill(None), 0, "no request, no fill");
        assert_eq!(super::parse_fill(Some("b5")), 0xB5);
        assert_eq!(
            super::parse_fill(Some("0xb5")),
            0xB5,
            "the 0x prefix is optional"
        );
        assert_eq!(
            super::parse_fill(Some("zz")),
            0,
            "a value that does not parse is off"
        );
        assert_eq!(super::parse_fill(Some("")), 0);
        assert_eq!(
            super::parse_fill(Some("100")),
            0,
            "a byte that does not fit is off"
        );
    }

    use super::place;
    use crate::LoadError;

    /// A bare ELF with one loadable segment. **Generated, never extracted** (D051).
    ///
    /// `filesz` and `memsz` are supplied separately so the `.bss` behaviour can be
    /// exercised, which is the part most easily got wrong.
    fn elf_with_segment(vaddr: u64, filesz: u64, memsz: u64, fill: u8) -> Vec<u8> {
        const EHDR: usize = 64;
        const PHDR: usize = 56;
        let data_at = EHDR + PHDR;

        let mut v = vec![0_u8; data_at];
        v[..4].copy_from_slice(b"\x7fELF");
        v[4] = 2; // 64-bit
        v[5] = 1; // little-endian
        v[7] = 9; // FreeBSD, as real material carries
        v[16..18].copy_from_slice(&0xFE18_u16.to_le_bytes()); // e_type
        v[18..20].copy_from_slice(&0x3E_u16.to_le_bytes()); // x86-64
        v[24..32].copy_from_slice(&vaddr.to_le_bytes()); // e_entry
        v[32..40].copy_from_slice(&(EHDR as u64).to_le_bytes()); // e_phoff
        v[54..56].copy_from_slice(&(PHDR as u16).to_le_bytes()); // e_phentsize
        v[56..58].copy_from_slice(&1_u16.to_le_bytes()); // e_phnum

        let p = EHDR;
        v[p..p + 4].copy_from_slice(&1_u32.to_le_bytes()); // PT_LOAD
        v[p + 4..p + 8].copy_from_slice(&6_u32.to_le_bytes()); // RW
        v[p + 8..p + 16].copy_from_slice(&(data_at as u64).to_le_bytes()); // p_offset
        v[p + 16..p + 24].copy_from_slice(&vaddr.to_le_bytes()); // p_vaddr
        v[p + 32..p + 40].copy_from_slice(&filesz.to_le_bytes());
        v[p + 40..p + 48].copy_from_slice(&memsz.to_le_bytes());

        v.extend(std::iter::repeat_n(
            fill,
            usize::try_from(filesz).expect("small"),
        ));
        v
    }

    /// Far from anything a normal process maps, so a test is about the mechanism
    /// rather than about luck.
    const TEST_BASE: u64 = 0x0000_5000_0000_0000;

    #[test]
    fn segment_bytes_land_at_the_address_the_guest_expects() {
        let bytes = elf_with_segment(0x1000, 64, 64, 0xAB);
        let image = place(&bytes, TEST_BASE, 4096).expect("place");

        let at = image.segments()[0].address;
        assert_eq!(at, TEST_BASE + 0x1000);

        // SAFETY: the image holds the reservation covering this address, and 64 bytes
        // were just copied there.
        let seen = unsafe {
            std::slice::from_raw_parts(
                std::ptr::with_exposed_provenance::<u8>(usize::try_from(at).expect("fits")),
                64,
            )
        };
        assert!(seen.iter().all(|b| *b == 0xAB), "segment content is wrong");
    }

    #[test]
    fn bss_beyond_the_container_is_zeroed() {
        // The guest is entitled to assume this. Leaving it as whatever the allocator
        // returned makes a guest work or fail depending on what ran before it.
        let bytes = elf_with_segment(0x1000, 16, 4096, 0xFF);
        let image = place(&bytes, TEST_BASE + 0x10_0000, 4096).expect("place");

        let seg = image.segments()[0];
        assert_eq!(seg.copied, 16);
        assert_eq!(seg.zeroed, 4096 - 16);

        // SAFETY: the image holds the reservation covering this segment.
        let seen = unsafe {
            std::slice::from_raw_parts(
                std::ptr::with_exposed_provenance::<u8>(
                    usize::try_from(seg.address).expect("fits"),
                ),
                4096,
            )
        };
        assert!(seen[..16].iter().all(|b| *b == 0xFF), "copied part");
        assert!(seen[16..].iter().all(|b| *b == 0), "bss must be zero");
    }

    #[test]
    fn the_entry_point_is_adjusted_for_the_placement_base() {
        // A module links at zero, so an unadjusted entry would jump into whatever
        // happens to live at the raw address.
        let bytes = elf_with_segment(0x2000, 32, 32, 0x11);
        let image = place(&bytes, TEST_BASE + 0x20_0000, 4096).expect("place");
        assert_eq!(image.entry(), TEST_BASE + 0x20_0000 + 0x2000);
    }

    #[test]
    fn the_span_is_page_aligned_outwards() {
        let bytes = elf_with_segment(0x1234, 16, 16, 0x22);
        let image = place(&bytes, TEST_BASE + 0x30_0000, 4096).expect("place");
        let (span_base, span_len) = image.span();
        assert_eq!(span_base % 4096, 0, "span must start on a page");
        assert_eq!(span_len % 4096, 0, "span must be whole pages");
        assert!(span_base <= image.segments()[0].address);
    }

    #[test]
    fn a_container_with_nothing_loadable_is_refused() {
        // Returning an empty image would look like a successful load of a module that
        // does nothing, which is never what happened.
        let mut bytes = elf_with_segment(0x1000, 16, 16, 0x33);
        bytes[64..68].copy_from_slice(&0_u32.to_le_bytes()); // PT_NULL
        assert!(matches!(
            place(&bytes, TEST_BASE + 0x40_0000, 4096),
            Err(LoadError::NothingToLoad)
        ));
    }

    #[test]
    fn totals_account_for_every_byte() {
        let bytes = elf_with_segment(0x1000, 100, 500, 0x44);
        let image = place(&bytes, TEST_BASE + 0x50_0000, 4096).expect("place");
        assert_eq!(image.bytes_copied(), 100);
        assert_eq!(image.bytes_zeroed(), 400);
    }
}
