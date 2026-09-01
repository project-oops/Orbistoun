//! The reference assembler's output formats, as parsers.
//!
//! Each one matches something `llvm-mc` or `llvm-objdump` prints. Where a pattern reads
//! oddly, the reason is that the reference prints oddly - so before simplifying one, check
//! what it is matching against rather than what it looks like it should match.
//!
//! `./bin/orbistoun tables` is the guard on that: it regenerates from a recording and diffs
//! against the committed tables, so a parser that quietly changed what it accepts fails.

use std::sync::OnceLock;

use regex::Regex;

use crate::assembler::Sample;

/// Compiles once, on first use.
///
/// A `Regex` is expensive to build and free to reuse, and the operand solver evaluates
/// these inside a loop over every probe in the corpus.
macro_rules! pattern {
    ($name:ident, $re:literal) => {
        fn $name() -> &'static Regex {
            static CELL: OnceLock<Regex> = OnceLock::new();
            CELL.get_or_init(|| Regex::new($re).expect(concat!("static pattern: ", $re)))
        }
    };
}

// `        v_mov_b32_e32 v0, v1     ; encoding: [0x01,0x03,0x00,0x7e]`
pattern!(
    assembled_re,
    r"^\s*(?P<mnemonic>[a-z_0-9]+)(?P<operands>[^;]*);\s*encoding:\s*\[(?P<bytes>[^\]]+)\]"
);

// `<stdin>:12:1: error: invalid instruction`
pattern!(
    rejection_re,
    r"^(?P<file>[^:]+):(?P<line>\d+):\d+:\s*error:\s*(?P<why>.+)$"
);

// `format:[BUF_FMT_32_UINT]`
pattern!(buffer_format_re, r"format:\[(?P<name>BUF_FMT_[A-Z0-9_]+)\]");

// `<stdin>:12:1: warning: invalid instruction encoding`
pattern!(
    invalid_re,
    r"^<stdin>:(\d+):\d+:\s*warning:\s*invalid instruction"
);

// `	mnemonic operands  // offset: word [word]`, from `llvm-objdump -d`.
//
// **Not anchored to end of line.** A branch prints a trailing symbol reference such as
// `<control+0x2c>`, and anchoring seemed tidier while silently dropping every branch
// instruction - which the contiguity check then caught as a gap, because a fixture missing
// its control flow teaches the decoder that what follows starts four bytes early.
pattern!(
    objdump_re,
    r"^\s*(?P<mnemonic>[a-z_0-9]+)(?P<operands>[^/]*)//\s*(?P<offset>[0-9A-Fa-f]+):\s*(?P<words>[0-9A-Fa-f]{8}(?:\s+[0-9A-Fa-f]{8})*)"
);

// `target triple = "amdgcn-mesa-mesa3d"`, or `// target triple: amdgcn-mesa-mesa3d`.
pattern!(
    triple_re,
    r#"(?m)^\s*(?://\s*target\s+triple\s*:\s*(?P<comment>\S+)|target\s+triple\s*=\s*"(?P<triple>[^"]+)")"#
);

// `offset:16` - a named immediate the reference appends to the *last* operand with no
// comma before it. Splitting on commas alone leaves `v2 offset:16` as one operand, which
// matches no register pattern - so the opcode reports as unsolvable and the offset field,
// which is real and which a translator must read, is never looked for.
pattern!(
    named_immediate_re,
    r"^(?P<name>[a-z_]+[0-9]*):(?P<value>-?(?:0x[0-9a-fA-F]+|\d+))$"
);

// `format:[BUF_FMT_32_FLOAT]` - a modifier whose value is printed as a symbolic name.
pattern!(
    symbolic_modifier_re,
    r"^(?P<name>[a-z_]+[0-9]*):\[[A-Za-z0-9_]+\]$"
);

// `attr3.y` - an attribute number and a channel within it, printed as one token.
pattern!(attribute_re, r"^attr(?P<number>\d+)\.(?P<channel>[xyzw])$");

// Register operands, including the pair and quad forms.
pattern!(
    register_re,
    r"^(?P<kind>[vs])(?:(?P<single>\d+)|\[(?P<first>\d+):\d+\])$"
);

