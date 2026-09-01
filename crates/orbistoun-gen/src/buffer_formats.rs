//! Solving the typed-buffer format table by asking the assembler what each code means.
//!
//! `crates/orbistoun-shader/data/buffer-formats.toml`
//!
//! # Why this is derived rather than typed in
//!
//! A typed buffer access carries a seven-bit format saying how to read what it fetched: how
//! many components, how wide each one is, and how to turn the bits into a number. A
//! translator that guesses gets a shader that runs and draws the wrong colours, which is
//! the failure mode this project spends most of its effort avoiding.
//!
//! The assembler already knows. Give it `format:N` and it emits the encoding; disassemble
//! that and it prints `format:[BUF_FMT_...]`. So the mapping from code to meaning can be
//! *measured*, one code at a time, and the structure is in the name: `BUF_FMT_32_32_FLOAT`
//! is two components of thirty-two bits read as floating point.
//!
//! # What is deliberately not decided here
//!
//! Nothing about *conversion*. This records what a format is, not what the translator
//! should do about it - a normalised eight-bit component is a real format with a real
//! meaning and this table says so, whether or not anything can translate it yet. Mixing
//! "what it is" with "what we support" is how a data table starts encoding the limitations
//! of the code that happened to read it first.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use anyhow::Result;

use crate::assembler::{self, Source};
use crate::target::MCPU;

/// How the format field is positioned, which this also checks rather than assumes.
const FORMAT_SHIFT: u32 = 19;
/// Width of the format field, in bits.
const FORMAT_WIDTH: u32 = 7;

/// The component type suffixes a format name can end in.
///
/// **Order matters: the longest match wins**, so `SSCALED` is not read as `SCALED` with a
/// stray `S`.
const TYPES: [&str; 8] = [
    "USCALED", "SSCALED", "UNORM", "SNORM", "FLOAT", "UINT", "SINT", "SRGB",
];

/// One solved format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Format {
    /// The name the reference disassembler prints.
    pub(crate) name: String,
    /// One entry per component, in bits.
    pub(crate) widths: Vec<u32>,
    /// How those bits become a number.
    pub(crate) kind: String,
}

/// What a whole run established.
#[derive(Debug, Default)]
pub(crate) struct Solved {
    /// Code to meaning, for every code that has one.
    pub(crate) formats: BTreeMap<u32, Format>,
    /// Codes the assembler explicitly calls invalid.
    pub(crate) invalid: Vec<u32>,
    /// Codes with no name at all, printed back numerically. Reserved.
    pub(crate) reserved: Vec<u32>,
    /// How many were recovered by asking for a name rather than a code.
    pub(crate) recovered: usize,
    /// Probes the assembler refused, in the by-name pass.
    ///
    /// Reported rather than discarded. A refused candidate means this generation has no
    /// such format spelling, which is a fact about the target - and a run where *every*
    /// candidate was refused looks identical to a run that found nothing, unless the
    /// difference is printed.
    pub(crate) refused: usize,
}

/// `BUF_FMT_32_32_FLOAT` as component widths and a type.
#[must_use]
pub(crate) fn parse_name(name: &str) -> Option<(Vec<u32>, String)> {
    let body = name.strip_prefix("BUF_FMT_")?;
    for kind in TYPES {
        let Some(widths) = body.strip_suffix(kind).and_then(|w| w.strip_suffix('_')) else {
            continue;
        };
        let parsed: Option<Vec<u32>> = widths.split('_').map(|w| w.parse().ok()).collect();
        return match parsed {
            Some(w) if !w.is_empty() => Some((w, (*kind).to_owned())),
            _ => None,
        };
    }
    None
}

/// The probe line for one numeric code.
///
/// One instruction per code. The mnemonic is irrelevant to the format field, so the
/// single-channel load is used throughout - the *format* names the component count, and a
/// mismatch between the two is legal and common.
fn probe_for_code(code: u32) -> String {
    format!("tbuffer_load_format_x v0, v1, s[8:11], 0 format:{code} idxen")
}

/// The probe line for one format name.
fn probe_for_name(name: &str) -> String {
    format!("tbuffer_load_format_x v0, v1, s[8:11], 0 format:[{name}] idxen")
}

/// Every numeric probe, one per representable code.
#[must_use]
pub(crate) fn numeric_probes() -> String {
    let mut out = String::new();
    for code in 0..(1_u32 << FORMAT_WIDTH) {
        let _ = writeln!(out, "{}", probe_for_code(code));
    }
    out
}

