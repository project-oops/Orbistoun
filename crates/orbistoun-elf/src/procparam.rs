//! The process parameter block, which a hardware loader reads before the first guest
//! instruction runs.
//!
//! A launching executable carries a `PT_SCE_PROCPARAM` segment whose bytes are a fixed
//! header followed by pointers to three blocks: libc parameters, kernel-memory parameters,
//! and one more. The loader reads it for the SDK version and the flexible memory budget. The
//! layout comes from obSCEne's `crt.c` and the open-toolchain ELF specification it cites; the
//! `+0x40` memory-parameter offset is confirmed on hardware (`docs/REFERENCES.md`). Only the
//! fixed header and the three pointers are read: the layout inside the pointed-to blocks has
//! no cited source, so the pointers are reported and not followed.

/// The magic a loader looks for at `+0x08`, `"ORBI"` little-endian.
///
/// Named in the open-toolchain ELF specification; a loader that does not find it does not
/// trust the rest of the block.
pub const MAGIC: u32 = 0x4942_524F;

/// Offset of the memory-parameter pointer within the block.
///
/// Confirmed on hardware: a block with this slot null faults on a write through it at this
/// offset (`docs/REFERENCES.md`).
pub const MEM_PARAM_OFFSET: usize = 0x40;

/// The fixed header and the three pointers a launching title's process parameters carry.
///
/// Only fields with a cited offset are represented. The pointed-to blocks are left as
/// addresses for a caller to resolve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcParam {
    /// The size the block states, its one mandatory field.
    pub size: u64,
    /// The magic at `+0x08`. Compare with [`MAGIC`] via [`Self::magic_ok`].
    pub magic: u32,
    /// How many entries follow the fixed header. A real launching title states five.
    pub entry_count: u32,
    /// The SDK version at `+0x10`.
    pub sdk_version: u32,
    /// The second SDK version field at `+0x14`.
    pub sdk_version_second: u32,
    /// The libc-parameter pointer at `+0x38`, as a guest virtual address. Zero when the block
    /// is too short to carry it.
    pub libc_param: u64,
    /// The memory-parameter pointer at [`MEM_PARAM_OFFSET`], as a guest virtual address. Zero
    /// when absent.
    pub mem_param: u64,
    /// The third pointer at `+0x48`, as a guest virtual address. Zero when absent.
    pub third_param: u64,
}

impl ProcParam {
    /// Reads the block from a `PT_SCE_PROCPARAM` segment's bytes.
    ///
    /// Returns `None` only when the bytes are too short for the fixed header (`0x18`). A
    /// pointer past the end of a shorter block reads as zero, which is what the loader reads
    /// and what a module with no process parameters of its own carries.
    #[must_use]
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let word = |at: usize| -> u64 {
            bytes
                .get(at..at + 8)
                .and_then(|s| s.try_into().ok())
                .map_or(0, u64::from_le_bytes)
        };
        let half = |at: usize| -> u32 {
            bytes
                .get(at..at + 4)
                .and_then(|s| s.try_into().ok())
                .map_or(0, u32::from_le_bytes)
        };
        // The header runs to +0x18, ending with the two SDK-version halves.
        if bytes.len() < 0x18 {
            return None;
        }
        Some(Self {
            size: word(0x00),
            magic: half(0x08),
            entry_count: half(0x0c),
            sdk_version: half(0x10),
            sdk_version_second: half(0x14),
            libc_param: word(0x38),
            mem_param: word(MEM_PARAM_OFFSET),
            third_param: word(0x48),
        })
    }

    /// Whether the magic reads `"ORBI"`. A loader ignores a block that fails this.
    #[must_use]
    pub fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }
}

#[cfg(test)]
mod tests {
    use super::{MAGIC, MEM_PARAM_OFFSET, ProcParam};

    /// A block laid out as obSCEne's `crt.c` builds it.
    fn crt_block() -> Vec<u8> {
        let mut b = vec![0_u8; 0x60];
        b[0x00..0x08].copy_from_slice(&0x60_u64.to_le_bytes());
        b[0x08..0x0c].copy_from_slice(&MAGIC.to_le_bytes());
        b[0x0c..0x10].copy_from_slice(&5_u32.to_le_bytes());
        b[0x10..0x14].copy_from_slice(&0_u32.to_le_bytes());
        b[0x14..0x18].copy_from_slice(&0_u32.to_le_bytes());
        b[0x38..0x40].copy_from_slice(&0x1000_u64.to_le_bytes());
        b[MEM_PARAM_OFFSET..MEM_PARAM_OFFSET + 8].copy_from_slice(&0x2000_u64.to_le_bytes());
        b[0x48..0x50].copy_from_slice(&0x3000_u64.to_le_bytes());
        b
    }

    /// The header and the three pointers are read at their cited offsets.
    #[test]
    fn reads_the_header_and_the_three_pointers_at_their_cited_offsets() {
        // Asserted by value: a pointer shifted by one field would still parse plausibly.
        let p = ProcParam::parse(&crt_block()).expect("a full block");
        assert_eq!(p.size, 0x60);
        assert!(p.magic_ok());
        assert_eq!(p.entry_count, 5);
        assert_eq!(p.libc_param, 0x1000);
        assert_eq!(p.mem_param, 0x2000);
        assert_eq!(p.third_param, 0x3000);
    }

    /// A block without the magic is still read, and `magic_ok` reports it.
    #[test]
    fn a_block_without_the_magic_is_read_but_reports_it() {
        // A caller may still want the size; `magic_ok` says whether a loader believes it.
        let mut b = crt_block();
        b[0x08..0x0c].copy_from_slice(&0_u32.to_le_bytes());
        let p = ProcParam::parse(&b).expect("still long enough");
        assert!(!p.magic_ok());
    }

    /// A header-only block reports absent pointers as zero.
    #[test]
    fn a_header_only_block_reports_absent_pointers_as_zero() {
        let mut b = crt_block();
        b.truncate(0x20);
        let p = ProcParam::parse(&b).expect("header present");
        assert!(p.magic_ok());
        assert_eq!(p.mem_param, 0, "no pointer in a header-only block");
        assert_eq!(p.libc_param, 0);
    }

    /// Bytes too short for the header are not a block.
    #[test]
    fn bytes_too_short_for_a_header_are_not_a_block() {
        assert!(ProcParam::parse(&[0_u8; 0x10]).is_none());
    }
}
