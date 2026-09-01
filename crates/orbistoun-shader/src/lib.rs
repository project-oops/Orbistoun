//! Shader bytecode: decode it, and measure how much of it is understood.
//!
//! # Why this exists before any translator does
//!
//! Translating shaders is the hardest remaining problem in the project and it is not
//! a search problem - no amount of iterating on failures writes a compiler. It has to
//! be built deliberately, against the published instruction set.
//!
//! But *deciding what to build next* is a counting problem, and counting can start
//! immediately. This crate answers, for a real corpus of real shaders:
//!
//! - How many distinct instructions are in use?
//! - Which ones do we not understand?
//! - **Which single instruction, if supported, would unblock the most shaders?**
//!
//! That last question is the whole point. It turns an unbounded compiler project into
//! a ranked worklist, the same way the import survey turned "emulate the operating
//! system" into a frequency-ordered list of functions. Same pattern, applied one layer
//! down.
//!
//! # It needs no GPU, no driver, and no running emulator
//!
//! Everything here is a pure transformation of bytes. A shader captured once can be
//! analysed on CI, in a VM, or years later. That matters because the corpus outlives
//! any particular version of the translator: change the translator, re-run the corpus,
//! diff the result.
//!
//! # What it deliberately does not do
//!
//! It does not know what any instruction *means*. It reads the encoding - family,
//! opcode, length - and stops there. Semantics are the translator's job, and mixing
//! the two would make this unable to report on instructions it cannot yet translate,
//! which is precisely the set worth reporting on.
//!
//! # Provenance
//!
//! The encoding table is transcribed from AMD's publicly published instruction set
//! documentation for these GPUs. That is hardware documentation from the chip vendor,
//! not console firmware, and it sits in the same category as FreeBSD source in the
//! oracle list: lawful, citable, and by some distance the best reference available
//! anywhere in this project. See `ACKNOWLEDGEMENTS.md` and `data/encodings.toml`.

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
/// Note what is *not* here: decoding never fails. A shader that cannot be decoded is
/// a finding reported through [`Decode`], not an error - a corpus sweep must tell you
/// how many strange binaries there are, not stop at the first one.
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
    /// The built-in operand table. Every decode needs one now that operands are read.
    fn operands() -> crate::operand::OperandTable {
        crate::operand::OperandTable::builtin().expect("built-in operand table")
    }

    use super::{CorpusCoverage, EncodingTable, decode};

    #[test]
    fn the_whole_pipeline_runs_end_to_end() {
        // Decode, observe, rank - the shape every caller uses. Worth one test that
        // exercises the seams together, since each module tests its own logic in
        // isolation and the joins are where an interface drifts.
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
        // Every blocker must render against the table without panicking - these
        // strings go straight into a report.
        for blocker in &ranked {
            assert!(!blocker.key.describe(&table).is_empty());
        }
    }
}
