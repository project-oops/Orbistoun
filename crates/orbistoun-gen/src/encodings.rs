//! Solving each encoding family's identifying bits from assembled samples.
//!
//! `crates/orbistoun-shader/data/encodings.toml` says, for each family: which bits identify
//! it, where its opcode sits, and how wide its instructions are. Those rows were
//! hand-transcribed from a published reference, and when the project's target generation
//! turned out to be the wrong one (D139) three of them were silently wrong in the worst
//! possible way - this generation's long-form vector format sits exactly where the previous
//! one's interpolation format did, so a long-form arithmetic instruction decoded as an
//! interpolation and nothing said so.
//!
//! So the rows are solved here instead, the same way the per-opcode operand layouts are
//! (D085): assemble instructions, look at the bytes, and derive the answer.
//!
//! # What is transcribed and what is solved
//!
//! **Membership is declared by a person.** `families/VOP3.s` holds instructions a reader of
//! the published reference says belong to that family. Reading a specification and writing
//! code from it is ordinary engineering.
//!
//! **Every bit pattern is solved.** The mask, the value, the opcode's position and width,
//! and the instruction width all come from the assembled bytes. Nothing here reads a number
//! out of a reference, which is the derivation D085 refuses.
//!
//! # This reports; it does not write
//!
//! `data/encodings.toml` is not purely generated. It carries the reasoning behind each row
//! and citations into the published reference, which is where a wrong row gets *corrected*
//! from - so a person edits it, acting on what this says. Overwriting it would throw that
//! away and leave a table nobody could check without the document that produced it.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use anyhow::{Context, Result};

use crate::assembler::{self, Source};
use crate::solve;

/// One family, solved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Family {
    /// The probe file's stem - `VOP3`, `SOP1`.
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
    // Sorted, so a run is reproducible and a diff between two runs means something.
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
        // Three steps, in this order because each needs the one before it.
        //
        // The opcode's *start* comes from the probe file's operand variation. Its mask comes
        // from what separates this family from the others - not from what is constant within
        // it, which the probes cannot answer. Only then can the range between the two be
        // swept for the rest of the family, and the opcode's *width* solved from a sample
        // set that actually reaches the top of the range.
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

        // Bounded by whichever prefix is *longer*: what separates this family from the
        // others, or what its own probes hold constant. Too loose and the sweep walks into a
        // neighbouring format and brings its instructions back as members of this one; too
        // tight and it cannot reach the opcodes the probes missed. The longer of the two is
        // the only bound that is safe in both directions.
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
    /// A run that solved nine families and could not solve the tenth is not a success, and
    /// printing only the nine is how that becomes invisible.
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
