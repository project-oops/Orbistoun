//! Solving per-opcode operand layouts from assembled probes.
//!
//! Writes `crates/orbistoun-shader/data/opcode-operands.toml` (D085).
//!
//! Each entry is the one bit field that explains every probe sample of that opcode. Probes
//! use varied and high register numbers so a coincidence cannot survive and a too-narrow
//! field cannot win. An opcode whose operands cannot be solved unambiguously is absent
//! rather than approximated.
//!
//! Three questions need the assembler (whether an unexplained operand is implicit, whether
//! a probe can reach bits a widening guessed at, and what code a symbolic name like `mrt0`
//! carries), so they go through [`Oracle`], which the tests substitute.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use anyhow::{Context, Result};

use crate::assembler::Sample;
use crate::table::Encoding;

/// Tokens the reference prints among the operands that are not operands at all.
///
/// `off` says there is no scalar base register; the cache hints and an export's `done`,
/// `compr` and `vm` flags say how the access behaves. None is an operand field. The list
/// must match the decoder's copy, so a solved layout compares with a printed one.
///
/// They are still worth setting in a probe: `unorm` sits immediately above the image
/// family's `dmask`, and probes that leave it clear let a five-bit window fit as well as
/// the real four-bit field.
const MODIFIERS: [&str; 14] = [
    "off", "glc", "slc", "dlc", "lds", "gds", "offen", "idxen", "tfe", "nv", "unorm", "done",
    "compr", "vm",
];

/// Field shapes worth trying, for register selectors.
///
/// Register fields in this architecture are six to nine bits, and five for a buffer
/// resource selector, which names a group of four consecutive scalar registers.
const WIDTHS: std::ops::Range<u32> = 5..10;

/// Widths an immediate field can have.
///
/// A different set from register selectors: two and three for selectors (an interpolation
/// names one of four channels in two bits), four for the image family's channel mask below
/// `unorm`, twelve for a flat access's byte offset below its cache hints, and the wider
/// offsets and branch targets. An extra width cannot produce a wrong answer: a second fit
/// makes the operand ambiguous, and the solver refuses.
const IMMEDIATE_WIDTHS: [u32; 9] = [2, 3, 4, 12, 13, 16, 20, 21, 32];

/// How a field's bits are read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Kind {
    /// A direct vector-register index.
    Vgpr,
    /// The shared source numbering: registers, special registers, inline constants.
    Source,
    /// A plain number.
    Immediate,
    /// The encoding does not carry this operand at all.
    Implicit,
}

impl Kind {
    /// The spelling the decoder's table uses.
    #[must_use]
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Vgpr => "vgpr",
            Self::Source => "source",
            Self::Immediate => "immediate",
            Self::Implicit => "implicit",
        }
    }
}

/// One operand slot, solved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Field {
    /// Which word of the instruction it lives in.
    pub(crate) word: usize,
    /// Bit position it starts at.
    pub(crate) shift: u32,
    /// How many bits it occupies. Zero for an implicit operand.
    pub(crate) width: u32,
    /// How its bits are read.
    pub(crate) kind: Kind,
    /// What the field's value is multiplied by. A pair is named by half its base.
    pub(crate) scale: u32,
    /// For an implicit operand, the text the reference always prints.
    pub(crate) implicit: Option<String>,
}

/// Something that can assemble one instruction and say what came back.
///
/// Live, it shells out; in a test it is a table of canned answers, so every path runs with
/// no toolchain installed.
pub(crate) trait Oracle {
    /// Assembles one line. An empty result means it was refused.
    fn assemble_one(&self, text: &str) -> Vec<Sample>;
}

/// Splits one comma-separated piece into the operands it actually contains.
///
/// Usually one. A piece carrying trailing modifiers becomes the operand plus the named
/// immediates after it, in printed order, so a `name:value` field still gets a slot.
#[must_use]
pub(crate) fn split_operand(piece: &str) -> Vec<String> {
    let mut out = Vec::new();
    for token in piece.split_whitespace() {
        if MODIFIERS.contains(&token) || crate::patterns::symbolic_modifier(token) {
            continue;
        }
        if let Some((number, channel)) = crate::patterns::attribute(token) {
            // Two operands, both real fields: which attribute, and which of its four
            // channels. The `xyzw` ordering is checked by the solve, which finds no field
            // if it is wrong.
            out.push(number);
            out.push(channel.to_string());
            continue;
        }
        if let Some(value) = crate::patterns::named_immediate(token) {
            out.push(value);
            continue;
        }
        out.push(token.to_owned());
    }
    out
}

/// One reading of an operand: how the bits are interpreted, the code, and the scale.
type Reading = (Kind, i64, u32);

