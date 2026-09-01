//! The vendor container wrapper.
//!
//! Real executables are not plain ELFs (D049). They are wrapped: a fixed-size header,
//! a table of segment descriptors, then the inner ELF image.
//!
//! ```text
//! 0x00  magic  54 14 f5 ee
//! 0x0C  header_size, meta_size
//! 0x10  stated_size (NOT the file length)
//! 0x18  segment_count
//! 0x20  segment_count x 32-byte descriptors
//!  ...  inner ELF
//! ```
//!
//! # The offset is derived and then verified, never assumed
//!
//! The inner ELF begins after the descriptor table, so its offset is
//! `HEADER_SIZE + segment_count * SEGMENT_SIZE`. Every file inspected put it at 416,
//! consistent with twelve descriptors - but 416 is an *observation*, not a constant,
//! and hardcoding it would silently mis-parse anything with a different count.
//!
//! So the offset is computed, and then the ELF magic is checked at that offset. If the
//! derivation is wrong the parse fails loudly (D010) rather than reading whatever
//! happened to be there.

use zerocopy::{FromBytes, Immutable, KnownLayout, little_endian};

use crate::ElfError;

/// Magic at offset zero of a wrapped container.
pub const WRAPPER_MAGIC: [u8; 4] = [0x54, 0x14, 0xf5, 0xee];

/// Magic used by the previous console generation's wrapper.
///
/// Recognised only so the error can say *which* format it is rather than "not a
/// container". Both generations coexist inside a single title: bundled modules use
/// the current format, substituted stub libraries the older one.
pub const PREVIOUS_GENERATION_MAGIC: [u8; 4] = [0x4f, 0x15, 0x3d, 0x1d];

/// Which console generation a container was built for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Generation {
    /// The current target.
    Current,
    /// The previous console.
    Previous,
}

impl Generation {
    /// How to name it in a report.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Current => "current generation",
            Self::Previous => "previous generation",
        }
    }
}

/// Size of the wrapper header.
pub const HEADER_SIZE: usize = 32;

/// Size of one segment descriptor.
pub const SEGMENT_SIZE: usize = 32;

/// Upper bound on the descriptor count, to reject an absurd header before it is used
/// for arithmetic. Real files observed carry twelve.
pub const MAX_SEGMENTS: u16 = 4096;

/// The wrapper header, exactly as it appears on disk.
#[derive(Debug, Clone, Copy, FromBytes, Immutable, KnownLayout)]
#[repr(C)]
pub struct WrapperHeader {
    /// Magic - see [`WRAPPER_MAGIC`].
    pub magic: [u8; 4],
    /// Format version.
    pub version: u8,
    /// Mode.
    pub mode: u8,
    /// Endianness marker.
    pub endian: u8,
    /// Attribute bits.
    pub attributes: u8,
    /// Key type.
    pub key_type: little_endian::U32,
    /// Size of the header region.
    pub header_size: little_endian::U16,
    /// Size of the metadata region.
    pub meta_size: little_endian::U16,
    /// A size the header states.
    ///
    /// **Not the file length.** Observed consistently *smaller* than the file, by a
    /// variable amount (38 bytes on one module, 10,670 on an executable), so it
    /// measures some region rather than the whole container. What region is not yet
    /// established, so nothing is inferred from it (D010) - it is reported and left
    /// alone.
    pub stated_size: little_endian::U64,
    /// Number of segment descriptors following the header.
    pub segment_count: little_endian::U16,
    /// Flags.
    pub flags: little_endian::U16,
    /// Padding to 32 bytes.
    pub reserved: little_endian::U32,
}

/// Bit in a segment's flags marking it as carrying program-header data.
///
/// Descriptors come in pairs: one small block per data segment (0x20 or 0x60 bytes,
/// almost certainly digests) and one carrying the actual bytes. Only the latter has
/// this bit.
pub const SEGMENT_FLAG_HAS_DATA: u64 = 0x800;

/// Shift applied to a segment's flags to recover the program-header index it serves.
pub const SEGMENT_PHDR_INDEX_SHIFT: u32 = 20;

/// One segment descriptor.
///
/// # How the wrapper and the inner ELF relate
///
/// The inner ELF's program headers describe a *virtual* layout, and their `p_offset`
/// values routinely point past the end of the container - so they are not file
/// offsets. The wrapper's descriptors are what actually locate the bytes: each
/// data-bearing descriptor names a program-header index in the top bits of its flags,
/// and its `stored_size` equals that header's `p_filesz`.
///
/// Verified against real material: every data-bearing descriptor matched its program
/// header's size exactly, across an executable and three modules.
#[derive(Debug, Clone, Copy, FromBytes, Immutable, KnownLayout)]
#[repr(C)]
pub struct WrapperSegment {
    /// Segment flags.
    pub flags: little_endian::U64,
    /// File offset of the segment's data.
    pub offset: little_endian::U64,
    /// Size of the data as stored.
    pub stored_size: little_endian::U64,
    /// Size of the data once expanded.
    pub expanded_size: little_endian::U64,
}