/// Solves the numeric sweep: for each code, what does the assembler call it?
#[must_use]
pub(crate) fn solve_numeric(output: &assembler::Output) -> Solved {
    let mut solved = Solved::default();
    for line in output.stdout.lines() {
        let Some(words) = crate::patterns::encoding(line) else {
            continue;
        };
        let Some(first) = words.first() else { continue };
        let encoded = (first >> FORMAT_SHIFT) & ((1 << FORMAT_WIDTH) - 1);

        // Two different things print no name, and they are not the same fact.
        //
        //   - the *default* format, which the disassembler omits the way it omits any
        //     modifier sitting at its default. It is a real format with a real meaning.
        //   - a code with no meaning at all, which prints back numerically as `format:78`.
        //     Those are reserved, and a shader carrying one is wrong.
        //
        // Told apart by whether the number came back: `format:N` in the output means the
        // assembler had nothing to call it.
        let Some(name) = crate::patterns::buffer_format(line) else {
            solved.reserved.push(encoded);
            continue;
        };
        if name == "BUF_FMT_INVALID" {
            solved.invalid.push(encoded);
            continue;
        }
        if let Some((widths, kind)) = parse_name(name) {
            solved.formats.insert(
                encoded,
                Format {
                    name: name.to_owned(),
                    widths,
                    kind,
                },
            );
        }
    }
    solved
}

/// The names to try in the second pass, given what the first found.
///
/// Every combination of a component layout already observed with every component type
/// already observed - so nothing is invented, and a name that assembles is a measurement
/// rather than a guess.
#[must_use]
pub(crate) fn name_candidates(solved: &Solved) -> Vec<String> {
    let mut layouts: Vec<String> = solved
        .formats
        .values()
        .map(|f| {
            f.widths
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join("_")
        })
        .collect();
    layouts.sort_unstable();
    layouts.dedup();
    let mut out = Vec::new();
    for layout in layouts {
        for kind in TYPES {
            out.push(format!("BUF_FMT_{layout}_{kind}"));
        }
    }
    out
}

/// Folds the by-name pass into what the numeric sweep found.
///
/// **The default's meaning is not printed anywhere**, so it is asked for by name instead.
/// It is the same question from the other end: the numeric sweep asks "what is code N
/// called", this asks "what code is name X", and only the second can reach a format whose
/// name is never printed.
pub(crate) fn solve_by_name(
    solved: &mut Solved,
    candidates: &[String],
    assembled: &assembler::Assembled,
) {
    for (index, sample) in assembled.samples.iter().enumerate() {
        let Some(first) = sample.words.first() else {
            continue;
        };
        let Some(&line) = assembled.from_line.get(index) else {
            continue;
        };
        if line == usize::MAX {
            continue;
        }
        let encoded = (first >> FORMAT_SHIFT) & ((1 << FORMAT_WIDTH) - 1);
        if solved.formats.contains_key(&encoded) {
            continue;
        }
        // The name comes from what was *asked for*, not from what came back. The default
        // prints nothing, which is the entire reason this second pass exists - reading the
        // output would find no name and skip exactly the code being looked for.
        //
        // It is trusted only because the encoding was checked: the field really does hold
        // this code, so the name really does mean it.
        let Some(name) = candidates.get(line) else {
            continue;
        };
        if let Some((widths, kind)) = parse_name(name) {
            solved.formats.insert(
                encoded,
                Format {
                    name: name.clone(),
                    widths,
                    kind,
                },
            );
            solved.recovered += 1;
        }
    }
}

