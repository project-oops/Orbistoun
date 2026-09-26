//! Turning a measured finding into a change the emulator can carry.
//!
//! A fix the loop proposes is a policy entry, from a rule or from a model: one line in a file that
//! is already a runtime input, reverted by deleting it, with no rebuild (D296). Behaviour no effect
//! can express is Rust, written by a person and graded by the conformance probe. `FURTHER` shows
//! the guest got past something, not that the behaviour is right, so a trial that changes only a
//! return value may be kept on `FURTHER` and one that writes memory needs a conformance check;
//! [`Evidence`] carries the distinction.

use crate::turn::bare;
use orbistoun_hle::learned::{Evidence as Known, Measurement};
use orbistoun_hle::{Delivery, StubRegion, StubReturn};

/// A change the loop earned, in the shape the policy file takes.
///
/// Proposed, never applied here: producing it is a judgement about what the measurement supports,
/// and writing a file is the caller's decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Patch {
    /// The function this changes, bare.
    pub function: String,
    /// What it should answer, where the measurement says.
    pub answers: Option<StubReturn>,
    /// A region to give the guest, and how it should arrive.
    pub region: Option<StubRegion>,
    /// What would have to be true for this to be worth keeping.
    pub evidence: Evidence,
    /// Claims this patch rests on that nothing measured. Never empty for a write.
    pub assumptions: Vec<String>,
}

/// What has to be observed before a patch is kept.
///
/// A field rather than a judgement made at the point of reading, so it is made the same way every
/// time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Evidence {
    /// The guest reached code it could not reach before.
    ///
    /// Enough only for a patch that changes what a function answers. A wrong answer that buys
    /// progress shows as a wall that moved; a wrong write shows only when something unrelated
    /// breaks.
    Further,
    /// A conformance check covering this function passed.
    ///
    /// Required for any patch that touches guest memory: the probe grades named checks against a
    /// spec, the only oracle here that says correct rather than proceeded.
    ConformanceCheck,
}

/// The rule tier: a patch derived from a finding, with nothing guessed.
///
/// `None` when the finding establishes no contract, which is the common answer: a sweep that
/// concluded `Unmoved` found no slot, and turning that into a change would invent one.
///
/// The size of the region behind the base is not measured, since a sweep sees where the guest
/// faulted, not what it asked for. The number is recorded as an assumption in a file where changing
/// it costs a re-run (D300).
#[must_use]
pub fn from_finding(target: &str, finding: &crate::experiment::Finding) -> Option<Patch> {
    /// How much space a region gets, as a multiple of the offset the guest indexed by.
    ///
    /// Twice, because the sweep measures the one access that faulted rather than the extent the
    /// guest intends to use, the reasoning `axis::around` gives.
    const HEADROOM: u64 = 2;
    /// Never smaller than this, so a tiny offset does not produce a region a page could not
    /// hold.
    const SMALLEST: u64 = 0x1_0000;
    /// Page size, which a region is rounded up to.
    ///
    /// Rounded here as well as where it is reserved, so the number in the file covers its own last
    /// byte: `0xfffe0` doubled is half a page short.
    const PAGE: u64 = 0x1000;

    let crate::experiment::Finding::OutParameter {
        slot,
        offset,
        answer,
    } = finding
    else {
        return None;
    };

    let function = target
        .rsplit_once("::")
        .map_or(target, |(_, f)| f)
        .to_owned();
    let bytes = offset
        .unsigned_abs()
        .saturating_mul(HEADROOM)
        .max(SMALLEST)
        .div_ceil(PAGE)
        .saturating_mul(PAGE);

    Some(Patch {
        function,
        // Only where the sweep found the read gated on it. A stub answers a thirty-two-bit code, so
        // a measured answer that does not fit one is dropped rather than folded to `Ok`; the write
        // is still earned.
        answers: answer
            .and_then(|value| u32::try_from(value).ok())
            .map(|raw| {
                if raw == 0 {
                    StubReturn::Ok
                } else {
                    StubReturn::Raw(raw)
                }
            }),
        region: Some(StubRegion {
            via: Delivery::Argument(*slot),
            bytes,
        }),
        // This one writes memory, so a moved wall is not enough to keep it (D296).
        evidence: Evidence::ConformanceCheck,
        // Everything the sweep did not establish: it measures which slot is read and what is added
        // to it, and nothing about the region's size or what the other arguments select (D291).
        assumptions: vec![
            format!(
                "{bytes:#x} bytes is a guess: the sweep measured where the guest faulted, not how much it asked for"
            ),
            format!(
                "nothing measured says what the arguments other than arg{slot} select, so they are ignored"
            ),
        ],
    })
}