/// Every reading of an operand.
///
/// A field holds a code, not a register number, and one operand can have several readings:
/// a vector register is a direct index in some fields and sits at 256 upward in the shared
/// numbering. All plausible readings are returned and the samples decide. A multi-register
/// operand is reduced to its base.
#[must_use]
pub(crate) fn expected(
    operand: &str,
    named: &BTreeMap<String, i64>,
    symbolic: &BTreeMap<String, i64>,
) -> Option<Vec<Reading>> {
    if let Some(&code) = symbolic.get(operand) {
        // An export target or an interpolation parameter: a selector with a measured code,
        // so the only reading is the raw number.
        return Some(vec![(Kind::Immediate, code, 1)]);
    }
    if let Some(&code) = named.get(operand) {
        // A special register or an inline float, whose fixed codes reach past the register
        // range.
        return Some(vec![(Kind::Source, code, 1)]);
    }
    if let Some((is_vector, number)) = crate::patterns::register(operand) {
        let number = i64::from(number);
        if is_vector {
            return Some(vec![
                (Kind::Vgpr, number, 1),
                (Kind::Source, number + 256, 1),
            ]);
        }
        // Scale two and four: a pair is named by a field holding half its base register,
        // and a quad (a buffer resource constant) by one holding a quarter.
        return Some(vec![
            (Kind::Source, number, 1),
            (Kind::Source, number, 2),
            (Kind::Source, number, 4),
        ]);
    }
    let value = crate::patterns::immediate(operand)?;
    let mut readings = vec![(Kind::Immediate, value, 1)];
    // A small integer is an inline constant in a source field and the number itself in an
    // offset field; both are offered.
    if (0..=64).contains(&value) {
        readings.push((Kind::Source, 128 + value, 1));
    } else if (-16..=-1).contains(&value) {
        // The negative inline constants sit above the positive ones: -1 through -16 at 193
        // upward.
        readings.push((Kind::Source, 192 + (-value), 1));
    }
    Some(readings)
}

/// Every field shape and reading that explains this operand in every sample.
///
/// `reserved` maps a word index to the bits the encoding table already claims (see
/// [`reserved_bits_for`]); a candidate overlapping them would read a constant or opcode bit as
/// part of a value. Without it, a nine-bit window over `v_cmp_lt_f32_e32`'s eight-bit
/// vector source reads the register plus 256, which is the same register in the shared
/// numbering, and the solve stays ambiguous.
#[must_use]
pub(crate) fn candidates_for(
    samples: &[Sample],
    position: usize,
    named: &BTreeMap<String, i64>,
    symbolic: &BTreeMap<String, i64>,
    reserved: &BTreeMap<usize, u32>,
) -> Vec<Field> {
    // Samples that carry this operand. The reference omits an operand at its default, so a
    // sample without it is not evidence of absence.
    let mut wanted: Vec<(&Sample, Vec<Reading>)> = Vec::new();
    for sample in samples {
        let Some(operand) = sample.operands.get(position) else {
            continue;
        };
        let Some(readings) = expected(operand, named, symbolic) else {
            return Vec::new();
        };
        wanted.push((sample, readings));
    }
    if wanted.is_empty() {
        return Vec::new();
    }

    let Some(word_count) = samples.iter().map(|s| s.words.len()).min() else {
        return Vec::new();
    };
    let mut found = Vec::new();
    let widths: Vec<u32> = WIDTHS.chain(IMMEDIATE_WIDTHS).collect();

    for word_index in 0..word_count {
        for &width in &widths {
            let mask = (1_u64 << width) - 1;
            for shift in 0..=(32 - width) {
                if ((mask << shift) as u32) & reserved.get(&word_index).copied().unwrap_or(0) != 0 {
                    continue;
                }
                for (kind, scale) in [
                    (Kind::Vgpr, 1_u32),
                    (Kind::Source, 1),
                    (Kind::Source, 2),
                    // Four, because a buffer resource constant spans four consecutive scalar
                    // registers and its field holds the group index.
                    (Kind::Source, 4),
                    (Kind::Immediate, 1),
                ] {
                    let fits = wanted.iter().all(|(sample, readings)| {
                        let raw = i64::from((sample.words[word_index] >> shift) & mask as u32);
                        readings.iter().any(|&(k, code, sc)| {
                            k == kind && sc == scale && raw * i64::from(scale) == code
                        })
                    });
                    if fits {
                        found.push(Field {
                            word: word_index,
                            shift,
                            width,
                            kind,
                            scale,
                            implicit: None,
                        });
                    }
                }
            }
        }
    }
    found
}

/// Bits no operand can occupy, per word.
///
/// The opcode field, the family's fixed bits (constant across the family, so a candidate
/// overlapping them fits every sample), and the opcode's continuation where a family splits
/// it. Keyed by word, because the typed-buffer family's fourth opcode bit is at 53, in the
/// second word. Without the fixed bits, a nine-bit window at `v_mov_b32_e32`'s destination
/// reaches the always-set low bit of VOP1's mask and reads `v0` as 256, making the solve
/// ambiguous.
#[must_use]
pub(crate) fn reserved_bits_for(family: &str, encodings: &[Encoding]) -> BTreeMap<usize, u32> {
    let mut out = BTreeMap::new();
    let Some(encoding) = encodings.iter().find(|e| e.name == family) else {
        return out;
    };
    let mut first = encoding.mask;
    if encoding.opcode.width > 0 {
        first |= ((1_u32 << encoding.opcode.width) - 1) << encoding.opcode.shift;
    }
    out.insert(0, first);
    if let Some(extension) = encoding.extension {
        let bits = ((1_u32 << extension.width) - 1) << extension.shift;
        *out.entry(extension.word).or_insert(0) |= bits;
    }
    out
}

