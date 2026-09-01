//! Relocations: the entries that turn a linked image into a placed one.
//!
//! Standard `Elf64_Rela` throughout - the vendor adds nothing here. Two tables carry
//! them, and the split matters:
//!
//! - **`DT_RELA`** holds data relocations: absolute addresses baked into the image
//!   that must be adjusted for wherever it actually landed.
//! - **`DT_JMPREL`** holds the procedure linkage table: one slot per imported
//!   function. **These are where imports become calls** - writing a host address into
//!   a slot is what D005 means by "interception is linking".
//!
//! # Only four types matter to get started
//!
//! An image placed at a base needs `RELATIVE` (adjust an internal pointer), `64` and
//! `GLOB_DAT` (write a symbol address), and `JUMP_SLOT` (write a function address).
//! TLS relocations need thread-local storage to exist first and are deliberately not
//! handled - reported as unsupported rather than silently skipped, since a skipped
//! relocation leaves a pointer that looks valid and is not.

use zerocopy::{FromBytes, Immutable, KnownLayout, little_endian};

/// Size of one `Elf64_Rela`.
pub const RELA_SIZE: usize = 24;

/// Relocation types this loader understands.
pub mod kind {
    /// Write `symbol + addend`.
    pub const ABS64: u32 = 1;
    /// Write `symbol` - a data symbol's address.
    pub const GLOB_DAT: u32 = 6;
    /// Write `symbol` - a function address, in a PLT slot.
    pub const JUMP_SLOT: u32 = 7;
    /// Write `base + addend` - an internal pointer adjusted for placement.
    pub const RELATIVE: u32 = 8;
    /// TLS module id. Needs thread-local storage to exist.
    pub const DTPMOD64: u32 = 16;
    /// TLS offset within a module. Needs thread-local storage to exist.
    pub const DTPOFF64: u32 = 17;
    /// TLS offset from the thread pointer. Needs thread-local storage to exist.
    pub const TPOFF64: u32 = 18;
}

/// One relocation entry, exactly as it appears on disk.
#[derive(Debug, Clone, Copy, FromBytes, Immutable, KnownLayout)]
#[repr(C)]
pub struct Elf64Rela {
    /// Where to write, as a virtual address before placement.
    pub offset: little_endian::U64,
    /// Packed symbol index and relocation type.
    pub info: little_endian::U64,
    /// Constant added to the computed value.
    pub addend: little_endian::I64,
}

impl Elf64Rela {
    /// The relocation type.
    pub fn kind(&self) -> u32 {
        (self.info.get() & 0xFFFF_FFFF) as u32
    }

    /// Index into the dynamic symbol table, or zero when the type needs no symbol.
    pub fn symbol_index(&self) -> u32 {
        (self.info.get() >> 32) as u32
    }

    /// Whether this type is one the loader can apply today.
    pub fn is_supported(&self) -> bool {
        matches!(
            self.kind(),
            kind::ABS64 | kind::GLOB_DAT | kind::JUMP_SLOT | kind::RELATIVE
        )
    }

    /// Whether this type needs thread-local storage to exist first.
    pub fn is_tls(&self) -> bool {
        matches!(self.kind(), kind::DTPMOD64 | kind::DTPOFF64 | kind::TPOFF64)
    }
}

/// Parses a relocation table.
///
/// A trailing partial entry is ignored rather than treated as an error: the table
/// length comes from the dynamic section and a rounding disagreement should not make an
/// otherwise-loadable image unloadable.
pub fn parse_table(bytes: &[u8]) -> Vec<Elf64Rela> {
    bytes
        .chunks_exact(RELA_SIZE)
        .filter_map(|c| Elf64Rela::read_from_prefix(c).ok().map(|(r, _)| r))
        .collect()
}

/// A tally of what a relocation table contains.
///
/// Reported rather than silently acted on: knowing *how many* relocations were skipped
/// and why is the difference between "this image is not ready" and "this image loaded
/// and then behaved strangely".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RelocationTally {
    /// Entries applied.
    pub applied: usize,
    /// Entries needing thread-local storage, which does not exist yet.
    pub tls_deferred: usize,
    /// Entries of a type this loader does not implement.
    pub unsupported: usize,
    /// Entries whose symbol could not be resolved.
    pub unresolved: usize,
}

