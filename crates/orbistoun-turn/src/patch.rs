//! Turning a measured finding into a change the emulator can carry.
//!
//! The loop measures a contract, satisfies it, and proves it helped. This is what it does
//! next: emit the change, in a form that costs no rebuild and reverts by deleting a line.
//!
//! # Three tiers, and the third is the smallest
//!
//! | tier | who proposes | what it emits | rebuild |
//! |---|---|---|---|
//! | **1** | a rule | a policy entry | no |
//! | **2** | a model | a policy entry | no |
//! | **3** | a person, or a model | Rust | yes |
//!
//! **The output being data is what removes the person, not the model being good.** A policy
//! entry is one line in a file that is already a runtime input: blast radius is one line, undo
//! is deleting it, the loop re-runs in seconds. Rust is none of those things and no amount of
//! model quality changes any of them - which is why anything expressible as an effect belongs
//! in the first two tiers, and only real logic is left for the third (D296).
//!
//! # What may be accepted on what evidence
//!
//! `FURTHER` means the guest executed code it could not reach before. It does **not** mean the
//! behaviour is right, and principle 3's opening sentence is this exact failure: *"a stub that
//! returns success is indistinguishable from working code until forty thousand frames later"*.
//!
//! So a trial that changes only a **return value** may be accepted on `FURTHER`, and a trial
//! that **writes memory** may not - that needs a conformance check covering it. [`Evidence`]
//! carries the distinction so a caller cannot lose it.

use crate::turn::bare;
use orbistoun_hle::learned::{Evidence as Known, Measurement};
use orbistoun_hle::{Delivery, StubRegion, StubReturn};

/// A change the loop earned, in the shape the policy file takes.
///
/// **Proposed, never applied here.** Producing it is a judgement about what the measurement
/// supports; writing a file is a decision about a machine, and the two belong to different
/// layers - the same split `promote` already makes for a knowledge entry (D291).
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
/// **A field rather than a judgement made at the time**, for the same reason `Effect` is one
/// in `orbistoun-env`: the distinction decides what a result means, and one made by hand at
/// the point of reading is one that gets made differently next time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Evidence {
    /// The guest reached code it could not reach before.
    ///
    /// Enough **only** for a patch that changes what a function answers. A wrong answer that
    /// buys progress is visible as a wall that moved; a wrong *write* is not visible at all
    /// until something unrelated breaks.
    Further,
    /// A conformance check covering this function passed.
    ///
    /// Required for any patch that touches guest memory. The probe announces each check by
    /// name and grades against a spec, which is the only oracle here that says *correct*
    /// rather than *proceeded*.
    ConformanceCheck,
}