/// A patch for the function whose placeholder the guest dereferenced.
///
/// The one auto-keepable shape: it changes what a function answers and writes no memory, so
/// `FURTHER` is sufficient (D296). Zero, because for anything the caller dereferences an error code
/// is a wild pointer, and zero is what a caller tests for (D125). The premise is measured: the
/// guest treating the answer as an address shows the function returns something dereferenceable.
#[must_use]
pub fn from_placeholder_source(function: &str) -> Patch {
    Patch {
        // Bare, like `from_finding`: a `Patch` names a function.
        function: bare(function).to_owned(),
        answers: Some(StubReturn::Ok),
        region: None,
        evidence: Evidence::Further,
        assumptions: vec![
            concat!(
                "zero is what a pointer-returning function must answer rather than an ",
                "error code (D125); what it should really return is not measured"
            )
            .to_owned(),
        ],
    }
}

/// The other answer for a function whose placeholder the guest dereferenced.
///
/// Zero is what a caller may test, not what an allocator is for, so a region delivered through the
/// return is the hypothesis to compare against it (D300). Both are proposed and run, and whichever
/// reaches further is kept.
#[must_use]
pub fn from_placeholder_source_as_region(function: &str) -> Patch {
    /// What an allocator gets when nothing has measured what it wanted.
    ///
    /// A number in a file, labelled as unmeasured: nothing observed says how much the guest intends
    /// to use.
    const UNMEASURED: u64 = 0x10_000;

    Patch {
        function: bare(function).to_owned(),
        answers: None,
        region: Some(StubRegion {
            via: Delivery::Return,
            bytes: UNMEASURED,
        }),
        // It hands over memory the guest will write into, so a moved wall is not enough.
        evidence: Evidence::ConformanceCheck,
        assumptions: vec![
            concat!(
                "that this function is expected to return memory rather than a value the ",
                "caller tests - the guest dereferencing its answer is consistent with both"
            )
            .to_owned(),
            format!(
                "{UNMEASURED:#x} bytes is unmeasured: nothing observed says how much it wanted"
            ),
        ],
    }
}

// Promotion: a measurement, as the change that ships it.

