//! The vendor container wrapper.
//!
//! Real executables are wrapped ELFs (D049): a fixed-size header, a table of segment
//! descriptors, then the inner ELF image.
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
//! The inner ELF begins after the descriptor table, at `HEADER_SIZE + segment_count *
//! SEGMENT_SIZE`. The offset is computed from the count and then verified by the ELF magic
//! there, so a wrong derivation fails loudly rather than reading whatever is at that offset.

use zerocopy::{FromBytes, Immutable, KnownLayout, little_endian};

use crate::ElfError;

/// Magic at offset zero of a wrapped container.
pub const WRAPPER_MAGIC: [u8; 4] = [0x54, 0x14, 0xf5, 0xee];

/// Magic used by the previous generation's wrapper.
///
/// Both generations coexist inside a single title: bundled modules use the current format,
/// substituted stub libraries the older one.
pub const PREVIOUS_GENERATION_MAGIC: [u8; 4] = [0x4f, 0x15, 0x3d, 0x1d];

/// Which hardware generation a container was built for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Generation {
    /// The current target.
    Current,
    /// The previous generation.
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

/// Upper bound on the descriptor count, to reject an absurd header before it is used for
/// arithmetic.
pub const MAX_SEGMENTS: u16 = 4096;

/// The wrapper header, exactly as it appears on disk.
#[derive(Debug, Clone, Copy, FromBytes, Immutable, KnownLayout)]
#[repr(C)]
pub struct WrapperHeader {
    /// Magic; see [`WRAPPER_MAGIC`].
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
    /// A size the header states.
    ///
    /// Not the file length: it is smaller than the file by a variable amount, so it
    /// measures some unidentified region. Nothing is inferred from it (D010).
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
/// Descriptors come in pairs: one small metadata block per data segment (0x20 or 0x60
/// bytes) and one carrying the bytes. Only the latter has this bit.
pub const SEGMENT_FLAG_HAS_DATA: u64 = 0x800;

/// Shift applied to a segment's flags to recover the program-header index it serves.
pub const SEGMENT_PHDR_INDEX_SHIFT: u32 = 20;

/// One segment descriptor.
///
/// The inner ELF's program headers describe a virtual layout, and their `p_offset` values
/// routinely point past the end of the container. The descriptors locate the bytes: each
/// data-bearing descriptor names a program-header index in the top bits of its flags, and
/// its `stored_size` equals that header's `p_filesz`.
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
    /// Only meaningful when [`Self::has_data`] is true. On the paired metadata blocks the
    /// same bits hold the wrapper-table index of the data descriptor they accompany.
    pub fn program_header_index(&self) -> usize {
        (self.flags.get() >> SEGMENT_PHDR_INDEX_SHIFT) as usize
    }

    /// Whether this descriptor carries program-header data rather than metadata.
    pub fn has_data(&self) -> bool {
        self.flags.get() & SEGMENT_FLAG_HAS_DATA != 0
    }

    /// The byte range this descriptor occupies in the container.
    ///
    /// Saturating: a corrupt descriptor claiming an enormous size produces a range that fails
    /// the bounds check, never an arithmetic panic.
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
    /// The two parse identically, but a title built for the previous generation is a
    /// different emulation problem, so a report says which it read.
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
    /// The two headers have the same shape and differ only in the four magic bytes and in
    /// what the segments contain, not in how the wrapper is read (D049).
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

        // Derived, then checked below against the ELF magic.
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
    /// `None` when no descriptor serves that header, which is normal: several program headers
    /// describe regions inside another header's data.
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