/// One operand slot as the decoder's table spells it.
#[must_use]
pub(crate) fn render_field(field: &Field) -> String {
    let mut body = format!(
        "word = {}, shift = {}, width = {}, kind = \"{}\", scale = {}",
        field.word,
        field.shift,
        field.width,
        field.kind.as_str(),
        field.scale
    );
    if let Some(text) = &field.implicit {
        let _ = write!(body, ", implicit = \"{text}\"");
    }
    format!("{{ {body} }}")
}

/// One opcode's solved layout.
#[derive(Debug, Clone)]
pub(crate) struct Solved {
    /// The mnemonic the reference printed.
    pub(crate) mnemonic: String,
    /// One entry per operand position.
    pub(crate) fields: Vec<Field>,
    /// How many samples the answer rests on.
    pub(crate) samples: usize,
}

/// Solves fields for every operand position, or `None` if any is ambiguous.
pub(crate) fn solve(
    samples: &[Sample],
    named: &BTreeMap<String, i64>,
    symbolic: &BTreeMap<String, i64>,
    reserved: &BTreeMap<usize, u32>,
    oracle: &dyn Oracle,
) -> Option<Vec<Field>> {
    // The most operands any sample has, not the fewest: the reference omits an operand at
    // its default (a flat access's byte offset prints only when non-zero), so the fewest
    // would hide real fields. A position only some samples carry is solved from those; too
    // few samples leave it ambiguous, and ambiguous is refused.
    let count = samples.iter().map(|s| s.operands.len()).max()?;
    if count == 0 {
        return Some(Vec::new());
    }

    let mut solved = Vec::new();
    for position in 0..count {
        let mut found = candidates_for(samples, position, named, symbolic, reserved);
        if found.is_empty() {
            // No field explains this operand: either it is implicit, or the probes never
            // varied it. Identical text in every sample fits both, so the assembler decides.
            let texts: BTreeSet<&str> = samples
                .iter()
                .filter_map(|s| s.operands.get(position).map(String::as_str))
                .collect();
            if texts.len() == 1 && samples.len() > 1 {
                // Not implicit, so the solve stops rather than record a real field as absent.
                if implicit_operand_carries_no_bits(&samples[0], position, oracle) == Some(false) {
                    return None;
                }
                solved.push(Field {
                    word: 0,
                    shift: 0,
                    width: 0,
                    kind: Kind::Implicit,
                    scale: 1,
                    implicit: texts.iter().next().map(|t| (*t).to_owned()),
                });
                continue;
            }
            // Anything unexplained stops the solve rather than yield a partial operand list.
            return None;
        }

        // A multi-register operand names an aligned group, so a scaled reading of another
        // field always fits beside the real unscaled one. An unscaled reading wins where one
        // exists; where scaling is real, no unscaled reading fits.
        if found.iter().any(|f| f.scale == 1) {
            found.retain(|f| f.scale == 1);
        }

        // What remains must agree on how the bits are read: 242 is vector register 242 under
        // one reading and the constant 1.0 under another. A disagreement is unsolved and
        // calls for a better probe.
        let kinds: BTreeSet<(Kind, u32)> = found.iter().map(|f| (f.kind, f.scale)).collect();
        if kinds.len() > 1 {
            return None;
        }

        // Narrowest, then lowest word, then lowest shift, so the answer is deterministic.
        // Scalar registers stop at 101, so probes also use inline constants and special
        // registers, whose codes reach the top of the space; with those, the narrowest
        // consistent field is the real one.
        found.sort_by_key(|f| (f.width, f.word, f.shift));
        solved.push(found.remove(0));
    }
    Some(solved)
}

/// Whether an operand no field explains genuinely occupies no bits.
///
/// Substitutes a different value into the operand and assembles. Refused means nothing
/// else is legal there, so it is implicit; accepted with identical words means it occupies
/// no bits; accepted with different words means a field the probes missed, so it is not
/// implicit. `None` when the probes are inconclusive, and the caller keeps the text-identity
/// rule.
pub(crate) fn implicit_operand_carries_no_bits(
    sample: &Sample,
    position: usize,
    oracle: &dyn Oracle,
) -> Option<bool> {
    let current = sample.operands.get(position)?;
    // Candidates span register files and special names, so one is plausible wherever the
    // operand sits; one equal to the current value proves nothing.
    let mut conclusive = false;
    for candidate in ["vcc", "exec", "s[0:1]", "v0", "s0", "vcc_lo"] {
        if candidate == current {
            continue;
        }
        let Some(text) = substitute(sample, position, candidate) else {
            continue;
        };
        let assembled = oracle.assemble_one(&text);
        let Some(first) = assembled.first() else {
            // Refused: consistent with a fixed operand. Keep looking for an acceptance.
            conclusive = true;
            continue;
        };
        if first.words != sample.words {
            return Some(false);
        }
        return Some(true);
    }
    if conclusive { Some(true) } else { None }
}

/// `sample` with operand `position` replaced by `candidate`, modifiers intact.
///
/// Rebuilt from the printed text, replacing one token in place, rather than
/// `operands.join(", ")`, which drops modifiers some families need to be legal (a typed
/// buffer access needs `format:[...]` and an addressing mode).
#[must_use]
pub(crate) fn substitute(sample: &Sample, position: usize, candidate: &str) -> Option<String> {
    let target = sample.operands.get(position)?;
    let mut seen = 0_usize;
    let mut replaced = false;
    let pieces: Vec<String> = sample
        .printed
        .split(',')
        .map(|piece| {
            let mut out = Vec::new();
            for token in piece.split_whitespace() {
                if !replaced && token == target && seen == position {
                    out.push(candidate.to_owned());
                    replaced = true;
                    continue;
                }
                if MODIFIERS.contains(&token) || crate::patterns::symbolic_modifier(token) {
                    out.push(token.to_owned());
                    continue;
                }
                if token == target {
                    seen += 1;
                }
                out.push(token.to_owned());
            }
            out.join(" ")
        })
        .collect();
    replaced.then(|| format!("{} {}", sample.mnemonic, pieces.join(", ")))
}