/// A measurement, written as the knowledge-file entry it implies.
///
/// `learned.toml` is one machine's cache; a knowledge file is what the emulator ships, so this is
/// where an observation becomes a claim (D297). Every field comes from the measurement: no
/// `purpose`, no `arity`, no argument list, since a sweep measured none of those and filling them
/// in from the name is what the provenance rules stop. The assumptions travel with it, so the entry
/// never arrives stronger than the observation.
#[must_use]
pub fn knowledge_entry(measurement: &Measurement) -> String {
    use core::fmt::Write as _;

    let mut out = String::new();
    let _ = writeln!(out, "\n[[function]]");
    let _ = writeln!(out, "name = {}", quoted(&measurement.function));
    // No `found_by`: that field says how the name was found, and a measurement establishes how the
    // behaviour was.
    let _ = writeln!(out, "known_by = {}", quoted(measurement.known.label()));
    let _ = writeln!(out, "found_in = [{}]", quoted(&measurement.measured));
    let _ = writeln!(out, "found_on = {}", quoted(&measurement.on));

    let what = match (&measurement.answers, &measurement.region) {
        (Some(answer), Some(region)) => format!(
            "Answers {answer:?} and hands back {:#x} bytes through {}.",
            region.bytes,
            delivery(region.via)
        ),
        (Some(answer), None) => format!("Answers {answer:?}."),
        (None, Some(region)) => format!(
            "Hands back {:#x} bytes through {}.",
            region.bytes,
            delivery(region.via)
        ),
        (None, None) => "Measured, with no effect recorded.".to_owned(),
    };
    let _ = writeln!(
        out,
        "note = {}",
        quoted(&format!(
            "{what} Measured by a sweep against {}, on the evidence of {}.",
            measurement.measured,
            match measurement.evidence {
                Known::Further => "the guest reaching code it could not reach before",
                Known::ConformanceCheck => "a conformance check covering it",
            }
        ))
    );

    if !measurement.assumes.is_empty() {
        let _ = writeln!(out, "assumptions = [");
        for assumption in &measurement.assumes {
            let _ = writeln!(out, "    {},", quoted(assumption));
        }
        let _ = writeln!(out, "]");
    }
    out
}

/// How a region reaches the guest, in words an entry can carry.
fn delivery(via: Delivery) -> String {
    match via {
        Delivery::Argument(slot) => format!("argument {slot}"),
        Delivery::Return => "the return value".to_owned(),
    }
}