/// The rule tier: a patch derived from a finding, with nothing guessed.
///
/// `None` when the finding establishes no contract - which is the common answer and not a
/// failure. A sweep that concluded `Unmoved` measured every slot and found none of them, and
/// turning that into a change would be inventing one.
///
/// # What it assumes, and says so
///
/// The size of the region behind the base is **not measured**: a sweep sees where the guest
/// faulted, not what it asked for. A number goes in because one has to, it is recorded as an
/// assumption, and it lives in a file where changing it costs a re-run rather than a rebuild -
/// which is the whole reason policy is data (D291, D295).
#[must_use]
pub fn from_finding(target: &str, finding: &crate::experiment::Finding) -> Option<Patch> {
    /// How much space a region gets, as a multiple of the offset the guest indexed by.
    ///
    /// Twice, because the sweep measures the one access that faulted rather than the extent
    /// the guest intends to use, and a region sized to exactly that access answers a narrower
    /// question than the one worth asking - the reasoning `axis::around` already gives.
    const HEADROOM: u64 = 2;
    /// Never smaller than this, so a tiny offset does not produce a region a page could not
    /// hold.
    const SMALLEST: u64 = 0x1_0000;
    /// Page size, which a region is rounded up to.
    ///
    /// **Rounded here rather than only where it is reserved.** `0xfffe0` doubled is `0x1fffc0`,
    /// half a page short of covering its own last byte - and a run at exactly that size faults
    /// *inside the region it was just given*, which reads as "the base was not the problem".
    /// The service rounds too, so this is belt and braces; what it buys is a number in the
    /// file that means what it says (D289).
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
        // Only where the sweep found the read to be gated on it. A patch that forced an answer
        // nothing measured would be a guess wearing a measurement's clothes.
        // **Refused rather than narrowed.** A stub answers a thirty-two-bit code, so a measured
        // answer that does not fit one cannot be expressed - and folding it to `Ok` would put a
        // value in the file that nothing measured, which is the one thing a patcher must never
        // do (principle 3). Dropping it leaves the write, which is still earned.
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
        // **Everything the sweep did not establish.** It measures which slot is read and what
        // is added to it; it measures nothing about how large a region the guest intends to
        // use or what the other arguments select. Left out, the entry would read as though it
        // had (D291).
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
/// **The only shape here that is auto-keepable.** It changes what a function *answers* and
/// writes no memory, so `FURTHER` is sufficient by the rule in [`Evidence`] - a wrong answer
/// that buys progress shows up as a wall that moved, where a wrong write does not show up
/// until something unrelated breaks (D296).
///
/// # Why zero rather than something chosen
///
/// D125 settled it: *"for anything the caller dereferences, an error code is a wild pointer -
/// so those answer zero, which is what a caller already tests for."* And the premise is
/// **measured**, not assumed: the guest treating the answer as an address is the evidence that
/// the function returns something dereferenceable (D299).
///
/// Zero is not a guess at the right answer. It is the answer a caller is entitled to test, and
/// a null it checks is worth more than a wild pointer it follows.
#[must_use]
pub fn from_placeholder_source(function: &str) -> Patch {
    Patch {
        // Bare, like `from_finding` - the qualified form is what the caller needed to keep
        // the library, and a `Patch` names a function (D355).
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
/// **The hypothesis the rule made it impossible to state.** D125 says a pointer-returning
/// function must not answer an error code, so the loop answered zero and the guest accepted it.
/// Zero is what a caller may *test*; it is not what an allocator is *for*. Until a region could
/// be delivered through the return, there was nothing to compare "answer zero" against, and a
/// result nothing was compared against is a rule that was followed rather than a measurement
/// (D300).
///
/// Both are proposed and both are run. Whichever reaches further is the one kept, which is the
/// exhaustive-rather-than-ranked discipline every other sweep here already uses (D231).
#[must_use]
pub fn from_placeholder_source_as_region(function: &str) -> Patch {
    /// What an allocator gets when nothing has measured what it wanted.
    ///
    /// **A number in a file, and labelled.** Nothing observed says how much the guest intends
    /// to use; a snapshot of what it actually touches would say, and does not exist yet.
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

// --- Tier 2: a model proposing entries in the same grammar --------------------
//
// **Not built.** The shape is settled and the reason for the order is measured: tier 1 needs
// no model and closes the loop for the contract it already produces, and tier 2's vocabulary
// is worth sizing against a title nobody has worked on before it is written. Of the six gaps
// the probe still reports, one is effect-shaped and five are real logic - so the vocabulary's
// value is front-loaded and mostly already spent on functions written by hand (D296).
//
// When it is built it emits a [`Patch`] like this one and is checked the same way, so the only
// new thing is where the proposal comes from - which is the arrangement `Step::NameAHash`
// already uses, and the only one this project trusts a model inside.

// --- Tier 3: Rust, for behaviour no effect can express ------------------------
//
// **Not built, and deliberately the smallest.** A pseudo-random sequence, a symbol lookup,
// formatted output: none is an effect, all need code, and code needs a rebuild - which turns a
// try-measure-revert cycle from milliseconds into minutes and inverts the economics the whole
// dispatcher was designed around (D231).
//
// Its oracle is the conformance probe rather than `FURTHER`, because `FURTHER` cannot tell
// `sqrt` from a `sqrt` that returns its argument, and the probe grades against a spec.

// --- Promotion: a measurement, as the change that ships it -------------------

/// A measurement, written as the knowledge-file entry it implies.
///
/// # Why this is the change worth generating
///
/// `learned.toml` is one machine's cache. A knowledge file is what the emulator **ships**, so
/// a measurement becoming an entry is the moment a thing one person watched happen turns into
/// something the project claims - which is exactly what promotion means here (D297).
///
/// **Every field comes from the measurement, and none is invented.** That is what keeps a
/// generated change clear of principle 1: nothing is recalled, so nothing can be recall
/// dressed as reasoning. The entry is deliberately partial - no `purpose`, no `arity`, no
/// argument list - because a sweep measured none of those, and filling them in from the name
/// is the exact move the provenance rules exist to stop.
///
/// The assumptions travel. They are the difference between a measurement and an assertion,
/// and an entry that shed them on the way would arrive stronger than it left.
#[must_use]
pub fn knowledge_entry(measurement: &Measurement) -> String {
    use core::fmt::Write as _;

    let mut out = String::new();
    let _ = writeln!(out, "\n[[function]]");
    let _ = writeln!(out, "name = {}", quoted(&measurement.function));
    // **No `found_by`.** That field says how the *name* was found - harvested, generated,
    // supplied - and a measurement establishes how the *behaviour* was. Claiming one from the
    // other put `generated` on a name the symbol database re-derives as `static`, and the
    // shipped-files provenance check caught it within a minute of the patch being applied. A
    // generated entry inventing a field is the single thing this function is written not to
    // do, and it did it in the first draft (D328).
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
/// Small and hand-written because the alternative is a serialiser in a crate that has no
/// other reason to hold one - and because a measurement's strings are prose, where the only
/// hazards are a quote and a backslash.
fn quoted(text: &str) -> String {
    let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

/// A unified diff that appends to a file, ready for `git apply`.
///
/// # Why an append, and only an append
///
/// **A diff that cannot fail to apply is worth more than one that reads better.** Inserting
/// into the middle of a file means matching context that may have moved, and a generated patch
/// that half-applies is worse than none - a reviewer then has to work out what the generator
/// meant rather than what it did. Appending needs three lines of context at the end of a file
/// nobody else is appending to.
///
/// Returns nothing when the file already names the function: a second entry for one function
/// is two claims about the same thing, and nothing here can say which is current.
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
/// # Why an insertion and not another append
///
/// An **answer** belongs inside the entry whose question it settles, and appending would add
/// a second entry for a function that already has one - two claims about the same thing, which
/// `Learned::record` refuses for exactly this reason. So this matches a line and puts the
/// addition under it.
///
/// Three lines of context either side, which is what `git apply` wants and what makes a stale
/// patch fail loudly rather than land in the wrong place.
///
/// `None` when the anchor is absent or appears more than once: a patch aimed at a line that
/// might be either of two is a patch nobody can check, and refusing beats guessing which
/// (D358).
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
/// # Why an answer needs this and an insertion will not do
///
/// The first version of this inserted `edge_cases = [...]` under the entry's name. It applied
/// cleanly and produced **`duplicate key edge_cases in table function`** - a file the tool
/// could no longer read, from a patch `git apply` was perfectly happy with.
///
/// A key that already exists has to be *joined*, not added again. `git apply` checks that a
/// patch fits the text; nothing in it checks that the result means anything, which is the same
/// distinction that let a generated entry claim `found_by = generated` for a name the database
/// re-derives as static (D328, D358).
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
/// **Scoped to the entry, because the file has many.** Searching the whole file for
/// `edge_cases = [` would find whichever came first and put an answer about one function into
/// another's entry - a patch that applies, parses, and is a lie.
///
/// `None` when the entry has no such key, which is the caller's signal that adding one is safe.
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

    /// **The entry says only what was measured.**
    ///
    /// No `purpose`, no `arity`, no argument list: a sweep measured none of those, and filling
    /// them in from the name is the move principle 1 exists to stop. What a generated change
    /// must never do is arrive looking better-founded than the observation behind it.
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
        // **And not how the name was found.** `found_by` is about the *name* - harvested,
        // generated, supplied - and a measurement is about the behaviour. The first draft
        // claimed `generated` and the shipped-files provenance check refused it, because the
        // symbol database re-derives that name as `static` (D328).
        assert!(
            !entry.contains("found_by"),
            "a measurement does not know how the name was found: {entry}"
        );
    }

    /// The generated block is valid TOML that parses as a knowledge file.
    ///
    /// **The property a generator has to earn.** A patch that produces a file the tool cannot
    /// read is worse than no patch: it costs a reviewer the time to find out.
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

    /// **A quote in an assumption does not end the string it is in.**
    ///
    /// Assumptions are prose written by whoever ran the loop, so this is reachable rather
    /// than theoretical - and a generator that emits unparseable TOML on one entry has
    /// produced a patch nobody can apply and a reviewer has to debug.
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

    /// **The diff appends, and the header counts the lines it actually writes.**
    ///
    /// A hunk header that disagrees with its body is rejected by `git apply`, and a generated
    /// patch that does not apply is worse than none - a reviewer works out what the generator
    /// meant rather than what it did.
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
    /// **The rule this file exists to hold.** A wrong answer that buys progress shows up as a
    /// wall that moved; a wrong *write* shows up as something unrelated breaking much later,
    /// which is principle 3's opening sentence (D296).
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
    /// **The distinction the whole `Evidence` field exists for.** This changes an answer and
    /// touches no memory, so the cheap oracle is enough; every other patch here writes, and
    /// a wrong write is invisible until something unrelated breaks (D296, D299).
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

    /// **An answer goes inside the entry whose question it settles.**
    ///
    /// Appending would add a second entry for a function that already has one - two claims
    /// about the same thing, which is what `Learned::record` refuses (D358).
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

    /// **An anchor that appears twice is refused rather than guessed at.**
    ///
    /// A patch aimed at a line that might be either of two is a patch nobody can check, and
    /// landing it in the wrong entry is worse than not producing one.
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