    /// Builds a wrapper around a minimal inner ELF, generated from the documented structure
    /// (D051).
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
        v[7] = 9; // ELFOSABI_FREEBSD
        v
    }

    /// The ELF offset is computed from the descriptor count.
    #[test]
    fn the_offset_is_derived_from_the_segment_count_not_hardcoded() {
        for count in [0_u16, 1, 7, 12, 40] {
            let bytes = wrapped(count, &minimal_elf());
            let w = Wrapper::parse(&bytes).expect("parses");
            assert_eq!(
                w.elf_offset(),
                HEADER_SIZE + count as usize * SEGMENT_SIZE,
                "offset must follow the count"
            );
        }
        // Twelve descriptors, the common case, lands at 416.
        let w = Wrapper::parse(&wrapped(12, &minimal_elf())).expect("parses");
        assert_eq!(w.elf_offset(), 416);
    }

    /// A derived offset that does not land on an ELF fails loudly.
    #[test]
    fn a_derivation_that_does_not_land_on_an_elf_fails_loudly() {
        let mut bytes = wrapped(12, &minimal_elf());
        bytes[0x18] = 11; // claim eleven descriptors, so the offset lands 32 bytes early
        let err = Wrapper::parse(&bytes).expect_err("must not guess");
        assert!(
            matches!(err, ElfError::InnerElfNotFound { expected_at, segment_count }
                if expected_at == HEADER_SIZE + 11 * SEGMENT_SIZE && segment_count == 11),
            "got {err:?}"
        );
    }

    /// Both generations parse, and are reported apart.
    #[test]
    fn both_generations_parse_and_are_told_apart() {
        // Read with the current layout, a previous-generation header yields the same
        // fields, a plausible segment count and descriptors that parse (D049).
        let mut current = [0_u8; HEADER_SIZE];
        current[..4].copy_from_slice(&WRAPPER_MAGIC);
        let mut previous = [0_u8; HEADER_SIZE];
        previous[..4].copy_from_slice(&PREVIOUS_GENERATION_MAGIC);

        assert_eq!(Wrapper::generation(&current), Some(Generation::Current));
        assert_eq!(Wrapper::generation(&previous), Some(Generation::Previous));
        assert_eq!(Wrapper::generation(b"not a container"), None);
        assert!(Wrapper::is_either_generation(&previous));
    }

    /// A plain ELF is reported as unwrapped, not corrupt.
    #[test]
    fn a_plain_elf_is_reported_as_unwrapped_not_as_corrupt() {
        assert!(matches!(
            Wrapper::parse(&minimal_elf()),
            Err(ElfError::NotWrapped)
        ));
    }

    /// An absurd segment count is rejected before any arithmetic uses it.
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

    /// Truncation after the header is caught.
    #[test]
    fn truncation_after_the_header_is_caught() {
        let bytes = wrapped(12, &minimal_elf());
        let cut = &bytes[..HEADER_SIZE + 5 * SEGMENT_SIZE];
        assert!(matches!(
            Wrapper::parse(cut),
            Err(ElfError::Truncated { .. })
        ));
    }

    /// A header shorter than the struct is caught.
    #[test]
    fn a_header_shorter_than_the_struct_is_caught() {
        assert!(matches!(
            Wrapper::parse(&WRAPPER_MAGIC),
            Err(ElfError::Truncated { .. })
        ));
    }

    /// The segment table parses to the stated count.
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

    /// The stated size is read and nothing is inferred from it.
    #[test]
    fn the_stated_size_is_read_but_nothing_is_inferred_from_it() {
        // It is smaller than the file by a variable amount, so no check uses it.
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

    /// Descriptor flags decode to a program-header index and a data bit.
    #[test]
    fn segment_flags_decode_to_a_program_header_index_and_a_data_bit() {
        // A data-bearing descriptor names its program header in the top bits, and its
        // stored size equals that header's filesz.
        let bytes = wrapped_with_flags(&[(0x0000_2804, 0x40), (0x0011_0004, 0x20)]);
        let w = Wrapper::parse(&bytes).expect("parses");
        let segs = w.segments(&bytes).expect("segments");

        assert!(
            segs[0].has_data(),
            "0x800 marks the data-bearing descriptor"
        );
        assert_eq!(segs[0].program_header_index(), 0);
        // The paired metadata block carries no program data, and its index bits hold the
        // wrapper-table index of the descriptor it accompanies.
        assert!(
            !segs[1].has_data(),
            "the paired block carries no program data"
        );
    }

    /// Program-header data is located through the descriptor table.
    #[test]
    fn program_header_data_is_located_through_the_descriptor_table() {
        // The inner ELF's p_offset values point past end-of-file.
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

    /// A descriptor pointing past the end of the file is caught.
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

    /// Header fields read back as written.
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