/// A TOML string, with the characters that would end it escaped.
///
/// Hand-written rather than a serialiser this crate would hold for nothing else; a measurement's
/// strings are prose, where the only hazards are a quote and a backslash.
fn quoted(text: &str) -> String {
    let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

/// A unified diff that appends to a file, ready for `git apply`.
///
/// An append needs only the context at the end of the file, so it cannot fail to apply, where a
/// generated patch that half-applies is worse than none. Returns nothing when the file already
/// names the function: a second entry is a second claim about the same thing.
#[must_use]
pub fn appending_diff(path: &str, existing: &str, addition: &str) -> String {
    use core::fmt::Write as _;

    let lines: Vec<&str> = existing.lines().collect();
    let context: Vec<&str> = lines.iter().rev().take(3).rev().copied().collect();
    let added: Vec<&str> = addition.lines().collect();
    let start = lines.len().saturating_sub(context.len()) + 1;

    let mut out = String::new();
    let _ = writeln!(out, "--- a/{path}\n+++ b/{path}");
    let _ = writeln!(
        out,
        "@@ -{start},{} +{start},{} @@",
        context.len(),
        context.len() + added.len()
    );
    for line in context {
        let _ = writeln!(out, " {line}");
    }
    for line in added {
        let _ = writeln!(out, "+{line}");
    }
    out
}

/// A unified diff that inserts lines directly after an anchor line.
///
/// An answer belongs inside the entry whose question it settles; appending would add a second
/// entry, which `Learned::record` refuses. Three lines of context either side, so a stale patch
/// fails loudly rather than landing in the wrong place. `None` when the anchor is absent or appears
/// more than once, since a patch aimed at either of two lines cannot be checked.
#[must_use]
pub fn inserting_diff(path: &str, existing: &str, anchor: &str, addition: &str) -> Option<String> {
    use core::fmt::Write as _;

    let lines: Vec<&str> = existing.lines().collect();
    let mut found = lines.iter().enumerate().filter(|(_, l)| **l == anchor);
    let (at, _) = found.next()?;
    if found.next().is_some() {
        return None;
    }

    let before = at.saturating_sub(CONTEXT);
    let after = (at + 1 + CONTEXT).min(lines.len());
    let added: Vec<&str> = addition.lines().collect();
    let span = after - before;

    let mut out = String::new();
    let _ = writeln!(out, "--- a/{path}\n+++ b/{path}");
    let _ = writeln!(
        out,
        "@@ -{},{span} +{},{} @@",
        before + 1,
        before + 1,
        span + added.len()
    );
    for line in &lines[before..=at] {
        let _ = writeln!(out, " {line}");
    }
    for line in &added {
        let _ = writeln!(out, "+{line}");
    }
    for line in &lines[at + 1..after] {
        let _ = writeln!(out, " {line}");
    }
    Some(out)
}

/// Lines of context either side of an insertion.
const CONTEXT: usize = 3;

/// A unified diff that replaces one line with another.
///
/// A key that already exists has to be joined, not added again: inserting a second `edge_cases =
/// [...]` applies cleanly and produces a duplicate key the tool cannot read. `git apply` checks
/// that a patch fits the text, not that the result means anything.
#[must_use]
pub fn replacing_diff(path: &str, existing: &str, at: usize, replacement: &str) -> Option<String> {
    use core::fmt::Write as _;

    let lines: Vec<&str> = existing.lines().collect();
    if at >= lines.len() {
        return None;
    }
    let before = at.saturating_sub(CONTEXT);
    let after = (at + 1 + CONTEXT).min(lines.len());
    let span = after - before;

    let mut out = String::new();
    let _ = writeln!(out, "--- a/{path}\n+++ b/{path}");
    let _ = writeln!(out, "@@ -{},{span} +{},{span} @@", before + 1, before + 1);
    for line in &lines[before..at] {
        let _ = writeln!(out, " {line}");
    }
    let _ = writeln!(out, "-{}", lines[at]);
    let _ = writeln!(out, "+{replacement}");
    for line in &lines[at + 1..after] {
        let _ = writeln!(out, " {line}");
    }
    Some(out)
}

/// Where one entry's list of a given key starts, by line.
///
/// Scoped to the entry, because the file has many: a whole-file search would find whichever came
/// first and put one function's answer into another's entry.
///
/// `None` when the entry has no such key, the caller's signal that adding one is safe.
#[must_use]
pub fn key_line_of(existing: &str, function: &str, key: &str) -> Option<usize> {
    let lines: Vec<&str> = existing.lines().collect();
    let name = format!("name = \"{function}\"");
    let start = lines.iter().position(|l| *l == name)?;
    let prefix = format!("{key} = ");
    lines
        .iter()
        .enumerate()
        .skip(start + 1)
        .take_while(|(_, l)| !l.starts_with("[[function]]"))
        .find(|(_, l)| l.starts_with(&prefix))
        .map(|(at, _)| at)
}

#[cfg(test)]
mod tests {
    use super::{Evidence, from_finding, from_placeholder_source};
    use crate::experiment::Finding;
    use orbistoun_hle::knowledge::Oracle;
    use orbistoun_hle::learned::{Evidence as Known, Measurement};
    use orbistoun_hle::{Delivery, StubRegion, StubReturn};

    /// A measurement, as a sweep produces one.
    fn measured() -> Measurement {
        Measurement {
            function: "sceKernelReserveVirtualRange".to_owned(),
            library: "libkernel".to_owned(),
            measured: "PPSA02664-app0".to_owned(),
            on: "2026-08-27".to_owned(),
            by: "orbistoun 0.1.0".to_owned(),
            known: Oracle::GuestObserved,
            evidence: Known::ConformanceCheck,
            answers: Some(StubReturn::Ok),
            region: Some(StubRegion {
                via: Delivery::Argument(0),
                bytes: 0x20_0000,
            }),
            assumes: vec!["0x200000 bytes is a guess".to_owned()],
        }
    }

    /// The entry says only what was measured: no `purpose`, no `arity`, no argument list.
    #[test]
    fn a_generated_entry_invents_nothing_and_keeps_its_assumptions() {
        let entry = super::knowledge_entry(&measured());

        assert!(
            entry.contains(r#"name = "sceKernelReserveVirtualRange""#),
            "{entry}"
        );
        assert!(entry.contains(r#"known_by = "guest-observed""#), "{entry}");
        assert!(
            entry.contains(r#"found_in = ["PPSA02664-app0"]"#),
            "{entry}"
        );
        assert!(entry.contains("0x200000 bytes is a guess"), "{entry}");
        assert!(entry.contains("argument 0"), "{entry}");
        assert!(
            !entry.contains("arity") && !entry.contains("purpose"),
            "a sweep measured neither: {entry}"
        );
        // Nor how the name was found: `found_by` is about the name, a measurement about the
        // behaviour.
        assert!(
            !entry.contains("found_by"),
            "a measurement does not know how the name was found: {entry}"
        );
    }

    /// The generated block is valid TOML that parses as a knowledge file.
    ///
    /// A patch that produces a file the tool cannot read is worse than no patch.
    #[test]
    fn the_generated_entry_parses_as_a_knowledge_file() {
        let file = format!(
            "library = \"libkernel\"\n{}",
            super::knowledge_entry(&measured())
        );
        let parsed: toml::Value = toml::from_str(&file).expect("valid TOML");

        let functions = parsed["function"].as_array().expect("one function");
        assert_eq!(functions.len(), 1);
        assert_eq!(
            functions[0]["name"].as_str(),
            Some("sceKernelReserveVirtualRange")
        );
    }

    /// A quote in an assumption, which is prose, does not end the string it is in.
    #[test]
    fn prose_containing_a_quote_survives_into_valid_toml() {
        let mut awkward = measured();
        awkward.assumes = vec![r#"the guest calls it "early", before main"#.to_owned()];
        let file = format!(
            "library = \"libkernel\"\n{}",
            super::knowledge_entry(&awkward)
        );

        let parsed: toml::Value = toml::from_str(&file).expect("a quote must not end the string");
        let assumptions = parsed["function"][0]["assumptions"]
            .as_array()
            .expect("assumptions survive");
        assert!(
            assumptions[0].as_str().is_some_and(|s| s.contains('"')),
            "the quote itself has to survive, not just the parse"
        );
    }

    /// The diff appends, and the hunk header counts the lines it writes, as `git apply` requires.
    #[test]
    fn an_appending_diff_has_a_header_matching_its_body() {
        let existing = "library = \"libkernel\"\na = 1\nb = 2\nc = 3\n";
        let diff = super::appending_diff("data/libkernel.toml", existing, "d = 4\ne = 5\n");

        assert!(diff.starts_with("--- a/data/libkernel.toml\n"), "{diff}");
        let header = diff
            .lines()
            .find(|l| l.starts_with("@@"))
            .expect("a hunk header");
        let context = diff.lines().filter(|l| l.starts_with(' ')).count();
        let added = diff
            .lines()
            .filter(|l| l.starts_with('+') && !l.starts_with("+++"))
            .count();

        assert_eq!(context, 3, "three lines of context");
        assert_eq!(added, 2);
        assert_eq!(header, "@@ -2,3 +2,5 @@", "{diff}");
    }

    /// A measured contract becomes a patch that states both halves.
    #[test]
    fn an_out_parameter_becomes_an_answer_and_a_write() {
        let patch = from_finding(
            "libkernel::sceKernelReserveVirtualRange",
            &Finding::OutParameter {
                slot: 0,
                offset: 0xfffe0,
                answer: Some(0),
            },
        )
        .expect("a measured contract is patchable");

        assert_eq!(patch.function, "sceKernelReserveVirtualRange");
        assert_eq!(patch.answers, Some(StubReturn::Ok));
        let write = patch.region.expect("the contract includes a region");
        assert_eq!(write.via, Delivery::Argument(0));
        assert!(
            write.bytes >= 0xfffe0,
            "a region has to cover the offset the guest indexes by"
        );
        assert_eq!(
            write.bytes % 0x1000,
            0,
            "and be a whole number of pages, or it stops short of its own last byte (D289)"
        );
    }

    /// A patch that writes memory is never keepable on a moved wall alone.
    ///
    /// A wrong write shows as something unrelated breaking much later, unlike a wrong answer
    /// (D296).
    #[test]
    fn a_patch_that_writes_memory_needs_a_conformance_check() {
        let patch = from_finding(
            "libkernel::sceFoo",
            &Finding::OutParameter {
                slot: 1,
                offset: -0x20,
                answer: None,
            },
        )
        .expect("patchable");
        assert!(patch.region.is_some());
        assert_eq!(patch.evidence, Evidence::ConformanceCheck);
    }

    /// A patch that rests on a guess says which guess.
    #[test]
    fn the_size_of_the_region_is_recorded_as_unmeasured() {
        let patch = from_finding(
            "libkernel::sceFoo",
            &Finding::OutParameter {
                slot: 0,
                offset: 0x1000,
                answer: Some(0),
            },
        )
        .expect("patchable");
        let assumed = patch.assumptions.join(" | ");
        assert!(assumed.contains("guess"), "{assumed}");
        assert!(
            assumed.contains("not how much it asked for"),
            "the distinction between where it faulted and what it wanted: {assumed}"
        );
    }

    /// The placeholder patch is keepable on a moved wall, and the only one that is.
    ///
    /// It changes an answer and touches no memory, so the cheap oracle is enough; every other patch
    /// here writes (D299).
    #[test]
    fn an_answer_only_patch_is_keepable_on_further_alone() {
        let patch = from_placeholder_source("sceKernelGetGPI");

        assert_eq!(patch.function, "sceKernelGetGPI");
        assert_eq!(patch.answers, Some(StubReturn::Ok));
        assert!(patch.region.is_none(), "it hands over no memory");
        assert_eq!(patch.evidence, Evidence::Further);
        assert!(
            !patch.assumptions.is_empty(),
            "zero is what a caller may test, not a measurement of the right answer"
        );
    }

    /// A sweep that concluded nothing produces no patch.
    #[test]
    fn nothing_measured_is_nothing_changed() {
        for finding in [
            Finding::Unmoved {
                tested: vec![0, 1],
                not_addresses: vec![],
            },
            Finding::NeverPlanted,
            Finding::Dereferenced { slot: 0 },
        ] {
            assert!(
                from_finding("libkernel::sceFoo", &finding).is_none(),
                "{finding:?} establishes no contract to write down"
            );
        }
    }

    /// An answer goes inside the entry whose question it settles, not in a second entry.
    #[test]
    fn an_insertion_lands_under_its_anchor_with_context() {
        let file = "a = 1\nb = 2\nname = \"sceFoo\"\nc = 3\nd = 4\n";
        let diff = super::inserting_diff("k.toml", file, "name = \"sceFoo\"", "note = \"x\"\n")
            .expect("the anchor is there exactly once");

        let added: Vec<&str> = diff
            .lines()
            .filter(|l| l.starts_with('+') && !l.starts_with("+++"))
            .collect();
        assert_eq!(added, vec!["+note = \"x\""]);
        assert!(
            diff.lines().any(|l| l == " name = \"sceFoo\""),
            "the anchor is context, not an addition: {diff}"
        );
        let header = diff.lines().find(|l| l.starts_with("@@")).expect("a hunk");
        let context = diff.lines().filter(|l| l.starts_with(' ')).count();
        assert_eq!(header, "@@ -1,5 +1,6 @@", "{diff}");
        assert_eq!(context, 5);
    }

    /// An anchor that appears twice is refused rather than guessed at.
    #[test]
    fn an_ambiguous_anchor_produces_no_patch() {
        let file = "name = \"sceFoo\"\nx = 1\nname = \"sceFoo\"\n";

        assert!(super::inserting_diff("k.toml", file, "name = \"sceFoo\"", "note = 1\n").is_none());
    }

    /// An anchor that is not there produces nothing, rather than an empty hunk.
    #[test]
    fn a_missing_anchor_produces_no_patch() {
        assert!(
            super::inserting_diff("k.toml", "a = 1\n", "name = \"sceNope\"", "x = 1\n").is_none()
        );
    }
}
