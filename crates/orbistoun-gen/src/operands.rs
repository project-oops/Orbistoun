//! Solving per-opcode operand layouts from assembled probes.
//!
//! `crates/orbistoun-shader/data/opcode-operands.toml`
//!
//! # Solved, not transcribed
//!
//! Each entry is the one bit field that explains every probe sample of that opcode. Probes
//! use varied and high register numbers so a coincidence cannot survive and a too-narrow
//! field cannot win. **An opcode whose operands could not be solved unambiguously is absent
//! rather than approximated** - that refusal is the whole design, and most of the comments
//! below are about cases where it fired for the right reason and cases where it fired for
//! the wrong one.
//!
//! # The oracle seam
//!
//! Three questions here cannot be answered from bits alone - whether an unexplained operand
//! is genuinely implicit, whether a probe *could* reach the bits a widening had to guess at,
//! and what code a symbolic name like `mrt0` carries. All three are answered by assembling
//! something and looking, so they go through [`Oracle`] - which the tests substitute, and
//! which is what makes the solver testable with no toolchain at all.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use anyhow::{Context, Result};

use crate::assembler::Sample;
use crate::table::Encoding;

/// Tokens the reference prints among the operands that are not operands at all.
///
/// `off` says there is no scalar base register; the cache hints say how the access behaves.
/// None of them is a field, and treating them as one made every flat load and store
/// unsolvable - the solver looked for bits encoding the word "off".
const MODIFIERS: [&str; 10] = [
    "off", "glc", "slc", "dlc", "lds", "gds", "offen", "idxen", "tfe", "nv",
];

/// Field shapes worth trying, for register selectors.
///
/// Register fields in this architecture are six to nine bits. **Five**, because a buffer
/// resource selector names a *group* of four consecutive scalar registers rather than a
/// register, so it needs a quarter of the range and a quarter of the bits. Every field
/// solved before that one selected a single register and none was narrower than six, which
/// is why the lower bound had never been tested - the buffer accesses simply reported as
/// unsolvable, and an unsolvable opcode looks like a gap in the probes rather than a gap in
/// the solver.
const WIDTHS: std::ops::Range<u32> = 5..10;

/// Widths an immediate field can have.
///
/// Immediates are far wider than register selectors and their widths are not the same set -
/// a memory offset and a branch target are both immediates and neither is eight bits.
///
/// **Two and three**, because a *selector* is an immediate too and they are tiny: an
/// interpolation names one of four channels in two bits. Without them the three
/// interpolation opcodes had no candidate for that operand at all.
///
/// Widening the search cannot produce a wrong answer, only fewer answers: an extra width
/// that also fits makes an operand *ambiguous*, and the solver refuses rather than picking.
const IMMEDIATE_WIDTHS: [u32; 6] = [2, 3, 16, 20, 21, 32];

/// How a field's bits are read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Kind {
    /// A direct vector-register index.
    Vgpr,
    /// The shared source numbering - registers, special registers, inline constants.
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
/// The seam. Live, it shells out; in a test it is a table of canned answers, which is what
/// lets every path below - including the two that ask the assembler a question - run with no
/// toolchain installed.
pub(crate) trait Oracle {
    /// Assembles one line. An empty result means it was refused.
    fn assemble_one(&self, text: &str) -> Vec<Sample>;
}