// A bare number, decimal or hex.
pattern!(
    immediate_re,
    r"^(?:0x(?P<hex>[0-9a-fA-F]+)|(?P<dec>-?\d+))$"
);

// `; encoding: [0x01,0x03,0x00,0x7e]`, when the mnemonic is not wanted.
pattern!(encoding_re, r"encoding:\s*\[(?P<bytes>[^\]]+)\]");

/// One assembled listing line, or `None` if the line carries no encoding.
pub(crate) fn assembled(line: &str) -> Option<Sample> {
    let caps = assembled_re().captures(line)?;
    let printed = caps.name("operands")?.as_str().trim().to_owned();
    let mut operands = Vec::new();
    for piece in caps.name("operands")?.as_str().split(',') {
        operands.extend(split_operand(piece));
    }
    Some(Sample {
        mnemonic: caps.name("mnemonic")?.as_str().to_owned(),
        operands,
        words: words_of(caps.name("bytes")?.as_str()),
        printed,
    })
}

/// The one-based line number of an `invalid instruction encoding` warning.
///
/// A *warning*, not an error - the disassembler reports an unrecognised word that way, and
/// treating it as an error would miss it entirely.
pub(crate) fn invalid_instruction(line: &str) -> Option<usize> {
    invalid_re()
        .captures(line)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse().ok())
}

/// One diagnostic line, as `(one-based line number, reason)`.
pub(crate) fn rejection(line: &str) -> Option<(usize, String)> {
    let caps = rejection_re().captures(line)?;
    Some((
        caps.name("line")?.as_str().parse().ok()?,
        caps.name("why")?.as_str().to_owned(),
    ))
}

/// One `llvm-objdump` listing line, as an instruction.
pub(crate) fn objdump_line(line: &str) -> Option<crate::fixtures::Instruction> {
    let caps = objdump_re().captures(line)?;
    let words: Vec<u32> = caps
        .name("words")?
        .as_str()
        .split_whitespace()
        .filter_map(|w| u32::from_str_radix(w, 16).ok())
        .collect();
    if words.is_empty() {
        return None;
    }
    Some(crate::fixtures::Instruction {
        offset: u64::from_str_radix(caps.name("offset")?.as_str(), 16).ok()?,
        words,
        mnemonic: caps.name("mnemonic")?.as_str().to_owned(),
        operands: caps.name("operands")?.as_str().trim().to_owned(),
    })
}

/// The target triple a shader source declares, if it declares one.
pub(crate) fn declared_triple(text: &str) -> Option<String> {
    let caps = triple_re().captures(text)?;
    caps.name("triple")
        .or_else(|| caps.name("comment"))
        .map(|m| m.as_str().to_owned())
}

/// The value of a `name:value` modifier, if the token is one.
pub(crate) fn named_immediate(token: &str) -> Option<String> {
    named_immediate_re()
        .captures(token)
        .and_then(|c| c.name("value").map(|m| m.as_str().to_owned()))
}

/// Whether a token is a modifier whose value is printed as a symbolic name.
pub(crate) fn symbolic_modifier(token: &str) -> bool {
    symbolic_modifier_re().is_match(token)
}

/// `attr3.y` as its number and its channel index.
///
/// `xyzw` = 0 to 3 is the only ordering they could have, and the solver *checks* it: give it
/// the wrong one and no field explains the samples, so it refuses.
pub(crate) fn attribute(token: &str) -> Option<(String, u32)> {
    let caps = attribute_re().captures(token)?;
    let channel = match caps.name("channel")?.as_str() {
        "x" => 0,
        "y" => 1,
        "z" => 2,
        "w" => 3,
        _ => return None,
    };
    Some((caps.name("number")?.as_str().to_owned(), channel))
}

/// A register operand, as `(is_vector, base number)`.
///
/// A multi-register operand is reduced to its base: the field encodes where the group
/// starts, and how far it extends is a property of the instruction.
pub(crate) fn register(token: &str) -> Option<(bool, u32)> {
    let caps = register_re().captures(token)?;
    let number = caps
        .name("single")
        .or_else(|| caps.name("first"))?
        .as_str()
        .parse()
        .ok()?;
    Some((caps.name("kind")?.as_str() == "v", number))
}

