//! Solving each encoding family's identifying bits from assembled samples.
//!
//! `crates/orbistoun-shader/data/encodings.toml` says, for each family, which bits
//! identify it, where its opcode sits and how wide its instructions are. Those bit patterns
//! are solved from assembled bytes, as the per-opcode operand layouts are (D085), rather
//! than transcribed from a reference.
//!
//! Family membership is declared by a person: `families/VOP3.s` holds instructions the
//! published reference places in that family. The mask, value, opcode position and width,
//! and instruction width are all solved.
//!
//! This reports rather than writes, because the table also carries hand-maintained
//! reasoning and citations for each row.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use anyhow::{Context, Result};

use crate::assembler::{self, Source};
use crate::solve;

/// One family, solved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Family {
    /// The probe file's stem, such as `VOP3` or `SOP1`.
    pub(crate) name: String,
    /// Bits that identify the family.
    pub(crate) mask: u32,
    /// What those bits hold.
    pub(crate) value: u32,
    /// Where the opcode starts.
    pub(crate) shift: u32,
    /// How wide the opcode is.
    pub(crate) width: u32,
    /// Instruction length, in bytes.
    pub(crate) width_bytes: usize,
    /// How many samples the answer rests on, after sweeping.
    pub(crate) samples: usize,
}

/// What a whole run established.
#[derive(Debug, Default)]
pub(crate) struct Report {
    /// Families that solved.
    pub(crate) solved: Vec<Family>,
    /// Everything that stopped one solving, in the order noticed.
    pub(crate) problems: Vec<String>,
}

/// Reads the probe files a person wrote, one per family.
fn probe_files(dir: &Path) -> Result<Vec<(String, String)>> {
    let mut entries: Vec<(String, String)> = Vec::new();
    let read = std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))?;
    for entry in read.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "s") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        entries.push((stem.to_owned(), text));
    }
    // Sorted, so a run is reproducible.
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    anyhow::ensure!(!entries.is_empty(), "no family probes in {}", dir.display());
    Ok(entries)
}

/// Solves every family in `dir`.
pub(crate) fn run(source: &Source, dir: &Path, record: Option<&Path>) -> Result<Report> {
    let mut report = Report::default();
    let mut probed: BTreeMap<String, Vec<(String, Vec<u32>)>> = BTreeMap::new();

    for (family, text) in probe_files(dir)? {
        let key = format!("encodings-{family}");
        if let Source::Transcript(t) = source {
            assembler::check_recording(t, &key, &text)?;
        }
        let output = assembler::assemble(source, &key, &text, record)?;
        let parsed = assembler::parse(&text, &output);

        for rejection in &parsed.rejected {
            report.problems.push(format!(
                "{family}: rejected {} - {}",
                rejection.probe, rejection.why
            ));
        }

        let samples: Vec<(String, Vec<u32>)> = parsed
            .samples
            .iter()
            .filter(|s| !s.words.is_empty())
            .map(|s| (s.mnemonic.clone(), s.words.clone()))
            .collect();
        if samples.len() < 2 {
            report.problems.push(format!(
                "{family}: {} usable sample(s), need at least 2",
                samples.len()
            ));
            continue;
        }
        let widths: std::collections::BTreeSet<usize> =
            samples.iter().map(|(_, w)| w.len() * 4).collect();
        if widths.len() != 1 {
            report.problems.push(format!(
                "{family}: samples disagree on width: {:?} bytes",
                widths.iter().collect::<Vec<_>>()
            ));
            continue;
        }
        probed.insert(family, samples);
    }

    for (family, samples) in &probed {
        // Three steps, each needing the one before: the opcode's start from the probes'
        // operand variation, the mask from what separates this family from the others, then
        // a sweep of the range between them to solve the opcode's width from samples that
        // reach the top of the range.
        let firsts: Vec<u32> = samples
            .iter()
            .filter_map(|(_, w)| w.first().copied())
            .collect();
        if solve::opcode_field(samples, solve::prefix_mask(&firsts)).is_none() {
            report
                .problems
                .push(format!("{family}: opcode field could not be solved"));
            continue;
        }

        // Bounded by the longer of two prefixes: what separates this family from the
        // others, or what its own probes hold constant. Looser would sweep into a
        // neighbouring format; tighter would miss opcodes the probes did not name.
        let others: Vec<Vec<u32>> = probed
            .iter()
            .filter(|(name, _)| *name != family)
            .map(|(_, s)| s.iter().filter_map(|(_, w)| w.first().copied()).collect())
            .collect();
        let mask = solve::separating_mask(&firsts, &others).max(solve::prefix_mask(&firsts));

        let base = &samples[0].1;
        let candidates = solve::sweep_candidates(base, mask);
        let key = format!("encodings-{family}-sweep");
        let names = assembler::disassemble(source, &key, &candidates, record)?;

        let mut found = samples.clone();
        for (name, words) in names.iter().zip(candidates.iter()) {
            if let Some(name) = name {
                found.push((name.clone(), words.clone()));
            }
        }

        let Some((shift, width)) = solve::opcode_field(&found, mask) else {
            report.problems.push(format!(
                "{family}: opcode width could not be solved after sweeping"
            ));
            continue;
        };

        report.solved.push(Family {
            name: family.clone(),
            mask,
            value: firsts[0] & mask,
            shift,
            width,
            width_bytes: found[0].1.len() * 4,
            samples: found.len(),
        });
    }

    Ok(report)
}

/// Renders the report a person reads.
#[must_use]
pub(crate) fn render(report: &Report) -> String {
    let mut out = String::new();
    for f in &report.solved {
        let _ = writeln!(
            out,
            "{:7} mask={:#010x} value={:#010x} opcode={{shift={}, width={}}} width_bytes={}  ({} samples)",
            f.name, f.mask, f.value, f.shift, f.width, f.width_bytes, f.samples
        );
    }
    for p in &report.problems {
        let _ = writeln!(out, "  problem: {p}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{Family, Report, render};

    /// The report lists problems as well as answers.
    ///
    /// An unsolved family must be visible beside the solved ones.
    #[test]
    fn problems_are_reported_alongside_answers() {
        let report = Report {
            solved: vec![Family {
                name: "VOP3".to_owned(),
                mask: 0xFFC0_0000,
                value: 0xD400_0000,
                shift: 16,
                width: 10,
                width_bytes: 8,
                samples: 42,
            }],
            problems: vec!["SOPK: opcode field could not be solved".to_owned()],
        };
        let text = render(&report);
        assert!(text.contains("VOP3"));
        assert!(text.contains("problem: SOPK"));
    }

    /// An empty report says nothing rather than claiming success.
    #[test]
    fn an_empty_report_renders_empty() {
        assert!(render(&Report::default()).is_empty());
    }
}