/// Widens a field to what the rest of its family reads at the same position.
///
/// A source field belongs to the encoding, so two opcodes of one family reading the same
/// bits at different widths cannot both be right. Sometimes no probe can reach the top
/// bits: `v_cndmask_b32`'s third source is always a scalar pair, so it never exceeds the
/// execution mask's code. The family's widest reading is adopted and every adoption is
/// reported. Only widths are reconciled; a disagreement about kind or scale stays a warning.
pub(crate) fn reconcile_widths(solved: &mut BTreeMap<(String, u32), Solved>) -> Vec<String> {
    // Not keyed by kind: width belongs to the encoding, how an opcode reads the bits does
    // not. `v_interp_p1` reads bits 7:0 as a vector register and `v_interp_mov` reads them
    // as a selector with three legal spellings, which solves two bits wide.
    let mut widest: BTreeMap<(String, usize, u32, u32), u32> = BTreeMap::new();
    for ((family, _), entry) in solved.iter() {
        for field in &entry.fields {
            if field.kind == Kind::Implicit {
                continue;
            }
            let key = (family.clone(), field.word, field.shift, field.scale);
            let slot = widest.entry(key).or_insert(0);
            *slot = (*slot).max(field.width);
        }
    }

    let mut adopted = Vec::new();
    for ((family, _), entry) in solved.iter_mut() {
        for field in &mut entry.fields {
            if field.kind == Kind::Implicit {
                continue;
            }
            let key = (family.clone(), field.word, field.shift, field.scale);
            let best = widest.get(&key).copied().unwrap_or(field.width);
            if field.width < best {
                adopted.push(format!(
                    "  adopted: {} {family} word {} bit {} widened {} -> {best}",
                    entry.mnemonic, field.word, field.shift, field.width
                ));
                field.width = best;
            }
        }
    }
    adopted
}

/// Names fields that opcodes of one family read differently.
///
/// A warning rather than a failure: a field can genuinely differ between opcodes, so the
/// narrower reading is not necessarily wrong.
#[must_use]
pub(crate) fn disagreements(solved: &BTreeMap<(String, u32), Solved>) -> Vec<String> {
    let mut positions: BTreeMap<(String, usize, u32), BTreeMap<u32, Vec<String>>> = BTreeMap::new();
    for ((family, _), entry) in solved {
        for field in &entry.fields {
            // No bits, so no width; every implicit slot nominally sits at word 0 bit 0 and
            // would otherwise collide with a real field there.
            if field.kind == Kind::Implicit {
                continue;
            }
            positions
                .entry((family.clone(), field.word, field.shift))
                .or_default()
                .entry(field.width)
                .or_default()
                .push(entry.mnemonic.clone());
        }
    }

    let mut out = Vec::new();
    for ((family, word, shift), widths) in positions {
        if widths.len() < 2 {
            continue;
        }
        out.push(format!(
            "  disagreement: {family} word {word} bit {shift} is read at {} different widths",
            widths.len()
        ));
        for (width, mut mnemonics) in widths {
            mnemonics.sort_unstable();
            mnemonics.dedup();
            out.push(format!("    {width:2} bits: {}", mnemonics.join(", ")));
        }
        out.push(
            concat!(
                "    a field is a property of the encoding, so at most one of these is right - ",
                "the wider is usually correct and the narrow one wants a probe putting a high ",
                "value in that slot"
            )
            .to_owned(),
        );
    }
    out
}

/// Operand codes that have documented names, read from the decoder's own table.
///
/// A probe using a special register or an inline constant is the only way to force a source
/// field to its full width: scalar registers stop at 101, so register-only samples fit a
/// seven-bit field when the real one is eight.
pub(crate) fn load_named_codes(path: &std::path::Path) -> Result<BTreeMap<String, i64>> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading the operand table at {}", path.display()))?;
    parse_named_codes(&text)
}

/// Parses the operand-code table.
pub(crate) fn parse_named_codes(text: &str) -> Result<BTreeMap<String, i64>> {
    let document: toml::Value = text.parse().context("parsing the operand table")?;
    let mut codes = BTreeMap::new();
    if let Some(rows) = document.get("operand_code").and_then(toml::Value::as_array) {
        for row in rows {
            let (Some(name), Some(first)) = (
                row.get("name").and_then(toml::Value::as_str),
                row.get("first").and_then(toml::Value::as_integer),
            ) else {
                continue;
            };
            codes.insert(name.to_owned(), first);
        }
    }

    // A sixty-four-bit operand names the pair by its low half, which the disassembler
    // spells without the suffix (`exec`, not `exec_lo`). An alias for reading probe output,
    // not a second decoder entry; `s_mov_b64` on the execution mask needs it.
    for (wide, half) in [("exec", "exec_lo"), ("vcc", "vcc_lo")] {
        if let Some(&code) = codes.get(half) {
            codes.insert(wide.to_owned(), code);
        }
    }
    Ok(codes)
}