/// A bare number, decimal or hexadecimal.
pub(crate) fn immediate(token: &str) -> Option<i64> {
    let caps = immediate_re().captures(token)?;
    if let Some(hex) = caps.name("hex") {
        return i64::from_str_radix(hex.as_str(), 16).ok();
    }
    caps.name("dec")?.as_str().parse().ok()
}

/// The typed-buffer format name a listing line printed, if it printed one.
pub(crate) fn buffer_format(line: &str) -> Option<&str> {
    buffer_format_re()
        .captures(line)
        .and_then(|c| c.name("name").map(|m| m.as_str()))
}

/// The encoding words on a listing line, ignoring everything else about it.
pub(crate) fn encoding(line: &str) -> Option<Vec<u32>> {
    encoding_re()
        .captures(line)
        .and_then(|c| c.name("bytes"))
        .map(|m| words_of(m.as_str()))
}

/// `0x01,0x03,0x00,0x7e` as little-endian 32-bit words.
///
/// A trailing partial word is dropped rather than zero-extended. An instruction is a whole
/// number of words, so a partial one means the line was not what it looked like - and
/// padding it would invent bits the assembler never emitted.
fn words_of(bytes: &str) -> Vec<u32> {
    let octets: Vec<u8> = bytes
        .split(',')
        .map(str::trim)
        .filter(|b| !b.is_empty())
        .filter_map(|b| u8::from_str_radix(b.trim_start_matches("0x"), 16).ok())
        .collect();
    octets
        .chunks_exact(4)
        .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
        .collect()
}

/// Splits one printed operand into the tokens a solver treats separately.
///
/// The reference prints modifiers alongside operands and separated by spaces rather than
/// commas, so a comma split alone leaves `v0 offset:4 glc` as one token. Splitting on
/// whitespace recovers them.
#[must_use]
pub(crate) fn split_operand(piece: &str) -> Vec<String> {
    piece
        .split_whitespace()
        .map(str::to_owned)
        .filter(|t| !t.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{assembled, buffer_format, encoding, rejection, split_operand, words_of};

    #[test]
    fn an_assembled_line_yields_mnemonic_operands_and_words() {
        let s = assembled("\tv_mov_b32_e32 v0, v1   ; encoding: [0x01,0x03,0x00,0x7e]")
            .expect("parses");
        assert_eq!(s.mnemonic, "v_mov_b32_e32");
        assert_eq!(s.operands, vec!["v0", "v1"]);
        assert_eq!(s.words, vec![0x7e00_0301]);
        assert_eq!(s.printed, "v0, v1");
    }

    /// Modifiers are separate tokens even though the reference prints them without commas.
    #[test]
    fn modifiers_printed_without_commas_are_split_out() {
        assert_eq!(
            split_operand(" v0 offset:4 glc "),
            ["v0", "offset:4", "glc"]
        );
    }

    #[test]
    fn a_diagnostic_yields_its_line_and_reason() {
        let (line, why) = rejection("<stdin>:12:1: error: invalid instruction").expect("parses");
        assert_eq!(line, 12);
        assert_eq!(why, "invalid instruction");
    }

    /// A warning is not a rejection. The assembler emits both, and treating a warning as a
    /// refusal would drop a probe that assembled perfectly well.
    #[test]
    fn a_warning_is_not_a_rejection() {
        assert!(rejection("<stdin>:3:1: warning: invalid instruction").is_none());
    }

    #[test]
    fn a_printed_buffer_format_is_recovered() {
        let line = "\ttbuffer_load_format_x v0, v1, s[8:11], 0 format:[BUF_FMT_32_UINT] idxen";
        assert_eq!(buffer_format(line), Some("BUF_FMT_32_UINT"));
        // A code with no name prints back numerically, which is a different fact.
        assert_eq!(buffer_format("... format:78 idxen"), None);
    }

    /// A partial trailing word is dropped, never padded.
    #[test]
    fn a_partial_word_is_dropped_rather_than_padded() {
        assert_eq!(words_of("0x01,0x02,0x03"), Vec::<u32>::new());
        assert_eq!(words_of("0x01,0x02,0x03,0x04,0x05"), vec![0x0403_0201]);
    }

    #[test]
    fn an_encoding_is_readable_without_the_mnemonic() {
        assert_eq!(
            encoding("anything at all ; encoding: [0x01,0x00,0x00,0x00]"),
            Some(vec![1])
        );
    }
}