/// Splits one comma-separated piece into the operands it actually contains.
///
/// Usually one. A piece carrying trailing modifiers becomes the operand plus whatever named
/// immediates followed it, in the order printed - so a field the reference reports only as
/// `name:value` still gets a slot the solver can find.
#[must_use]
pub(crate) fn split_operand(piece: &str) -> Vec<String> {
    let mut out = Vec::new();
    for token in piece.split_whitespace() {
        if MODIFIERS.contains(&token) || crate::patterns::symbolic_modifier(token) {
            continue;
        }
        if let Some((number, channel)) = crate::patterns::attribute(token) {
            // Split into two operands rather than skipped, because both are real fields
            // the decoder has to read: which attribute, and which of its four channels.
            //
            // `xyzw` = 0 to 3 is the only ordering they could have - and it is *checked*
            // rather than assumed, because the solver only finds a field if the values it
            // was given fit one consistently across every sample. Give it the wrong
            // ordering and no field explains the samples, so it refuses. An input to a
            // solve that would fail if it were wrong is a different thing from a guess.
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
/// A field holds a *code*, not a register number, and one operand text can be consistent
/// with more than one reading - a vector register is a direct index in some fields and sits
/// at 256 upward in the shared numbering. All plausible readings are returned and the
/// samples decide between them.
///
/// A multi-register operand is reduced to its base. The field encodes where the group
/// starts; how far it extends is a property of the instruction.
#[must_use]
pub(crate) fn expected(
    operand: &str,
    named: &BTreeMap<String, i64>,
    symbolic: &BTreeMap<String, i64>,
) -> Option<Vec<Reading>> {
    if let Some(&code) = symbolic.get(operand) {
        // An export target or an interpolation parameter: a selector, not a register, so
        // the only reading offered is the raw number. Its code was measured.
        return Some(vec![(Kind::Immediate, code, 1)]);
    }
    if let Some(&code) = named.get(operand) {
        // A special register or an inline float. Its code is fixed, and these are what push
        // a source field past the range registers alone can reach.
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
        // Scale four as well as two: a *pair* is named by a field holding half its base
        // register, and a *quad* - a buffer resource constant - by one holding a quarter.
        // Only the readings listed here are ever looked for, so an operand whose scale is
        // absent has no candidate field at all and its opcode reports as unsolvable, which
        // reads as a gap in the probes rather than a gap in the solver.
        return Some(vec![
            (Kind::Source, number, 1),
            (Kind::Source, number, 2),
            (Kind::Source, number, 4),
        ]);
    }
    let value = crate::patterns::immediate(operand)?;
    let mut readings = vec![(Kind::Immediate, value, 1)];
    // A small integer written plainly is an inline constant in a source field, and the same
    // text in an offset field is just the number. Both are offered.
    if (0..=64).contains(&value) {
        readings.push((Kind::Source, 128 + value, 1));
    } else if (-16..=-1).contains(&value) {
        // The negative inline constants sit above the positive ones: -1 through -16 at 193
        // upward. Omitting them made `s_mov_b64` unsolvable, with nothing to say why - the
        // sample using -1 had no reading any field could explain, and one operand with no
        // candidates fails the whole opcode.
        readings.push((Kind::Source, 192 + (-value), 1));
    }
    Some(readings)
}

/// Every field shape and reading that explains this operand in every sample.
///
/// `reserved` maps a word index to the bits of that word the encoding table has already
/// spoken for - the family's fixed bits, its opcode field, and any continuation of that
/// opcode. No operand can live in those, and a candidate overlapping them is reading a
/// constant, or a bit belonging to the opcode, as part of a value.
///
/// **Keyed by word rather than a single mask** because an opcode is not always kept in the
/// first one. The typed-buffer family puts its fourth opcode bit at 53, which is bit 21 of
/// the second word, and a mask covering only the first word would leave a candidate free to
/// swallow it.
///
/// That filter is not tidiness. `v_cmp_lt_f32_e32` was unsolvable without it: its second
/// source is an eight-bit vector register at bit 9, and a *nine*-bit window at the same
/// place reads the register plus 256 - which is exactly that register in the shared
/// numbering. Both readings explained every sample and always would, because the ninth bit
/// is part of the opcode and is 1 for this opcode in every instruction that has it.
#[must_use]
pub(crate) fn candidates_for(
    samples: &[Sample],
    position: usize,
    named: &BTreeMap<String, i64>,
    symbolic: &BTreeMap<String, i64>,
    reserved: &BTreeMap<usize, u32>,
) -> Vec<Field> {
    let mut wanted: Vec<(&Sample, Vec<Reading>)> = Vec::new();
    for sample in samples {
        let Some(operand) = sample.operands.get(position) else {
            return Vec::new();
        };
        let Some(readings) = expected(operand, named, symbolic) else {
            return Vec::new();
        };
        wanted.push((sample, readings));
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
                    // Four, because a buffer resource constant lives in *four* consecutive
                    // scalar registers and its field holds the group index rather than the
                    // register number. Without this the buffer accesses solved to a field
                    // one bit lower at scale two - the same product for every sample given,
                    // and wrong for the first one with a high data register, because the
                    // bit it borrowed belonged to the field next door.
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
/// Three sources, and they are the same argument three times.
///
/// The **opcode field**, because the encoding table extracts the opcode from those bits, so
/// an operand cannot also live there (D109).
///
/// The **family's fixed bits** - the encoding's own mask - because they are constant across
/// every instruction in the family by definition. A candidate overlapping them is reading a
/// constant as part of a value, and it will fit every sample forever, which is what makes it
/// so hard to notice.
///
/// The **opcode's continuation**, where a family keeps its opcode in two pieces. This is why
/// the result is keyed by word: the typed-buffer family's fourth opcode bit is at 53, in the
/// *second* word, and a first-word mask cannot exclude it.
///
/// D109 excluded only the first and left the second, and the gap cost the two most common
/// instructions in the set. `v_mov_b32_e32` writes its destination at bit 17, and a nine-bit
/// window there reaches bit 25 - the low bit of VOP1's mask, permanently 1. So the window
/// reads `v0` as 256, which is exactly `v0`'s code in the shared numbering: an eight-bit
/// vector register and a nine-bit source both explain every sample, neither can be
/// eliminated, and the solve stops as ambiguous. The symptom was the whole opcode missing
/// rather than a wrong field - the solver behaving correctly - which also means a move
/// decoded with no operands at all, and nothing said why.
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
    let count = samples.iter().map(|s| s.operands.len()).min()?;
    if count == 0 {
        return Some(Vec::new());
    }

    let mut solved = Vec::new();
    for position in 0..count {
        let mut found = candidates_for(samples, position, named, symbolic, reserved);
        if found.is_empty() {
            // No field anywhere explains this operand. Two things look like that, and they
            // are not the same: the operand is *implicit* and the encoding does not carry
            // it, or the probes never varied it and a real field went unnoticed.
            //
            // Identical text in every sample is consistent with **both**, so it is a
            // precondition and not the answer. The answer comes from the assembler.
            let texts: BTreeSet<&str> = samples
                .iter()
                .filter_map(|s| s.operands.get(position).map(String::as_str))
                .collect();
            if texts.len() == 1 && samples.len() > 1 {
                // Not implicit. Stopping the solve is right: continuing would record a
                // field that exists as one that does not.
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
            // Anything unexplained stops the solve. A partial operand list presented as
            // complete is the failure this whole effort exists to avoid.
            return None;
        }

        // A multi-register operand names an *aligned* group - a pair is always even, a quad
        // always a multiple of four - so a scaled reading of some other field always fits
        // alongside the unscaled reading of the real one. That is a genuine ambiguity in the
        // samples and it made every wide load and store unsolvable.
        //
        // Scaling is the exception rather than the rule, so an unscaled reading wins where
        // one exists. Safe because where scaling is real the unscaled reading simply does
        // not fit: a base field holding 3 for register 6 is explained by scale two and by
        // nothing else.
        if found.iter().any(|f| f.scale == 1) {
            found.retain(|f| f.scale == 1);
        }

        // What remains must agree on how the bits are read. Two readings that decode
        // differently are not a tie to be broken - a field holding 242 is vector register
        // 242 under one and the constant 1.0 under the other, and samples that only ever put
        // a register there cannot tell them apart. Reported as unsolved; the cure is a
        // better probe, not a coin toss.
        let kinds: BTreeSet<(Kind, u32)> = found.iter().map(|f| (f.kind, f.scale)).collect();
        if kinds.len() > 1 {
            return None;
        }

        // Narrowest wins, then lowest word, then lowest shift, so the answer is
        // deterministic. High register numbers alone are not enough to make that safe:
        // scalar registers stop at 101, so a seven-bit field explains every register sample
        // a real eight-bit field does. The probes therefore also use inline constants and
        // special registers, whose codes reach the top of the space - and with those in the
        // set, the narrowest consistent field is the real one.
        found.sort_by_key(|f| (f.width, f.word, f.shift));
        solved.push(found.remove(0));
    }
    Some(solved)
}

/// Whether an operand no field explains genuinely occupies no bits.
///
/// **Asking instead of asserting.** Substitute a different value into that operand and
/// assemble. Three outcomes, and the third is the one an earlier version's prose did not
/// allow for:
///
/// - **refused** - nothing else is legal there, so the operand is fixed. Implicit.
/// - **accepted, words identical** - the strongest evidence available. The operand
///   demonstrably occupies no bits, because changing it changed no bit.
/// - **accepted, words differ** - there *is* a field and the probes missed it. Calling that
///   implicit records an operand as un-encoded while the encoding carries it, and every
///   decode silently prints the sample's value instead of the real one.
///
/// `None` when nothing was accepted or refused informatively enough to say - the caller
/// keeps the original text-identity rule rather than losing a solve to an inconclusive probe.
pub(crate) fn implicit_operand_carries_no_bits(
    sample: &Sample,
    position: usize,
    oracle: &dyn Oracle,
) -> Option<bool> {
    let current = sample.operands.get(position)?;
    // Spread across register files and the special names, so at least one is plausible
    // wherever the operand sits. A candidate equal to what is already there proves nothing.
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
            // Refused. Consistent with the operand being fixed; keep looking in case
            // something else is accepted, which would be more informative.
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
/// **Not `operands.join(", ")`.** That drops the modifiers, and for some families the
/// modifiers are what make the instruction legal - a typed buffer access needs its
/// `format:[...]` and an addressing mode, and without them it is refused. Rebuilt from the
/// text the reference printed instead, replacing one token in place.
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
/// A source field is a property of the *encoding*, so two opcodes of one family reading the
/// same bits at different widths cannot both be right. Usually the narrow one solved that way
/// because no probe put a high enough value in that slot, and the cure is a better probe.
///
/// Sometimes no probe can. `v_cndmask_b32` takes a sixty-four-bit mask as its third source,
/// and a mask is always a scalar pair - so the highest value that field can legally hold is
/// the execution mask's code, and the top bits are unreachable by any instruction the
/// assembler will emit. Left alone it would warn forever, and **a check that always warns is
/// a check nobody reads**.
///
/// So the family's widest reading is adopted, and every adoption is reported. Only widths
/// are reconciled: two readings that disagree about *kind* or *scale* decode differently and
/// are a real ambiguity, which stays a warning.
pub(crate) fn reconcile_widths(solved: &mut BTreeMap<(String, u32), Solved>) -> Vec<String> {
    // Deliberately **not** keyed by kind. A field's width is a property of the encoding; how
    // a particular opcode reads those bits is not. The interpolation family proves the
    // point - `v_interp_p1` reads bits 7:0 as a vector register and `v_interp_mov` reads the
    // same bits as a parameter selector, and the selector has only three legal spellings, so
    // it solves two bits wide and nothing can probe it wider. Keyed by kind, those two never
    // met and reconciliation had nothing to compare.
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
/// This has happened four times, in three separate sittings, and every time it was caught by
/// a person putting the generated rows side by side and noticing. **That is not a check, it
/// is a habit** - and the last round produced three opcodes each too narrow in a *different*
/// source slot, which is exactly the pattern reading down a column misses.
///
/// A warning rather than a failure. The narrower reading is not necessarily wrong: a field
/// genuinely can differ between opcodes, and refusing to generate would make an unprovable
/// claim in the other direction. Naming it is enough, because the cure is always the same.
#[must_use]
pub(crate) fn disagreements(solved: &BTreeMap<(String, u32), Solved>) -> Vec<String> {
    let mut positions: BTreeMap<(String, usize, u32), BTreeMap<u32, Vec<String>>> = BTreeMap::new();
    for ((family, _), entry) in solved {
        for field in &entry.fields {
            // No bits, so no width to disagree about - and every implicit slot nominally
            // sits at word 0 bit 0, which would otherwise collide with a real field there
            // and report a disagreement that is not one.
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
/// Needed because a probe using a special register or an inline constant is the only way to
/// force a source field to its full width: scalar registers stop at 101, so samples using
/// only registers can be explained by a seven-bit field when the real one is eight. The first
/// attempt used registers alone and solved exactly that too-narrow field - correct on every
/// sample it was given, and wrong on the first instruction carrying a literal.
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

    // A sixty-four-bit operand names the pair by its low half, and the disassembler spells
    // that without the suffix: `exec`, not `exec_lo`. Same code, different width, so this is
    // an alias for reading probe output rather than a second entry the decoder should carry.
    //
    // Without it, every `s_mov_b64` sample touching the execution mask has no expected
    // reading at all, and the opcode reports as unsolved with nothing to say why. That is
    // exactly the instruction the mask is manipulated with, so the samples cannot be dropped.
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

    /// An oracle that accepts, and returns different bits: the case that must NOT be
    /// recorded as implicit.
    struct AcceptsDifferently;
    impl Oracle for AcceptsDifferently {
        fn assemble_one(&self, _: &str) -> Vec<Sample> {
            vec![sample("x", "v0", &[0xDEAD_BEEF])]
        }
    }

    /// Modifiers are not operands, and treating one as a field made every flat access
    /// unsolvable - the solver looked for bits encoding the word "off".
    #[test]
    fn modifiers_are_not_operands() {
        assert_eq!(split_operand(" v0 glc slc "), ["v0"]);
        assert_eq!(split_operand(" off "), Vec::<String>::new());
    }

    /// A named immediate becomes its value, so the field carrying it gets a slot.
    ///
    /// Splitting on commas alone leaves `v2 offset:16` as one operand, which matches no
    /// register pattern - so the opcode reports as unsolvable and the offset field, which is
    /// real and which a translator must read, is never looked for.
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
    /// Recognised explicitly rather than left to fall through: an unrecognised token has no
    /// reading the solver can find, so every typed-buffer opcode would report as unsolvable -
    /// which looks like a gap in the probes and is not one.
    #[test]
    fn a_symbolic_modifier_is_skipped() {
        assert_eq!(split_operand(" v0 format:[BUF_FMT_32_FLOAT] idxen"), ["v0"]);
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
    /// Omitting them made `s_mov_b64` unsolvable with nothing to say why: the sample using
    /// -1 had no reading any field could explain, and one operand with no candidates fails
    /// the whole opcode.
    #[test]
    fn negative_inline_constants_are_offered() {
        let readings = expected("-1", &BTreeMap::new(), &BTreeMap::new()).expect("known");
        assert!(readings.contains(&(Kind::Source, 193, 1)));
        let readings = expected("3", &BTreeMap::new(), &BTreeMap::new()).expect("known");
        assert!(readings.contains(&(Kind::Source, 131, 1)), "128 + 3");
        assert!(readings.contains(&(Kind::Immediate, 3, 1)));
    }

    /// Reserved bits cover the mask, the opcode, **and** a continuation in another word.
    ///
    /// D109 excluded only the first word and the gap cost the two most common instructions
    /// in the set.
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

    /// An operand accepted with *different* bits is not implicit.
    ///
    /// **The case the original prose did not allow for.** Recording it as implicit would
    /// mark an operand as un-encoded while the encoding carries it, and every decode would
    /// silently print the sample's value instead of the real one.
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
    /// Every implicit slot nominally sits at word 0 bit 0, which would otherwise collide
    /// with a real field there and report a disagreement that is not one.
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
    /// Carried rather than ignored: these probes are the part of the solve that cannot be
    /// replayed without them, so a recording that omitted them replays into a *different*
    /// table - quietly, and only for the two families that need symbolic codes.
    pub(crate) record: Option<&'a std::path::Path>,
}

impl Oracle for LiveOracle<'_> {
    fn assemble_one(&self, text: &str) -> Vec<Sample> {
        let input = format!("{text}\n");
        // **Keyed by what is being asked, not by the fact that something is.** A single
        // fixed key works live - each call re-invokes the assembler - and is silently wrong
        // on replay, where all forty-seven symbolic-code probes read the same recording and
        // get the same canned answer. The codes then come out empty and the two families
        // that need them, `exp` and `v_interp_mov`, drop out of the table entirely.
        //
        // Found by diffing a replay against the committed table, which is the whole reason
        // that diff exists (D209).
        let key = format!("operands-probe-{}", crate::assembler::key_for(&input));
        let Ok(output) = crate::assembler::assemble(self.source, &key, &input, self.record) else {
            return Vec::new();
        };
        let parsed = crate::assembler::parse(&input, &output);
        // A refusal anywhere makes the whole answer untrustworthy: the caller is asking
        // whether *this* line assembled, and a partial result would answer about another.
        if parsed.rejected.is_empty() {
            parsed.samples
        } else {
            Vec::new()
        }
    }
}

/// Codes for operands that are names rather than registers or numbers.
///
/// # Deriving rather than transcribing
///
/// The obvious approach is to write down that `mrt0` is 0 and `pos0` is 12, from the
/// reference. That is not done here, for the same reason nothing else is: a transcribed
/// number cannot be checked without the document it came from, and the failure mode is a
/// decoder that reports the wrong export target for the rest of the project's life.
///
/// Instead the code is **measured**, by the same move the encoding solver uses to find a
/// family's mask. Assemble the same instruction twice, changing only the name. Everything
/// that stays the same is not the field; the bits that move are.
///
/// The candidate *spellings* are enumerated. That is not the same as transcribing their
/// values: a spelling that does not exist is refused by the assembler and drops out, and one
/// that does exist has its code read off the encoding rather than assumed.
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

        // Bits that differ between any two spellings, tracked **per word**. Everything else
        // is the rest of the instruction, which was deliberately held constant.
        //
        // Per word rather than combined: an export keeps its target in the first word and
        // its sources in the second, and a single mask over both would not say which word
        // the field is in. Combining them happened to work for these two families and would
        // have been wrong for the first family where it mattered.
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
            // Nothing varied, or more than one word did. Either way this is not one field
            // and reading it as one would invent a number.
            continue;
        }
        let word_index = moved[0];
        let bits = varying[word_index];
        let shift = bits.trailing_zeros();
        let mask = bits >> shift;
        // A field is contiguous. Bits that moved in more than one run mean more than one
        // thing changed with the name, and their combined value is not a code.
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
    /// Printed in full rather than counted: these are the probes this generation does not
    /// have, and the list is the retarget worklist. A count says how much work there is and
    /// nothing about what it is.
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
    // Measured before anything is solved, because two families' operands cannot be read
    // without it. One round trip, and it is the difference between those opcodes solving and
    // reporting as a gap in the probes.
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
            // **Re-split with this module's rules, not the shared ones.** The shared parser
            // splits on whitespace, which is right for the encoding solver and wrong here:
            // it leaves `off` and `glc` as operands, keeps `v2 offset:16` as one token, and
            // never separates `attr3.y` into the two fields it is. Every flat access, every
            // typed buffer access and every interpolation reported as unsolvable when the
            // splitting was shared, because the solver was looking for bits encoding the
            // word "off".
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

    // Reconciled before anything is rendered: it edits the fields in place, and a line
    // already formatted would not see the change.
    report.adopted = reconcile_widths(&mut report.solved);
    report.disagreements = disagreements(&report.solved);

    // **A run that solved nothing must not write an empty table over the committed one.**
    // Same reasoning as the fixture generator: without a toolchain every probe is refused,
    // and a table with no rows in it silently removes every operand layout the decoder has.
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