impl WrapperSegment {
    /// Which program header this descriptor serves.
    ///
    /// **Only meaningful when [`Self::has_data`] is true.** On the paired metadata
    /// blocks the same bits carry something else - observed holding the *wrapper
    /// table* index of the data descriptor they accompany, not a program-header
    /// index - so reading it off a metadata block is a mistake.
    pub fn program_header_index(&self) -> usize {
        (self.flags.get() >> SEGMENT_PHDR_INDEX_SHIFT) as usize
    }

    /// Whether this descriptor carries program-header data rather than metadata.
    pub fn has_data(&self) -> bool {
        self.flags.get() & SEGMENT_FLAG_HAS_DATA != 0
    }

    /// The byte range this descriptor occupies in the container.
    ///
    /// Saturating: a corrupt descriptor claiming an enormous size must produce a
    /// range that fails the bounds check, never an arithmetic panic. Parsers see
    /// hostile input by definition.
    pub fn range(&self) -> std::ops::Range<usize> {
        let start = self.offset.get() as usize;
        let end = start.saturating_add(self.stored_size.get() as usize);
        start..end
    }
}

/// A parsed wrapper, and where the inner ELF starts.
#[derive(Debug, Clone, Copy)]
pub struct Wrapper {
    header: WrapperHeader,
    elf_offset: usize,
}

impl Wrapper {
    /// Whether `bytes` starts with the current-generation wrapper magic.
    pub fn is_wrapped(bytes: &[u8]) -> bool {
        bytes.len() >= 4 && bytes[..4] == WRAPPER_MAGIC
    }

    /// Whether `bytes` starts with the previous generation's wrapper magic.
    pub fn is_previous_generation(bytes: &[u8]) -> bool {
        bytes.len() >= 4 && bytes[..4] == PREVIOUS_GENERATION_MAGIC
    }

    /// Which generation's wrapper `bytes` carries, if either.
    ///
    /// Reported rather than flattened: the two parse identically, but a title built for
    /// the previous console is a different emulation problem, and a report that cannot
    /// say which one it read is hiding the single most useful fact about it.
    pub fn generation(bytes: &[u8]) -> Option<Generation> {
        if Self::is_wrapped(bytes) {
            Some(Generation::Current)
        } else if Self::is_previous_generation(bytes) {
            Some(Generation::Previous)
        } else {
            None
        }
    }

    /// Whether `bytes` starts with either generation's wrapper magic.
    ///
    /// **The two headers are byte-for-byte the same shape.** Read with the current
    /// layout, a previous-generation header yields the same version, mode, endianness,
    /// attributes, key type and header sizes, and a segment count and descriptors that
    /// are all plausible. The generations differ in the four magic bytes and in what the
    /// segments contain - not in how the wrapper is read (D176).
    pub fn is_either_generation(bytes: &[u8]) -> bool {
        Self::is_wrapped(bytes) || Self::is_previous_generation(bytes)
    }

    /// Parses the wrapper and locates the inner ELF.
    pub fn parse(bytes: &[u8]) -> Result<Self, ElfError> {
        if !Self::is_either_generation(bytes) {
            return Err(ElfError::NotWrapped);
        }

        let header = WrapperHeader::read_from_prefix(bytes)
            .map(|(h, _)| h)
            .map_err(|_| ElfError::Truncated {
                offset: 0,
                need: HEADER_SIZE,
                have: bytes.len(),
            })?;

        let count = header.segment_count.get();
        if count > MAX_SEGMENTS {
            return Err(ElfError::AbsurdSegmentCount {
                count,
                max: MAX_SEGMENTS,
            });
        }

        // Derived, not assumed. Checked below against the ELF magic.
        let elf_offset = HEADER_SIZE + (count as usize) * SEGMENT_SIZE;
        if elf_offset + 4 > bytes.len() {
            return Err(ElfError::Truncated {
                offset: elf_offset,
                need: 4,
                have: bytes.len(),
            });
        }
        if bytes[elf_offset..elf_offset + 4] != *b"\x7fELF" {
            return Err(ElfError::InnerElfNotFound {
                expected_at: elf_offset,
                segment_count: count,
            });
        }

        Ok(Self { header, elf_offset })
    }