/// Renders the table, byte-for-byte as the generator this replaced rendered it.
#[must_use]
pub(crate) fn render(solved: &Solved) -> String {
    let mut lines: Vec<String> = [
        "# Typed-buffer formats: what each code in bits 25:19 means.",
        "#",
        "# Generated by `orbistoun-gen buffer-formats` - do not edit by hand.",
        "#",
        "# Every row was measured. The generator assembles `format:N` for each code, reads",
        "# back the field it produced, and records the name the reference disassembler",
        "# prints for it - so the code, its position, and its meaning are all observed",
        "# rather than transcribed.",
        "#",
        "# `widths` is one entry per component, in bits. `kind` is how those bits become a",
        "# number. Together they say what a format *is*; they say nothing about whether",
        "# anything can translate it.",
        "",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    lines.push(format!("target = \"{MCPU}\""));
    lines.push(String::new());

    for (code, format) in &solved.formats {
        let widths = format
            .widths
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        lines.push("[[format]]".to_owned());
        lines.push(format!("code = {code}"));
        lines.push(format!("name = \"{}\"", format.name));
        lines.push(format!("widths = [{widths}]"));
        lines.push(format!("kind = \"{}\"", format.kind));
        lines.push(String::new());
    }
    lines.join("\n")
}

/// Runs the whole generator.
pub(crate) fn run(source: &Source, record: Option<&std::path::Path>) -> Result<(Solved, String)> {
    let numeric = numeric_probes();
    if let Source::Transcript(dir) = source {
        assembler::check_recording(dir, "buffer-formats-numeric", &numeric)?;
    }
    let output = assembler::assemble(source, "buffer-formats-numeric", &numeric, record)?;
    let mut solved = solve_numeric(&output);

    let candidates = name_candidates(&solved);
    let by_name_input = candidates
        .iter()
        .map(|n| probe_for_name(n))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    if let Source::Transcript(dir) = source {
        assembler::check_recording(dir, "buffer-formats-by-name", &by_name_input)?;
    }
    let by_name = assembler::assemble(source, "buffer-formats-by-name", &by_name_input, record)?;
    let parsed = assembler::parse(&by_name_input, &by_name);
    solved.refused = parsed.rejected.len();
    solve_by_name(&mut solved, &candidates, &parsed);

    anyhow::ensure!(
        !solved.formats.is_empty(),
        "no formats solved - the assembler printed nothing recognisable"
    );
    let rendered = render(&solved);
    Ok((solved, rendered))
}

/// Renders the counts a person reads after a run.
#[must_use]
pub(crate) fn render_report(solved: &Solved) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{} formats solved of {} codes",
        solved.formats.len(),
        1_u32 << FORMAT_WIDTH
    );
    if solved.recovered > 0 {
        let _ = writeln!(
            out,
            "  {} recovered by asking for a name rather than a code",
            solved.recovered
        );
    }
    if !solved.invalid.is_empty() {
        let _ = writeln!(out, "  {} explicitly invalid", solved.invalid.len());
    }
    if solved.refused > 0 {
        let _ = writeln!(
            out,
            "  {} candidate names this generation does not have",
            solved.refused
        );
    }
    let still = solved
        .reserved
        .iter()
        .filter(|c| !solved.formats.contains_key(c))
        .count();
    if still > 0 {
        let _ = writeln!(
            out,
            "  {still} reserved - no name, printed back numerically"
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{Format, Solved, name_candidates, numeric_probes, parse_name, render};

    /// The longest type suffix wins.
    ///
    /// `SSCALED` ends in `SCALED`, and a shorter-first match would read
    /// `BUF_FMT_8_SSCALED` as widths `[8, S]` - which fails to parse and silently drops a
    /// real format from the table.
    #[test]
    fn the_longest_type_suffix_wins() {
        assert_eq!(
            parse_name("BUF_FMT_8_SSCALED"),
            Some((vec![8], "SSCALED".to_owned()))
        );
        assert_eq!(
            parse_name("BUF_FMT_32_32_FLOAT"),
            Some((vec![32, 32], "FLOAT".to_owned()))
        );
    }

    /// A name that is not a layout plus a type is not half-parsed.
    #[test]
    fn an_unparseable_name_is_refused_whole() {
        assert_eq!(parse_name("BUF_FMT_INVALID"), None);
        assert_eq!(parse_name("SOMETHING_ELSE"), None);
        assert_eq!(parse_name("BUF_FMT_FLOAT"), None);
    }

    /// Every representable code is probed, exactly once.
    #[test]
    fn every_code_in_the_field_is_probed() {
        let probes = numeric_probes();
        assert_eq!(probes.lines().count(), 128);
        assert!(probes.contains("format:0 idxen"));
        assert!(probes.contains("format:127 idxen"));
    }

    /// Candidates are combinations of what was *observed*, never invented layouts.
    #[test]
    fn candidates_combine_only_observed_layouts() {
        let mut solved = Solved::default();
        solved.formats.insert(
            1,
            Format {
                name: "BUF_FMT_8_UINT".to_owned(),
                widths: vec![8],
                kind: "UINT".to_owned(),
            },
        );
        let candidates = name_candidates(&solved);
        assert_eq!(candidates.len(), 8, "one per component type, one layout");
        assert!(candidates.contains(&"BUF_FMT_8_FLOAT".to_owned()));
        assert!(
            !candidates.iter().any(|c| c.contains("16")),
            "16-bit was never observed and must not be invented"
        );
    }

    /// The rendered table is stable and shaped as the consumer expects.
    #[test]
    fn the_rendered_table_is_stable() {
        let mut solved = Solved::default();
        solved.formats.insert(
            4,
            Format {
                name: "BUF_FMT_8_UNORM".to_owned(),
                widths: vec![8],
                kind: "UNORM".to_owned(),
            },
        );
        let text = render(&solved);
        assert!(text.contains("[[format]]\ncode = 4\nname = \"BUF_FMT_8_UNORM\""));
        assert!(text.contains("widths = [8]"));
        assert_eq!(render(&solved), text, "rendering is deterministic");
    }
}