/// Renders the table, in the shape the decoder reads.
#[must_use]
pub(crate) fn render(solved: &BTreeMap<(String, u32), Solved>) -> String {
    let mut lines: Vec<String> = [
        "# Per-opcode operand fields.",
        "#",
        "# Generated by `orbistoun-gen operands` - do not edit by hand.",
        "#",
        "# Solved, not transcribed. Each entry is the one bit field that explains every",
        "# probe sample of that opcode; samples use varied and high register numbers so a",
        "# coincidence cannot survive and a too-narrow field cannot win. An opcode whose",
        "# operands could not be solved unambiguously is absent rather than approximated.",
        "#",
        "# The target is declared so the loader can refuse a half-retargeted table set.",
        "# An opcode number means a different instruction one generation over, so a table",
        "# from the wrong one is confidently wrong rather than merely incomplete.",
        "",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    lines.push(format!("target = \"{}\"", crate::target::MCPU));
    lines.push(String::new());

    for ((family, opcode), entry) in solved {
        let rendered: Vec<String> = entry.fields.iter().map(render_field).collect();
        lines.push("[[opcode_operands]]".to_owned());
        lines.push(format!("family = \"{family}\""));
        lines.push(format!("opcode = {opcode}"));
        lines.push(format!("mnemonic = \"{}\"", entry.mnemonic));
        lines.push(format!("samples = {}", entry.samples));
        lines.push(format!("operands = [{}]", rendered.join(", ")));
        lines.push(String::new());
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::{
        Field, Kind, Oracle, Solved, candidates_for, disagreements, expected, parse_named_codes,
        reconcile_widths, render_field, reserved_bits_for, solve, split_operand, substitute,
    };
    use crate::assembler::Sample;
    use std::collections::BTreeMap;

    fn sample(mnemonic: &str, printed: &str, words: &[u32]) -> Sample {
        let mut operands = Vec::new();
        for piece in printed.split(',') {
            operands.extend(split_operand(piece));
        }
        Sample {
            mnemonic: mnemonic.to_owned(),
            operands,
            words: words.to_vec(),
            printed: printed.to_owned(),
        }
    }

    /// An oracle that refuses everything: the "nothing else is legal there" case.
    struct Refuses;
    impl Oracle for Refuses {
        fn assemble_one(&self, _: &str) -> Vec<Sample> {
            Vec::new()
        }
    }

    /// An oracle that accepts and returns different bits: the case that must not be
    /// recorded as implicit.
    struct AcceptsDifferently;
    impl Oracle for AcceptsDifferently {
        fn assemble_one(&self, _: &str) -> Vec<Sample> {
            vec![sample("x", "v0", &[0xDEAD_BEEF])]
        }
    }

    /// Modifiers such as `off` are not operands.
    #[test]
    fn modifiers_are_not_operands() {
        assert_eq!(split_operand(" v0 glc slc "), ["v0"]);
        assert_eq!(split_operand(" off "), Vec::<String>::new());
    }

    /// A named immediate becomes its value, so the field carrying it gets a slot.
    ///
    /// Splitting on commas alone would leave `v2 offset:16` as one operand.
    #[test]
    fn a_named_immediate_becomes_a_slot() {
        assert_eq!(split_operand(" v2 offset:16"), ["v2", "16"]);
        assert_eq!(split_operand(" v2 offset:-8"), ["v2", "-8"]);
    }

    /// An attribute splits into number and channel, both of which are real fields.
    #[test]
    fn an_attribute_splits_into_number_and_channel() {
        assert_eq!(split_operand(" attr3.y"), ["3", "1"]);
        assert_eq!(split_operand(" attr0.x"), ["0", "0"]);
        assert_eq!(split_operand(" attr12.w"), ["12", "3"]);
    }

    /// A symbolic modifier is a field of the encoding, not a register operand.
    ///
    /// Recognised explicitly, since an unrecognised token has no reading and would leave
    /// every typed-buffer opcode unsolvable.
    #[test]
    fn a_symbolic_modifier_is_skipped() {
        assert_eq!(split_operand(" v0 format:[BUF_FMT_32_FLOAT] idxen"), ["v0"]);
        // The same without brackets, as the image family prints it.
        assert_eq!(split_operand(" v[4:7] dim:SQ_RSRC_IMG_2D"), ["v[4:7]"]);
    }

    /// A named immediate is not a symbolic modifier: a digit as the value's first character
    /// makes it a number in a field a translator must read.
    #[test]
    fn a_named_immediate_is_not_a_symbolic_modifier() {
        assert_eq!(split_operand("v2 offset:16"), ["v2", "16"]);
        assert_eq!(split_operand("v2 offset:0x20"), ["v2", "0x20"]);
    }

    /// A vector register has two readings, and the samples decide between them.
    #[test]
    fn a_vector_register_reads_two_ways() {
        let readings = expected("v5", &BTreeMap::new(), &BTreeMap::new()).expect("known");
        assert!(readings.contains(&(Kind::Vgpr, 5, 1)));
        assert!(readings.contains(&(Kind::Source, 261, 1)), "256 + 5");
    }

    /// A register pair is offered at every scale a group can be named by.
    #[test]
    fn a_register_group_is_offered_at_every_scale() {
        let readings = expected("s[8:11]", &BTreeMap::new(), &BTreeMap::new()).expect("known");
        assert!(readings.contains(&(Kind::Source, 8, 1)));
        assert!(readings.contains(&(Kind::Source, 8, 2)));
        assert!(
            readings.contains(&(Kind::Source, 8, 4)),
            "a quad names its group"
        );
    }

    /// Negative inline constants sit above the positive ones.
    ///
    /// Without them a sample using -1 has no reading, which fails the whole opcode.
    #[test]
    fn negative_inline_constants_are_offered() {
        let readings = expected("-1", &BTreeMap::new(), &BTreeMap::new()).expect("known");
        assert!(readings.contains(&(Kind::Source, 193, 1)));
        let readings = expected("3", &BTreeMap::new(), &BTreeMap::new()).expect("known");
        assert!(readings.contains(&(Kind::Source, 131, 1)), "128 + 3");
        assert!(readings.contains(&(Kind::Immediate, 3, 1)));
    }

    /// Reserved bits cover the mask, the opcode, and a continuation in another word.
    #[test]
    fn reserved_bits_cover_a_continuation_in_another_word() {
        let encodings = crate::table::parse(
            r#"
[[encoding]]
name = "MTBUF"
mask = "0xFC000000"
value = "0xE8000000"
opcode = { shift = 16, width = 3 }
opcode_extension = { shift = 21, width = 1, word = 1 }
"#,
        )
        .expect("parses");
        let reserved = reserved_bits_for("MTBUF", &encodings);
        assert_eq!(reserved[&0], 0xFC00_0000 | (0b111 << 16));
        assert_eq!(reserved[&1], 1 << 21, "the second word is not forgotten");
    }

    /// A candidate overlapping reserved bits is discarded.
    #[test]
    fn a_candidate_overlapping_reserved_bits_is_discarded() {
        let samples = [
            sample("t", "v0", &[0x0000_0000]),
            sample("t", "v1", &[0x0000_0001]),
        ];
        let free = candidates_for(
            &samples,
            0,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
        );
        let mut reserved = BTreeMap::new();
        reserved.insert(0_usize, 0xFFFF_FFFF_u32);
        let blocked = candidates_for(&samples, 0, &BTreeMap::new(), &BTreeMap::new(), &reserved);
        assert!(!free.is_empty());
        assert!(blocked.is_empty(), "every bit was spoken for");
    }

    /// An operand accepted with different bits is not implicit: the encoding carries it.
    #[test]
    fn an_operand_that_changes_the_bits_is_not_implicit() {
        let samples = [
            sample("v_cmp", "vcc, v0", &[0x1000_0000]),
            sample("v_cmp", "vcc, v1", &[0x1000_0001]),
        ];
        assert_eq!(
            solve(
                &samples,
                &BTreeMap::new(),
                &BTreeMap::new(),
                &BTreeMap::new(),
                &AcceptsDifferently
            ),
            None,
            "a field exists and the probes missed it - the solve must stop"
        );
    }

    /// An operand nothing else is legal in is implicit, and carries no bits.
    #[test]
    fn an_operand_nothing_else_is_legal_in_is_implicit() {
        let samples = [
            sample("v_cmp", "vcc, v0", &[0x1000_0000]),
            sample("v_cmp", "vcc, v1", &[0x1000_0001]),
        ];
        let fields = solve(
            &samples,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &Refuses,
        )
        .expect("solvable");
        assert_eq!(fields[0].kind, Kind::Implicit);
        assert_eq!(fields[0].width, 0);
        assert_eq!(fields[0].implicit.as_deref(), Some("vcc"));
    }

    /// Substitution keeps the modifiers, because for some families they make it legal.
    #[test]
    fn substitution_keeps_the_modifiers() {
        let s = sample(
            "tbuffer_load_format_x",
            "v1, v2, s[8:11], s3 format:[BUF_FMT_32_FLOAT] idxen",
            &[0, 0],
        );
        let text = substitute(&s, 0, "v9").expect("substitutes");
        assert!(text.contains("format:[BUF_FMT_32_FLOAT]"), "{text}");
        assert!(text.contains("idxen"), "{text}");
        assert!(text.starts_with("tbuffer_load_format_x v9,"), "{text}");
    }

    /// Reconciliation widens, reports, and leaves implicit slots alone.
    #[test]
    fn reconciliation_widens_and_reports() {
        let field = |width| Field {
            word: 0,
            shift: 9,
            width,
            kind: Kind::Source,
            scale: 1,
            implicit: None,
        };
        let mut solved = BTreeMap::new();
        solved.insert(
            ("VOP3".to_owned(), 1),
            Solved {
                mnemonic: "narrow".to_owned(),
                fields: vec![field(7)],
                samples: 3,
            },
        );
        solved.insert(
            ("VOP3".to_owned(), 2),
            Solved {
                mnemonic: "wide".to_owned(),
                fields: vec![field(9)],
                samples: 3,
            },
        );
        let adopted = reconcile_widths(&mut solved);
        assert_eq!(adopted.len(), 1, "one widening, reported");
        assert!(adopted[0].contains("narrow"), "{adopted:?}");
        assert_eq!(solved[&("VOP3".to_owned(), 1)].fields[0].width, 9);
    }

    /// A disagreement is named, and an implicit slot never causes a false one.
    ///
    /// Every implicit slot nominally sits at word 0 bit 0 and must not collide with a real
    /// field there.
    #[test]
    fn an_implicit_slot_does_not_cause_a_false_disagreement() {
        let implicit = Field {
            word: 0,
            shift: 0,
            width: 0,
            kind: Kind::Implicit,
            scale: 1,
            implicit: Some("vcc".to_owned()),
        };
        let real = Field {
            word: 0,
            shift: 0,
            width: 8,
            kind: Kind::Source,
            scale: 1,
            implicit: None,
        };
        let mut solved = BTreeMap::new();
        solved.insert(
            ("VOPC".to_owned(), 1),
            Solved {
                mnemonic: "a".to_owned(),
                fields: vec![implicit],
                samples: 2,
            },
        );
        solved.insert(
            ("VOPC".to_owned(), 2),
            Solved {
                mnemonic: "b".to_owned(),
                fields: vec![real],
                samples: 2,
            },
        );
        assert!(disagreements(&solved).is_empty());
    }

    /// A field renders exactly as the decoder's table spells it.
    #[test]
    fn a_field_renders_as_the_table_spells_it() {
        let field = Field {
            word: 1,
            shift: 9,
            width: 8,
            kind: Kind::Source,
            scale: 2,
            implicit: None,
        };
        assert_eq!(
            render_field(&field),
            "{ word = 1, shift = 9, width = 8, kind = \"source\", scale = 2 }"
        );
        let implicit = Field {
            implicit: Some("vcc".to_owned()),
            kind: Kind::Implicit,
            width: 0,
            ..field
        };
        assert!(render_field(&implicit).ends_with(", implicit = \"vcc\" }"));
    }

    /// The wide aliases are added, because the disassembler prints them.
    #[test]
    fn the_sixty_four_bit_aliases_are_added() {
        let codes = parse_named_codes(concat!(
            "[[operand_code]]\nname = \"exec_lo\"\nfirst = 126\n\n",
            "[[operand_code]]\nname = \"vcc_lo\"\nfirst = 106\n"
        ))
        .expect("parses");
        assert_eq!(codes.get("exec"), Some(&126), "printed without the suffix");
        assert_eq!(codes.get("vcc"), Some(&106));
    }
}

/// The live oracle: a real assembler, one line at a time.
pub(crate) struct LiveOracle<'a> {
    /// Where to get encodings from.
    pub(crate) source: &'a crate::assembler::Source,
    /// Where to write recordings, when one is being taken.
    ///
    /// Carried because these probes must be recorded too, or a replay yields a different
    /// table for the families that need symbolic codes.
    pub(crate) record: Option<&'a std::path::Path>,
}

impl Oracle for LiveOracle<'_> {
    fn assemble_one(&self, text: &str) -> Vec<Sample> {
        let input = format!("{text}\n");
        // Keyed by the question, so on replay each symbolic-code probe reads its own
        // recording rather than one shared answer (D209).
        let key = format!("operands-probe-{}", crate::assembler::key_for(&input));
        let Ok(output) = crate::assembler::assemble(self.source, &key, &input, self.record) else {
            return Vec::new();
        };
        let parsed = crate::assembler::parse(&input, &output);
        // A refusal anywhere voids the answer: the caller asks whether this line assembled.
        if parsed.rejected.is_empty() {
            parsed.samples
        } else {
            Vec::new()
        }
    }
}