impl RelocationTally {
    /// Total entries seen.
    pub const fn total(&self) -> usize {
        self.applied + self.tls_deferred + self.unsupported + self.unresolved
    }

    /// Whether every entry was applied.
    pub const fn complete(&self) -> bool {
        self.tls_deferred == 0 && self.unsupported == 0 && self.unresolved == 0
    }
}

#[cfg(test)]
mod tests {
    use super::{RELA_SIZE, RelocationTally, kind, parse_table};

    /// Builds a relocation entry. **Generated, never extracted** (D051).
    fn rela(offset: u64, sym: u32, kind: u32, addend: i64) -> Vec<u8> {
        let mut v = Vec::with_capacity(RELA_SIZE);
        v.extend_from_slice(&offset.to_le_bytes());
        v.extend_from_slice(&((u64::from(sym) << 32) | u64::from(kind)).to_le_bytes());
        v.extend_from_slice(&addend.to_le_bytes());
        v
    }

    #[test]
    fn info_splits_into_a_symbol_index_and_a_type() {
        // Getting this backwards produces relocations that look plausible and write
        // nonsense, so it is worth asserting directly.
        let table = parse_table(&rela(0x1000, 42, kind::JUMP_SLOT, 0));
        assert_eq!(table.len(), 1);
        assert_eq!(table[0].symbol_index(), 42);
        assert_eq!(table[0].kind(), kind::JUMP_SLOT);
        assert_eq!(table[0].offset.get(), 0x1000);
    }

    #[test]
    fn a_negative_addend_survives_the_round_trip() {
        // Addends are signed. Reading one as unsigned turns a small backwards offset
        // into an enormous forwards one.
        let table = parse_table(&rela(0, 0, kind::RELATIVE, -8));
        assert_eq!(table[0].addend.get(), -8);
    }

    #[test]
    fn supported_and_tls_types_are_distinguished() {
        for k in [kind::ABS64, kind::GLOB_DAT, kind::JUMP_SLOT, kind::RELATIVE] {
            let t = parse_table(&rela(0, 0, k, 0));
            assert!(t[0].is_supported(), "{k} should be supported");
            assert!(!t[0].is_tls());
        }
        for k in [kind::DTPMOD64, kind::DTPOFF64, kind::TPOFF64] {
            let t = parse_table(&rela(0, 0, k, 0));
            assert!(t[0].is_tls(), "{k} should be TLS");
            assert!(!t[0].is_supported(), "TLS is not applicable yet");
        }
    }

    #[test]
    fn several_entries_parse_in_order() {
        let mut bytes = Vec::new();
        for i in 0..4_u64 {
            bytes.extend(rela(i * 8, 0, kind::RELATIVE, i as i64));
        }
        let table = parse_table(&bytes);
        assert_eq!(table.len(), 4);
        assert_eq!(table[3].offset.get(), 24);
    }

    #[test]
    fn a_trailing_partial_entry_is_ignored_rather_than_failing() {
        // The table length comes from the dynamic section; a rounding disagreement
        // should not make an otherwise-loadable image unloadable.
        let mut bytes = rela(0x1000, 1, kind::RELATIVE, 0);
        bytes.extend_from_slice(&[0xAB; 7]);
        assert_eq!(parse_table(&bytes).len(), 1);
    }

    #[test]
    fn an_empty_table_parses_to_nothing() {
        assert!(parse_table(&[]).is_empty());
    }

    #[test]
    fn a_tally_reports_completeness_honestly() {
        let complete = RelocationTally {
            applied: 10,
            ..RelocationTally::default()
        };
        assert!(complete.complete());
        assert_eq!(complete.total(), 10);

        // One deferred entry means the image is not ready, even though nothing failed.
        // A pointer left unrelocated looks valid and is not.
        let partial = RelocationTally {
            applied: 10,
            tls_deferred: 1,
            ..RelocationTally::default()
        };
        assert!(!partial.complete());
        assert_eq!(partial.total(), 11);
    }
}
