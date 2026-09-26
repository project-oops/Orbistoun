//! Shader bytecode: decode it, and measure how much of it is understood.
//!
//! For a corpus of shaders this crate counts the distinct instructions in use, the ones
//! not understood, and which single instruction, if supported, unblocks the most shaders -
//! a ranked worklist for the translator. Everything is a pure transformation of bytes and
//! needs no GPU or driver. It reads the encoding (family, opcode, length) and not the
//! semantics, so it can report on instructions the translator cannot handle.
//!
//! The encoding table is transcribed from the GPU vendor's published instruction set
//! documentation; see `ACKNOWLEDGEMENTS.md` and `data/encodings.toml`.

pub mod corpus;
pub mod coverage;
pub mod decode;
pub mod encoding;
pub mod formats;
pub mod mnemonics;
pub mod operand;
pub mod report;

pub use corpus::{Capture, ShaderCorpus, shader_id};
pub use coverage::{Blocker, CorpusCoverage, OpcodeKey, ShaderSummary};
pub use decode::{Decode, Instruction, decode, decode_program};
pub use encoding::{Encoding, EncodingTable};
pub use formats::{BufferFormat, ComponentKind, FormatTable};
pub use mnemonics::MnemonicTable;
pub use operand::{Operand, OperandTable};

/// Why a shader operation failed.
///
/// Decoding never fails: an undecodable shader is a finding reported through [`Decode`],
/// so a corpus sweep counts every strange binary instead of stopping at the first.
#[derive(Debug, thiserror::Error)]
pub enum ShaderError {
    /// The encoding table could not be loaded or is self-inconsistent.
    #[error("encoding table: {0}")]
    Table(String),
    /// The shader corpus could not be read or written.
    #[error("shader corpus: {0}")]
    Corpus(String),
}

#[cfg(test)]
mod tests {
    /// The built-in operand table, which every decode needs.
    fn operands() -> crate::operand::OperandTable {
        crate::operand::OperandTable::builtin().expect("built-in operand table")
    }

    use super::{CorpusCoverage, EncodingTable, decode};

    /// Decode, observe and rank work together across the module seams.
    #[test]
    fn the_whole_pipeline_runs_end_to_end() {
        let table = EncodingTable::builtin().expect("builtin table");
        let words: Vec<u32> = vec![0x7E00_0000, 0xBF80_0000, 0xFFFF_FFF0];
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();

        let decoded = decode(&bytes, &table, &operands());
        assert!(!decoded.instructions.is_empty());

        let mut coverage = CorpusCoverage::new();
        coverage.observe("generated", &decoded, &|_| false);

        let ranked = coverage.ranked_blockers(crate::coverage::all_ordinary);
        assert!(
            !ranked.is_empty(),
            "nothing is supported, so all are blockers"
        );
        // Every blocker renders against the table; the strings go into a report.
        for blocker in &ranked {
            assert!(!blocker.key.describe(&table).is_empty());
        }
    }
}