/// Codes for operands that are names rather than registers or numbers.
///
/// Measured rather than transcribed (D085): assemble the same instruction with only the
/// name changed, and the bits that move are the field. The candidate spellings are
/// enumerated; a spelling that does not exist is refused, and one that does has its code
/// read off the encoding.
#[must_use]
pub(crate) fn derive_symbolic_codes(oracle: &dyn Oracle) -> BTreeMap<String, i64> {
    let mut exports: Vec<String> = (0..8).map(|n| format!("mrt{n}")).collect();
    exports.extend(["mrtz".to_owned(), "null".to_owned(), "prim".to_owned()]);
    exports.extend((0..4).map(|n| format!("pos{n}")));
    exports.extend((0..32).map(|n| format!("param{n}")));

    let families: [(&str, Vec<String>); 2] = [
        ("exp {name} v0, v1, v2, v3", exports),
        (
            "v_interp_mov_f32_e32 v0, {name}, attr0.x",
            vec!["p10".to_owned(), "p20".to_owned(), "p0".to_owned()],
        ),
    ];

    let mut codes = BTreeMap::new();
    for (template, candidates) in families {
        let mut assembled: Vec<(String, Vec<u32>)> = Vec::new();
        for name in &candidates {
            let samples = oracle.assemble_one(&template.replace("{name}", name));
            if let Some(first) = samples.first() {
                assembled.push((name.clone(), first.words.clone()));
            }
        }
        if assembled.len() < 2 {
            continue;
        }

        // Bits that differ between any two spellings, per word: an export keeps its target
        // in the first word and its sources in the second, and a combined mask would not
        // say which word holds the field.
        let reference = assembled[0].1.clone();
        let width = assembled.iter().map(|(_, w)| w.len()).min().unwrap_or(0);
        let mut varying = vec![0_u32; width];
        for (_, words) in &assembled {
            for index in 0..width {
                varying[index] |= words[index] ^ reference[index];
            }
        }

        let moved: Vec<usize> = (0..width).filter(|&i| varying[i] != 0).collect();
        if moved.len() != 1 {
            // Nothing varied, or more than one word did: not one field.
            continue;
        }
        let word_index = moved[0];
        let bits = varying[word_index];
        let shift = bits.trailing_zeros();
        let mask = bits >> shift;
        // A field is contiguous; non-contiguous moving bits mean more than one thing changed.
        if mask & mask.wrapping_add(1) != 0 {
            continue;
        }
        for (name, words) in &assembled {
            codes.insert(name.clone(), i64::from((words[word_index] >> shift) & mask));
        }
    }
    codes
}