    /// The parsed header.
    pub const fn header(&self) -> &WrapperHeader {
        &self.header
    }

    /// Byte offset of the inner ELF image.
    pub const fn elf_offset(&self) -> usize {
        self.elf_offset
    }

    /// Number of segment descriptors.
    pub fn segment_count(&self) -> u16 {
        self.header.segment_count.get()
    }

    /// The segment descriptor table.
    pub fn segments(&self, bytes: &[u8]) -> Result<Vec<WrapperSegment>, ElfError> {
        let count = self.segment_count() as usize;
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let at = HEADER_SIZE + i * SEGMENT_SIZE;
            let seg = WrapperSegment::read_from_prefix(&bytes[at..])
                .map(|(s, _)| s)
                .map_err(|_| ElfError::Truncated {
                    offset: at,
                    need: SEGMENT_SIZE,
                    have: bytes.len().saturating_sub(at),
                })?;
            out.push(seg);
        }
        Ok(out)
    }

    /// The size the header states, whatever it measures.
    pub fn stated_size(&self) -> u64 {
        self.header.stated_size.get()
    }

    /// The bytes backing a given program header, located through the descriptor table.
    ///
    /// `None` when no descriptor serves that header - which is normal and not an
    /// error: several program headers describe regions *inside* another header's data
    /// rather than having their own descriptor.
    pub fn data_for_program_header<'a>(
        &self,
        bytes: &'a [u8],
        index: usize,
    ) -> Result<Option<&'a [u8]>, ElfError> {
        for seg in self.segments(bytes)? {
            if !seg.has_data() || seg.program_header_index() != index {
                continue;
            }
            let range = seg.range();
            if range.end > bytes.len() {
                return Err(ElfError::Truncated {
                    offset: range.start,
                    need: range.len(),
                    have: bytes.len().saturating_sub(range.start.min(bytes.len())),
                });
            }
            return Ok(Some(&bytes[range]));
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Generation, HEADER_SIZE, MAX_SEGMENTS, PREVIOUS_GENERATION_MAGIC, SEGMENT_SIZE,
        WRAPPER_MAGIC, Wrapper,
    };
    use crate::ElfError;

    /// Builds a wrapper around a minimal inner ELF. **Generated, never extracted**
    /// (D051): this is constructed from the documented structure, not carved out of
    /// any real file.
    fn wrapped(segment_count: u16, inner: &[u8]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&WRAPPER_MAGIC);
        v.extend_from_slice(&[0x00, 0x01, 0x01, 0x12]); // version, mode, endian, attrs
        v.extend_from_slice(&1_u32.to_le_bytes()); // key_type
        v.extend_from_slice(&0x0560_u16.to_le_bytes()); // header_size
        v.extend_from_slice(&0x0610_u16.to_le_bytes()); // meta_size
        let total = HEADER_SIZE + segment_count as usize * SEGMENT_SIZE + inner.len();
        v.extend_from_slice(&(total as u64).to_le_bytes()); // stated_size
        v.extend_from_slice(&segment_count.to_le_bytes());
        v.extend_from_slice(&0x22_u16.to_le_bytes()); // flags
        v.extend_from_slice(&0_u32.to_le_bytes()); // reserved
        assert_eq!(v.len(), HEADER_SIZE);

        for i in 0..segment_count {
            v.extend_from_slice(&0x0011_0004_u64.to_le_bytes());
            v.extend_from_slice(&(0x0b70_u64 + u64::from(i)).to_le_bytes());
            v.extend_from_slice(&0x0001_9cc0_u64.to_le_bytes());
            v.extend_from_slice(&0x0001_9cc0_u64.to_le_bytes());
        }
        v.extend_from_slice(inner);
        v
    }

    /// A wrapper whose descriptors carry the given `(flags, size)` pairs, with real
    /// backing bytes after the inner ELF so ranges resolve.
    fn wrapped_with_flags(descriptors: &[(u64, u64)]) -> Vec<u8> {
        let inner = minimal_elf();
        let count = descriptors.len() as u16;
        let data_start = HEADER_SIZE + descriptors.len() * SEGMENT_SIZE + inner.len();
        let total: u64 = descriptors.iter().map(|(_, size)| size).sum();

        let mut v = Vec::new();
        v.extend_from_slice(&WRAPPER_MAGIC);
        v.extend_from_slice(&[0x00, 0x01, 0x01, 0x12]);
        v.extend_from_slice(&1_u32.to_le_bytes());
        v.extend_from_slice(&0x0560_u16.to_le_bytes());
        v.extend_from_slice(&0x0610_u16.to_le_bytes());
        v.extend_from_slice(&(data_start as u64 + total).to_le_bytes());
        v.extend_from_slice(&count.to_le_bytes());
        v.extend_from_slice(&0x22_u16.to_le_bytes());
        v.extend_from_slice(&0_u32.to_le_bytes());

        let mut at = data_start as u64;
        for (flags, size) in descriptors {
            v.extend_from_slice(&flags.to_le_bytes());
            v.extend_from_slice(&at.to_le_bytes());
            v.extend_from_slice(&size.to_le_bytes());
            v.extend_from_slice(&size.to_le_bytes());
            at += size;
        }
        v.extend_from_slice(&inner);
        v.resize(data_start + total as usize, 0xAB);
        v
    }

    fn minimal_elf() -> Vec<u8> {
        let mut v = vec![0_u8; 64];
        v[..4].copy_from_slice(b"\x7fELF");
        v[4] = 2; // 64-bit
        v[5] = 1; // little-endian
        v[7] = 9; // ELFOSABI_FREEBSD, as observed on real material
        v
    }

    #[test]
    fn the_offset_is_derived_from_the_segment_count_not_hardcoded() {
        // Twelve descriptors is what real files carry, giving 416 - but the parser
        // must compute it, or a file with a different count is mis-parsed silently.
        for count in [0_u16, 1, 7, 12, 40] {
            let bytes = wrapped(count, &minimal_elf());
            let w = Wrapper::parse(&bytes).expect("parses");
            assert_eq!(
                w.elf_offset(),
                HEADER_SIZE + count as usize * SEGMENT_SIZE,
                "offset must follow the count"
            );
        }
        // And the observed case lands where the real files do.
        let w = Wrapper::parse(&wrapped(12, &minimal_elf())).expect("parses");
        assert_eq!(w.elf_offset(), 416);
    }

    #[test]
    fn a_derivation_that_does_not_land_on_an_elf_fails_loudly() {
        // The check that makes deriving safe: if the arithmetic is wrong, say so
        // rather than parsing whatever happened to be at that offset.
        let mut bytes = wrapped(12, &minimal_elf());
        bytes[0x18] = 11; // claim eleven descriptors, so the offset lands 32 bytes early
        let err = Wrapper::parse(&bytes).expect_err("must not guess");
        assert!(
            matches!(err, ElfError::InnerElfNotFound { expected_at, segment_count }
                if expected_at == HEADER_SIZE + 11 * SEGMENT_SIZE && segment_count == 11),
            "got {err:?}"
        );
    }

    #[test]
    fn both_generations_parse_and_are_told_apart() {
        // **This test asserted the opposite.** The previous generation was refused with a
        // named error, on the assumption that it needed different handling - and nobody
        // had checked. Read with the current layout, its header yields the same version,
        // mode, endianness, attributes and key type, a plausible segment count, and
        // descriptors that parse. Two real titles then loaded end to end (D176).
        //
        // Reported separately rather than flattened: they parse the same, but a title
        // built for the previous console is a different emulation problem.
        let mut current = [0_u8; HEADER_SIZE];
        current[..4].copy_from_slice(&WRAPPER_MAGIC);
        let mut previous = [0_u8; HEADER_SIZE];
        previous[..4].copy_from_slice(&PREVIOUS_GENERATION_MAGIC);

        assert_eq!(Wrapper::generation(&current), Some(Generation::Current));
        assert_eq!(Wrapper::generation(&previous), Some(Generation::Previous));
        assert_eq!(Wrapper::generation(b"not a container"), None);
        assert!(Wrapper::is_either_generation(&previous));
    }

    #[test]
    fn a_plain_elf_is_reported_as_unwrapped_not_as_corrupt() {
        assert!(matches!(
            Wrapper::parse(&minimal_elf()),
            Err(ElfError::NotWrapped)
        ));
    }

    #[test]
    fn an_absurd_segment_count_is_rejected_before_it_is_used_for_arithmetic() {
        let mut bytes = wrapped(1, &minimal_elf());
        bytes[0x18..0x1a].copy_from_slice(&u16::MAX.to_le_bytes());
        assert!(matches!(
            Wrapper::parse(&bytes),
            Err(ElfError::AbsurdSegmentCount {
                max: MAX_SEGMENTS,
                ..
            })
        ));
    }

    #[test]
    fn truncation_after_the_header_is_caught() {
        let bytes = wrapped(12, &minimal_elf());
        let cut = &bytes[..HEADER_SIZE + 5 * SEGMENT_SIZE];
        assert!(matches!(
            Wrapper::parse(cut),
            Err(ElfError::Truncated { .. })
        ));
    }

    #[test]
    fn a_header_shorter_than_the_struct_is_caught() {
        assert!(matches!(
            Wrapper::parse(&WRAPPER_MAGIC),
            Err(ElfError::Truncated { .. })
        ));
    }

    #[test]
    fn the_segment_table_parses_and_matches_the_count() {
        let bytes = wrapped(12, &minimal_elf());
        let w = Wrapper::parse(&bytes).expect("parses");
        let segs = w.segments(&bytes).expect("segments");
        assert_eq!(segs.len(), 12);
        assert_eq!(segs[0].offset.get(), 0x0b70);
        assert_eq!(segs[1].offset.get(), 0x0b71, "each descriptor is distinct");
        assert_eq!(segs[0].stored_size.get(), 0x0001_9cc0);
    }

    #[test]
    fn the_stated_size_is_read_but_nothing_is_inferred_from_it() {
        // On real material this field is consistently SMALLER than the file, by a
        // variable amount, so it measures a region rather than the container. It is
        // reported and not used for any check until what it measures is established.
        let bytes = wrapped(12, &minimal_elf());
        let w = Wrapper::parse(&bytes).expect("parses");
        assert_eq!(
            w.stated_size() as usize,
            bytes.len(),
            "as this fixture wrote it"
        );

        let mut padded = bytes.clone();
        padded.extend_from_slice(&[0_u8; 16]);
        let w2 = Wrapper::parse(&padded).expect("parses");
        assert!(
            w2.stated_size() < padded.len() as u64,
            "trailing bytes are exactly the real-world shape; parsing must not care"
        );
    }

    #[test]
    fn segment_flags_decode_to_a_program_header_index_and_a_data_bit() {
        // The relationship verified against real material: a data-bearing descriptor
        // names its program header in the top bits, and its stored size equals that
        // header's filesz.
        let bytes = wrapped_with_flags(&[(0x0000_2804, 0x40), (0x0011_0004, 0x20)]);
        let w = Wrapper::parse(&bytes).expect("parses");
        let segs = w.segments(&bytes).expect("segments");

        assert!(
            segs[0].has_data(),
            "0x800 marks the data-bearing descriptor"
        );
        assert_eq!(segs[0].program_header_index(), 0);
        // The paired metadata block carries no program data, and its index bits mean
        // something else entirely - observed holding the wrapper-table index of the
        // descriptor it accompanies, not a program-header index. Reading it as one
        // would be a mistake, which is why `program_header_index` documents that it
        // is only meaningful when `has_data` is true.
        assert!(
            !segs[1].has_data(),
            "the paired block carries no program data"
        );
    }

    #[test]
    fn program_header_data_is_located_through_the_descriptor_table() {
        // The inner ELF's p_offset values routinely point past end-of-file, so the
        // descriptor table is the only way to reach the bytes.
        let bytes = wrapped_with_flags(&[(0x0030_2804, 0x40)]);
        let w = Wrapper::parse(&bytes).expect("parses");

        let data = w
            .data_for_program_header(&bytes, 3)
            .expect("lookup")
            .expect("descriptor 0 serves program header 3");
        assert_eq!(data.len(), 0x40);

        assert!(
            w.data_for_program_header(&bytes, 9)
                .expect("lookup")
                .is_none(),
            concat!(
                "a header with no descriptor is normal, not an error - several describe ",
                "regions inside another header's data"
            )
        );
    }

    #[test]
    fn a_descriptor_pointing_past_the_end_is_caught() {
        let mut bytes = wrapped_with_flags(&[(0x0000_2804, 0x40)]);
        // Claim a stored size far larger than the container.
        let stored_at = HEADER_SIZE + 16;
        bytes[stored_at..stored_at + 8].copy_from_slice(&u64::MAX.to_le_bytes());
        let w = Wrapper::parse(&bytes).expect("header still parses");
        assert!(matches!(
            w.data_for_program_header(&bytes, 0),
            Err(ElfError::Truncated { .. })
        ));
    }

    #[test]
    fn header_fields_read_back_as_written() {
        let bytes = wrapped(12, &minimal_elf());
        let w = Wrapper::parse(&bytes).expect("parses");
        let h = w.header();
        assert_eq!(h.magic, WRAPPER_MAGIC);
        assert_eq!(h.segment_count.get(), 12);
        assert_eq!(h.header_size.get(), 0x0560);
        assert_eq!(h.meta_size.get(), 0x0610);
        assert_eq!(w.segment_count(), 12);
    }
}
