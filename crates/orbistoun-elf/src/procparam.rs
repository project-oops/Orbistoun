//! The process parameter block, which a console loader reads before the first guest
//! instruction runs.
//!
//! A launching executable carries a `PT_SCE_PROCPARAM` segment ([`crate::SCE_PROCPARAM`])
//! whose bytes are a small fixed header followed by pointers to three further blocks the
//! title chose - libc parameters, kernel-memory parameters, and one more. The loader reads
//! this to learn the SDK version and, through the memory-parameter block, the flexible
//! memory budget the title asked for. A field it expects and does not find faults inside the
//! platform library before a single guest instruction runs, so the layout is not a place to
//! guess.
//!
//! # Provenance
//!
//! The layout below is taken from obSCEne's `crt.c`, which builds this structure to launch
//! on real hardware and cites the OpenOrbis PS4 ELF specification for the magic, the entry
//! count, and the fixed size. Two of its offsets are hardware-confirmed rather than merely
//! documented: obSCEne's D219 records a console faulting on a write through a null pointer at
//! this block's `+0x40` slot, which fixes the memory-parameter pointer at exactly that
//! offset. See `docs/REFERENCES.md`.
//!
//! # What this reads and what it deliberately does not
//!
//! The fixed header and the three pointers are read here, because every offset is cited. The
//! *contents* of the blocks the pointers lead to are not parsed: obSCEne supplies those blocks
//! sized-but-empty, so its build establishes that the memory-parameter block exists and how
//! large it is, but not where the flexible-memory field sits inside it. Reading that field
//! from a real title would be deriving a layout from material rather than confirming one from
//! a source, which the shared provenance rule forbids. So the pointer is followed no further
//! than reporting where it leads (D442).

/// The magic a loader looks for at `+0x08`, `"ORBI"` little-endian.
///
/// Named in the OpenOrbis PS4 ELF specification; a loader that does not find it does not
/// trust the rest of the block.
pub const MAGIC: u32 = 0x4942_524F;

/// Offset of the memory-parameter pointer within the block.
///
/// Hardware-confirmed, not merely documented: obSCEne left this slot null and a console
/// faulted writing through it, which is what pins the pointer to this offset rather than an
/// adjacent one (obSCEne D219).
pub const MEM_PARAM_OFFSET: usize = 0x40;

/// The fixed header and the three pointers a launching title's process parameters carry.
///
/// Only fields with a cited offset are represented. The blocks the pointers lead to are left
/// as addresses for a caller to resolve - see the module note on why their contents are not
/// parsed here.
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
    /// Returns `None` only when the bytes are too short to hold even the fixed header
    /// (`0x18`). A block long enough for the header but not for a pointer reports that
    /// pointer as zero rather than failing, because "the loader would read zero here" is the
    /// honest thing to say about a short block, and an absent pointer is exactly what a
    /// module with no process parameters of its own carries.
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
        // The header runs to +0x18 (two SDK-version halves ending there). Shorter than that
        // is not a process-parameter block at all.
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

    /// A block built exactly as obSCEne's `crt.c` lays it out, so the offsets under test are
    /// the ones a hardware-confirmed builder uses.
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

    #[test]
    fn reads_the_header_and_the_three_pointers_at_their_cited_offsets() {
        // The property that matters: the mem_param pointer is read from +0x40, the offset a
        // console fault pinned (obSCEne D219). A regression that shifted it by one field would
        // still parse and still look plausible, so it is asserted by value.
        let p = ProcParam::parse(&crt_block()).expect("a full block");
        assert_eq!(p.size, 0x60);
        assert!(p.magic_ok());
        assert_eq!(p.entry_count, 5);
        assert_eq!(p.libc_param, 0x1000);
        assert_eq!(p.mem_param, 0x2000);
        assert_eq!(p.third_param, 0x3000);
    }

    #[test]
    fn a_block_without_the_magic_is_read_but_reports_it() {
        // The bytes are still parsed - a caller may want the size - but magic_ok is the gate
        // that says whether a loader would believe them.
        let mut b = crt_block();
        b[0x08..0x0c].copy_from_slice(&0_u32.to_le_bytes());
        let p = ProcParam::parse(&b).expect("still long enough");
        assert!(!p.magic_ok());
    }

    #[test]
    fn a_header_only_block_reports_absent_pointers_as_zero() {
        // A short block is not a parse failure: zero is what a loader would read past its end,
        // and a module carrying no parameters of its own is the ordinary reason for it.
        let mut b = crt_block();
        b.truncate(0x20);
        let p = ProcParam::parse(&b).expect("header present");
        assert!(p.magic_ok());
        assert_eq!(p.mem_param, 0, "no pointer in a header-only block");
        assert_eq!(p.libc_param, 0);
    }

    #[test]
    fn bytes_too_short_for_a_header_are_not_a_block() {
        assert!(ProcParam::parse(&[0_u8; 0x10]).is_none());
    }
}