/// What a whole run established.
#[derive(Debug, Default)]
pub(crate) struct Report {
    /// Every opcode that solved.
    pub(crate) solved: BTreeMap<(String, u32), Solved>,
    /// How many distinct opcodes the probes reached.
    pub(crate) probed: usize,
    /// Probes this target rejected.
    ///
    /// Printed in full rather than counted: these are the probes this generation lacks, and
    /// the list is the retarget work.
    pub(crate) rejected: Vec<String>,
    /// Opcodes whose operands could not be solved unambiguously.
    pub(crate) unsolved: Vec<String>,
    /// Widths adopted from elsewhere in the family.
    pub(crate) adopted: Vec<String>,
    /// Fields one family reads at more than one width.
    pub(crate) disagreements: Vec<String>,
}

/// Runs the whole generator.
pub(crate) fn run(
    source: &crate::assembler::Source,
    probes_dir: &std::path::Path,
    encodings: &[Encoding],
    named: &BTreeMap<String, i64>,
    record: Option<&std::path::Path>,
) -> Result<Report> {
    let oracle = LiveOracle { source, record };
    // Measured first, because two families' operands cannot be read without it.
    let symbolic = derive_symbolic_codes(&oracle);

    let mut by_opcode: BTreeMap<(String, u32), Vec<Sample>> = BTreeMap::new();
    let mut report = Report::default();

    let mut paths: Vec<std::path::PathBuf> = std::fs::read_dir(probes_dir)
        .with_context(|| format!("reading {}", probes_dir.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "s"))
        .collect();
    paths.sort();
    anyhow::ensure!(!paths.is_empty(), "no probes in {}", probes_dir.display());

    for path in &paths {
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("probe");
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let key = format!("operands-{stem}");
        if let crate::assembler::Source::Transcript(dir) = source {
            crate::assembler::check_recording(dir, &key, &text)?;
        }
        let output = crate::assembler::assemble(source, &key, &text, record)?;
        let parsed = crate::assembler::parse(&text, &output);
        for rejection in &parsed.rejected {
            report.rejected.push(format!(
                "{stem}:{} {} - {}",
                rejection.line, rejection.probe, rejection.why
            ));
        }
        for sample in parsed.samples {
            if sample.words.is_empty() {
                continue;
            }
            // Re-split with this module's rules: the shared whitespace split keeps `off` and
            // `glc` as operands, `v2 offset:16` as one token, and `attr3.y` whole.
            let sample = Sample {
                operands: sample.printed.split(',').flat_map(split_operand).collect(),
                ..sample
            };
            if let Some(key) = crate::table::classify(&sample.words, encodings) {
                by_opcode.entry(key).or_default().push(sample);
            }
        }
    }
    report.probed = by_opcode.len();

    for ((family, opcode), samples) in &by_opcode {
        let reserved = reserved_bits_for(family, encodings);
        match solve(samples, named, &symbolic, &reserved, &oracle) {
            Some(fields) => {
                report.solved.insert(
                    (family.clone(), *opcode),
                    Solved {
                        mnemonic: samples[0].mnemonic.clone(),
                        fields,
                        samples: samples.len(),
                    },
                );
            }
            None => report
                .unsolved
                .push(format!("{family}:{opcode:#x} ({})", samples[0].mnemonic)),
        }
    }

    // Reconciled before rendering, because it edits the fields in place.
    report.adopted = reconcile_widths(&mut report.solved);
    report.disagreements = disagreements(&report.solved);

    // A run that solved nothing must not write an empty table over the committed one:
    // without a toolchain every probe is refused.
    anyhow::ensure!(
        !report.solved.is_empty(),
        concat!(
            "no opcode solved - refusing to write an empty table over the committed one. ",
            "This usually means llvm-mc is missing or lacks the AMDGPU target; ",
            "`tools/toolchain/setup.sh` builds a VM that has it."
        )
    );
    Ok(report)
}

/// Renders the report a person reads.
#[must_use]
pub(crate) fn render_report(report: &Report) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{} opcodes probed, {} solved",
        report.probed,
        report.solved.len()
    );
    if !report.rejected.is_empty() {
        let _ = writeln!(
            out,
            "{} probe(s) this target rejected:",
            report.rejected.len()
        );
        for entry in &report.rejected {
            let _ = writeln!(out, "  rejected: {entry}");
        }
    }
    for entry in &report.unsolved {
        let _ = writeln!(out, "  unsolved: {entry}");
    }
    for entry in &report.adopted {
        let _ = writeln!(out, "{entry}");
    }
    for entry in &report.disagreements {
        let _ = writeln!(out, "{entry}");
    }
    out
}
