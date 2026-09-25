//! Per-title overrides: settings and compatibility entries, layered and merged.
//!
//! Title-specific behaviour never reaches the core. There is no `if title == …`
//! anywhere - the core reads generic named settings, and a per-title file declares
//! what a given title needs.
//!
//! # Three layers, merged per key
//!
//! [`Layer::Global`] defaults, then [`Layer::Repo`] (our shipped compatibility
//! knowledge), then [`Layer::User`]. **Per key, never wholesale.** A user file that
//! sets a resolution must not silently drop the repo's compatibility entries for that
//! title - whole-file replacement produces bug reports that cannot be falsified, and
//! is a known failure of config systems shaped like this.
//!
//! # Two kinds of key
//!
//! - **Compatibility** ([`CompatEntry`]) describes a *deviation* and carries a
//!   [`CompatKind`] and a mandatory reason. The key names the behaviour, never the
//!   title: `raytracing_enabled`, not `gta_rt_fix`. That is what lets a second title
//!   needing the same thing add a line rather than a code path.
//! - **Preference** - an ordinary setting that happens to be scoped per title.
//!
//! # Nothing is applied silently
//!
//! [`Resolved`] records which layer set every key, so a run report can show effective
//! configuration with provenance. Behaviour that came from an override being invisible
//! is the same failure mode as a stub that lies about succeeding.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

/// How the worker words a guest's deliberate exit, and the one string that identifies it.
///
/// **A coupling, named once and guarded elsewhere.** `orbistoun-core::StopReason::Exited` produces
/// this text. Neither this crate nor `orbistoun-report` can depend on that one, so the value is
/// repeated here rather than referenced, and a test in `orbistoun-worker` - the one crate that sees
/// both - asserts they are the same string. A silent drift would not break anything loudly: the
/// rung would stop being awarded and every run would fall back to `Entered`, which is precisely the
/// quiet mis-measurement principle 3 refuses.
///
/// It lives here rather than beside the ladder because [`Status`] has to recognise it too - a run
/// that flipped *and* exited is recorded as `Flipped`, so the rung alone cannot say how it ended.
pub const DELIBERATE_EXIT: &str = "the guest called exit";

/// A setting value.
///
/// Typed rather than boolean-only: booleans multiply (`tolerate_unaligned_alloc`,
/// `tolerate_tiny_alloc`, …) where a typed value generalises
/// (`direct_memory_alignment = 4096`). Fewer keys, and it reads as configuration
/// rather than a list of exceptions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    /// A toggle.
    Bool(bool),
    /// A whole number - sizes, alignments, counts, limits.
    Int(i64),
    /// A scale factor or ratio.
    Float(f64),
    /// A named mode or free-form string.
    Text(String),
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bool(v) => write!(f, "{v}"),
            Self::Int(v) => write!(f, "{v}"),
            Self::Float(v) => write!(f, "{v}"),
            Self::Text(v) => write!(f, "{v}"),
        }
    }
}

/// Why a compatibility entry exists. Each resolves differently, which is the whole
/// reason they are distinguished.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompatKind {
    /// The title genuinely does something out-of-spec that real hardware tolerates.
    /// Legitimate and permanent; there is nothing to fix.
    Quirk,
    /// *Our* implementation is wrong and this masks it. Temporary; deleted when the
    /// bug is fixed.
    Workaround,
    /// A capability we have not built. Deleted when the feature ships, and aggregates
    /// into a feature-level work list.
    Unsupported,
}

/// A compatibility entry: a value, why it is set, and which kind of debt it is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompatEntry {
    /// The value to apply.
    pub value: Value,
    /// What kind of deviation this is.
    pub kind: CompatKind,
    /// Why it is here. Mandatory by construction - an entry without a reason is how
    /// a file becomes a graveyard of unexplained exceptions.
    pub reason: String,
}

/// How far a title got, coarsely.
///
/// # Why a ladder rather than a score
///
/// A compatibility database that grades titles has to say what a grade *means*, and the
/// usual vocabulary - "playable", "in-game", "intro" - would be aspirational fiction here.
/// Every rung below is instead a phase the loader already distinguishes, so a grade is
/// **derived from a run rather than typed by a person** and cannot drift from what the
/// tool actually observed.
///
/// Coarse on purpose. Two titles that both reach [`Reach::Entered`] are separated by their
/// import and call counts, not by inventing more rungs - the fine grain is already
/// measured and would only disagree with itself if it were also graded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reach {
    /// The container did not parse. Nothing about the title is known yet.
    Rejected,
    /// Parsed, but its imports could not be resolved.
    Parsed,
    /// Linked and ready, and never entered - so nothing has been learned about the guest.
    Linked,
    /// Guest code ran. Everything interesting in this project happens above this line.
    ///
    /// **The last rung, deliberately.** Surviving to the time limit looks like it deserves
    /// one of its own, and it does not: a guest spinning on four unimplemented functions
    /// survives, and one reaching forty-seven imports before faulting does not. Ranked as a
    /// higher rung, the least informative run in the corpus sorted to the top of the table -
    /// which is how this was found (D182).
    ///
    /// Not dying is an *outcome*, not a distance. It is recorded in [`Status::outcome`],
    /// and distance within this rung is measured by imports and then calls.
    Entered,
    /// The guest ran its program to the end and left by calling `exit`.
    ///
    /// # Why this earns a rung where surviving did not
    ///
    /// D182 refused a rung for reaching the time limit because *not dying is an outcome, not a
    /// distance*. This is the other shape, and it is the same test [`Self::Flipped`] passes: a
    /// guest reaches this by **a call it made**, with a status it chose, at the end of the work
    /// it set out to do. There is no way to spin into it. It is a specific thing done, not a
    /// thing not happening.
    ///
    /// # Why it sits below a flip rather than above it
    ///
    /// Finishing is trivially reachable in a way that presenting a frame is not - a program
    /// whose first instruction is `exit(0)` reaches this rung having learned nothing. Ranked
    /// above [`Self::Flipped`] it would sort exactly that run above a title rendering frames,
    /// which is D558's failure repeated: on this corpus it put the conformance probe at the head
    /// of the frontier, above every game. A frame is the harder thing and stays the higher rung.
    ///
    /// **A guest that flips and then exits is recorded as `Flipped`**, for the same reason: the
    /// frame is the stronger claim, and the deliberate stop is still carried in
    /// [`Status::outcome`].
    Exited,
    /// The guest got a frame to the output layer: it submitted a flip and a real port took it.
    ///
    /// # Why this one earns a rung where surviving did not
    ///
    /// D182 refused a rung for reaching the time limit because *not dying is an outcome, not a
    /// distance* - a guest spinning on four unimplemented functions survives. This is the
    /// opposite shape. A flip is accepted only after the guest has opened an output, set its
    /// attributes, registered buffers and configured it, each against a real implementation;
    /// there is no way to spin into it. It is a specific thing done, not a thing not happening.
    ///
    /// **It does not mean a picture was displayed.** Nothing scans a buffer out here, and a
    /// flip completes the instant it is accepted. The claim is exactly "the guest reached the
    /// layer that would present it", which is the furthest any title in this corpus has got
    /// (D558).
    ///
    /// Distance within this rung is *still* imports and standing before frames - see
    /// [`Status::beats`], where the reason is D182's, a second time.
    Flipped,
    /// The guest put **pixels it produced** into a buffer that reached the output layer.
    ///
    /// # What separates this from a flip
    ///
    /// [`Self::Flipped`] says the guest reached the layer that would present a frame, and says
    /// so honestly: nothing scans a buffer out, and a flip completes the instant it is accepted.
    /// Six guests sit there with a hundred percent standing and **not one has produced a
    /// pixel**. COMPATIBILITY.md's prose has said so since D558 - *"a place reached and not a
    /// picture shown"* - while the table said `flipped`, and the table is what gets read.
    ///
    /// This rung is the sentence the table could not say. It is awarded only when the buffer a
    /// flip carried is read back and found to hold something the guest wrote, which no amount of
    /// reaching the interface can fake.
    ///
    /// # Nothing awards it yet, and that is the point
    ///
    /// [`crate`] does not decide rungs; `orbistoun_report::trace::status_of` does, and it has no
    /// arm for this one because there is nothing to read back: no renderer is attached to the
    /// run path at all, so no presented buffer exists to inspect. A test beside that function
    /// asserts the absence rather than leaving it to be noticed.
    ///
    /// So every guest in the corpus now ranks below the top rung, which is the accurate reading
    /// and was not previously expressible. **The scale is supposed to be able to say "not yet".**
    /// A ladder whose top rung everything has reached measures nothing about the work left.
    ///
    /// # Why this is a thing done, not a thing not happening
    ///
    /// D182's rule, which refused a rung for surviving to the time limit, is the test every rung
    /// here has to pass. Pixels pass it in the strongest form available: a buffer differing from
    /// what it held before the guest ran is a positive measurement against a known prior, and it
    /// is the framebuffer-diffing oracle this project already treats as its only cheap
    /// mechanical correctness signal. There is no way to spin into it.
    Presented,
}

impl Reach {
    /// How to name it in a report.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Rejected => "rejected",
            Self::Parsed => "parsed",
            Self::Linked => "linked",
            Self::Entered => "entered",
            Self::Exited => "exited",
            Self::Flipped => "flipped",
            Self::Presented => "presented",
        }
    }
}

/// What a title last did, as opposed to what it is configured to do.
///
/// # The other half of the same file
///
/// A title file has always described what orbistoun *sets* for a title. This is what
/// orbistoun *got*, and it lives in the same file for the reason a separate compatibility
/// list would not: they are keyed by the same title, edited in the same session, and two
/// files would immediately disagree about which one was current.
///
/// **Deliberately not merged.** [`Resolved::merge`] layers settings and compatibility
/// entries per key, which is right for configuration and meaningless for a measurement:
/// there is no sense in which a user's run "overrides" the repository's recorded one. Both
/// are facts about different runs, and comparing them is the useful operation - which is
/// what [`Status::beats`] is for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    /// How far it got.
    pub reach: Reach,
    /// How it ended, in words - the fault site, the guest's own decision, or the limit.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub outcome: String,
    /// Distinct imports the guest called.
    #[serde(default)]
    pub imports: usize,
    /// Total calls through any stub.
    #[serde(default)]
    pub calls: u64,
    /// What percentage of those calls reached an implementation rather than a placeholder.
    ///
    /// Recorded because a call count alone cannot be compared across policies, and this is
    /// the number that says how much of the result was real (D181).
    #[serde(default)]
    pub standing: u32,
    /// What unimplemented functions answered during the run.
    ///
    /// **The entry is uncomparable without it.** A result produced with stubs reporting
    /// success reaches further than an honest one and means less, so a database that
    /// recorded only the numbers would rank the dishonest run higher for ever.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub default_return: String,
    /// How many functions were handed a specific answer instead of the default.
    ///
    /// **The same argument as `default_return`, and it was the half nobody recorded.** A
    /// measured policy leaves the default at `unimplemented` and puts its answers here, so a
    /// record carrying only the default reported a propped-up run as an honest one - and the
    /// guard written to catch exactly that read only the default (D312).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub overrides: usize,
    /// How many of those rest on nothing measured.
    ///
    /// **The half that decides whether this run was honest.** An answer taken from the target
    /// is the emulator being *right*, and a run using it measures the emulator as it stands. An
    /// answer somebody guessed until the guest moved is a prop. `overrides` cannot tell them
    /// apart, so for twelve days every title with a learned fact loaded recorded an experiment
    /// and the honest slot was unreachable - which is not what "propped up" was ever meant to
    /// mean (D555, D557).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub propping: usize,
    /// Frames the guest handed to the output layer.
    ///
    /// Zero for every title that never reached [`Reach::Flipped`], and the distance within that
    /// rung once one does. Recorded rather than merely implied by the rung, because "reached its
    /// first frame" and "has been running for a thousand" are the same rung and not the same
    /// result (D558).
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub frames: u64,
    /// Distinct imports the guest called that had nothing behind them.
    ///
    /// # What [`Self::standing`] could not see
    ///
    /// `standing` is a percentage of **calls**, and calls belong to whatever the guest loops on.
    /// A day that took this count from 35 to 20 moved `standing` from 100 to 100, because 914
    /// stubbed calls out of 419,091 and 32 out of 418,464 both round to nothing. The record was
    /// blind to the most direct measure there is of how much of the interface is real (D563).
    ///
    /// Counting **functions** is stable against a hot loop, and it is the work list: exactly the
    /// number of things the guest asked for and did not get.
    ///
    /// # Why optional
    ///
    /// [`None`] means *this run did not measure it*, which every record written before D563 is.
    /// It is not `Some(0)` - a title with nothing left unanswered - and collapsing the two would
    /// let an old record claim a perfect score it never earned, then refuse every honest run
    /// that followed. See [`Self::answered`] for how an unmeasured record ranks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unanswered: Option<usize>,
    /// The wall-clock limit the run was given, in seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit_seconds: Option<u64>,
    /// The build that produced it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub build: String,
    /// The day it was measured.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub measured_on: String,
    /// Anything a person should know that the numbers do not say.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
}

impl Status {
    /// Whether the run was helped along rather than measuring the emulator as it stands.
    ///
    /// **Either half counts.** Loosening the default and answering one function by name are
    /// the same act at different scales, and a measured policy does the second while leaving
    /// the first honest - so a check on the default alone waves it straight through (D312).
    ///
    /// **What counts as the second half narrowed, and did not disappear.** It used to be
    /// `overrides > 0` - any function answered by name at all. That treats a hardware
    /// measurement and a wild guess as the same act, and they are opposites: one is the
    /// emulator being right, the other is somebody trying answers until the guest moved. It is
    /// now the count of entries resting on nothing measured, which is the question the phrase
    /// "propped up" was always asking (D557).
    ///
    /// It is strictly harder to satisfy than the old test in one direction only - a run can now
    /// be honest where it was an experiment, never the reverse - because `propping` counts
    /// regions as well as answers, which the old field never did at all.
    pub fn propped_up(&self) -> bool {
        (!self.default_return.is_empty() && self.default_return != "unimplemented")
            || self.propping > 0
    }

    /// The policy in a phrase, for the line that says why an entry is set apart.
    ///
    /// **Both halves, because either can be the one doing the propping.** A message naming
    /// only the default is how the override half went unnoticed - principle 3's rule that a
    /// message must come from the branch that determined it.
    #[must_use]
    pub fn describe_policy(&self) -> String {
        let default = if self.default_return.is_empty() {
            "unimplemented"
        } else {
            &self.default_return
        };
        // Both numbers, because they answer different questions and reporting only the first
        // is how the distinction went unnoticed for twelve days. Principle 3: a message naming
        // a cause must come from the branch that determined it (D557).
        let named = match self.overrides {
            0 => return default.to_owned(),
            1 => "1 function answered by name".to_owned(),
            n => format!("{n} functions answered by name"),
        };
        match self.propping {
            0 => format!("{default}, with {named}, all of it measured"),
            n if n == self.overrides => format!("{default}, with {named}, none of it measured"),
            n => format!("{default}, with {named}, {n} of them resting on nothing measured"),
        }
    }

    /// How many of the imports it called were answered by something real.
    ///
    /// **Zero where nothing measured it**, which is not a claim that nothing was answered - it is
    /// a record that cannot say. Ranked as low as such a record can be, so the next run of that
    /// title replaces it with a real number rather than being refused by a score it never earned
    /// (D563).
    #[must_use]
    pub fn answered(&self) -> usize {
        self.unanswered
            .map_or(0, |missing| self.imports.saturating_sub(missing))
    }

    /// Whether the guest left by calling `exit` rather than dying.
    ///
    /// Read off [`Self::outcome`] rather than [`Self::reach`], because the rung cannot say it: a
    /// run that flipped *and* exited is recorded as `Flipped`, since a frame is the stronger claim
    /// (D685).
    #[must_use]
    pub fn exited_deliberately(&self) -> bool {
        self.outcome == DELIBERATE_EXIT
    }

    /// The order results are ranked in, in **one** place.
    ///
    /// # Three copies of this had already drifted
    ///
    /// [`Self::beats`] decides what gets recorded, [`frontier`] decides the order a shim shows,
    /// and [`render_markdown`] decides the table - and each held its own copy. Neither of the
    /// latter two gained `frames` when D558 added it, so this morning's rung ranked one way in
    /// the record and another in the table nobody would have checked.
    ///
    /// `frontier`'s own documentation had already said why that is dangerous: *a table that
    /// disagreed with the thing deciding what to record would be the more convincing of the two
    /// and the wrong one*. It was right, and the fix is one function rather than three careful
    /// edits (D563).
    ///
    /// **Order, and why:** the rung first; then whether the guest left deliberately; then how much
    /// of the interface was reached; then how much of that was answered by something real; then the
    /// share of calls that were; then frames; then calls. Everything after `imports` is a quality
    /// measure and must stay below it - a guest that gets further calls more, and some of what it
    /// calls will be unimplemented, so any of these ranked higher would report going further as
    /// going backwards (D182, D558).
    ///
    /// **The exception is the ending, and it is deliberate (D686).** It sits *above* `imports`,
    /// which the paragraph above otherwise forbids, because the case it exists for is a run whose
    /// import count fell *because* it stopped correctly: the conformance payload stopped making six
    /// calls it had only been making by running past its own refused `exit`. Below `imports` the
    /// tiebreaker could never fire for that case, which is the only case it was asked for.
    ///
    /// The cost is stated rather than hidden: on this corpus every game faults and both of our own
    /// guests do not, so an ending ranked above `imports` sorts the probes above the titles. See
    /// D686 for the frontier this produces and why it was accepted anyway.
    fn ranking_key(&self) -> (Reach, bool, usize, usize, u32, u64, u64) {
        (
            self.reach,
            self.exited_deliberately(),
            self.imports,
            self.answered(),
            self.standing,
            self.frames,
            self.calls,
        )
    }

    /// Whether two results were produced under settings that can be compared at all.
    ///
    /// Only the stub policy is checked. The time limit changes how far a run gets and is
    /// worth recording, but a longer run genuinely did get further; a *looser policy* is
    /// the case where the numbers move without anything being true.
    ///
    /// **Propped-up runs compare with each other, not with honest ones.** Comparing the count
    /// rather than the fact would make two experiments incomparable for differing by one
    /// override, which is not a difference in kind (D312).
    pub fn comparable_with(&self, other: &Self) -> bool {
        self.default_return == other.default_return && self.propped_up() == other.propped_up()
    }

    /// Whether this result should replace `previous`: **not worse, and not the same run again**.
    ///
    /// [`Self::beats`] answers "is this an improvement", which is the right question for a verdict
    /// and the wrong one for a record. A run can be *equal* on every ranked field and still carry
    /// something the record should hold - most obviously how it ended. A guest that stopped
    /// deliberately where it used to fault reaches exactly as far, so `beats` is false, and the
    /// record then keeps saying the guest died for as long as nothing else changes (D687).
    ///
    /// So: comparable, **not below** the record on the ranked key, and differing from it in the
    /// key or in the outcome. An identical rerun changes neither and is not written - the record
    /// would gain nothing but a new date, and a file that churns on every run is one nobody reads
    /// diffs of.
    ///
    /// **`measured_on` is deliberately not part of "different".** It differs on every run by
    /// construction, so counting it would make every rerun worth recording and delete the rule.
    #[must_use]
    pub fn worth_recording(&self, previous: &Self) -> bool {
        if !self.comparable_with(previous) {
            return false;
        }
        let (mine, theirs) = (self.ranking_key(), previous.ranking_key());
        mine >= theirs && (mine != theirs || self.outcome != previous.outcome)
    }

    /// Whether this result is an improvement on `previous`.
    ///
    /// **Not the recording gate** - that is [`Self::worth_recording`], which also accepts a run
    /// that is equal but ended differently (D687). This answers the verdict question: did it get
    /// further. The two were one function until a record kept saying a guest died after it had
    /// stopped calling `exit`.
    ///
    /// **Refuses to claim an improvement it cannot justify.** A run under a looser stub
    /// policy reaches further by construction, so ranking on the numbers alone would let
    /// one line of configuration permanently overwrite an honestly measured entry - and
    /// the database would then carry a best-ever result that nothing can reproduce.
    ///
    /// The ladder decides first; within a rung, more distinct imports, then more of them
    /// answered by real implementations, then more calls.
    ///
    /// **`standing` sits in the middle because otherwise this cannot see the most common
    /// kind of progress there is.** Implementing a function the guest already called moves
    /// no import and no call - the guest made exactly the same calls, and got real answers
    /// to more of them. Ranking on reach and counts alone reported "better or equal" and
    /// refused to record the session's actual work, which is how this was found: by the
    /// record rejecting a run that had plainly improved (D183).
    ///
    /// Calls come last and are the weakest signal: a guest spinning on one unimplemented
    /// function accumulates them without learning anything.
    ///
    /// **Frames sit above calls and below imports, which is D182's reasoning a second time.**
    /// A frame is an achievement where a call is not, so it outranks the weakest signal. But a
    /// guest can sit in its present loop handing over the same buffer for ever, and ranking
    /// frames above imports would sort that run above one that presented three times and then
    /// got twice as far into the engine - the exact failure that cost `Entered` its rung above
    /// "survived". The rung says it presented; the imports still say how far it got (D558).
    pub fn beats(&self, previous: &Self) -> bool {
        if !self.comparable_with(previous) {
            return false;
        }
        self.ranking_key() > previous.ranking_key()
    }
}

/// Whether a count is zero, so an ordinary run writes no line about overrides.
///
/// A file a person reads should carry what is unusual, not a field of zeroes.
#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde hands `skip_serializing_if` a reference to the field"
)]
fn is_zero(n: &usize) -> bool {
    *n == 0
}

/// The same, for a count that is a `u64`.
#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde hands `skip_serializing_if` a reference to the field"
)]
fn is_zero_u64(n: &u64) -> bool {
    *n == 0
}

/// Every recorded title, furthest first.
///
/// # Why this is not in the command that prints it
///
/// It was, and that put the ranking somewhere no test could reach - which is principle 13's
/// warning, and the reason `compare` already lives below the shims (D160). The GUI needs
/// the same order, and two shims sorting the frontier separately is how they come to
/// disagree about which title is closest to running.
///
/// Sorted by the same relation [`Status::beats`] uses, so the table and the record cannot
/// rank differently - a table that disagreed with the thing deciding what to record would
/// be the more convincing of the two and the wrong one.
pub fn frontier(mut titles: Vec<(String, Status)>) -> Vec<(String, Status)> {
    titles.sort_by(|a, b| {
        b.1.ranking_key()
            .cmp(&a.1.ranking_key())
            // Ties broken by name so the order is total. Without it the table reorders
            // between runs on titles that measured identically - which is exactly what the
            // two abort-at-53 entries do - and every diff shows spurious change.
            .then_with(|| a.0.cmp(&b.0))
    });
    titles
}

/// The frontier as a table, one line per title.
///
/// Rendered here rather than in a shim so a test can hold the whole shape against real
/// records. **This is the artefact that catches ordering mistakes**: a bad ranking is
/// invisible in a unit test written by whoever chose the ranking, and obvious the moment
/// the real table is read (D184).
pub fn render_frontier(titles: &[(String, Status)]) -> String {
    use core::fmt::Write as _;

    let mut out = String::new();
    for (title, status) in titles {
        // Writing into the buffer rather than formatting and appending: the same output,
        // one allocation fewer per line, and what the lint asks for.
        let _ = writeln!(
            out,
            "{:<22} {:<10} {:>3} imports ({} answered) {:>10} calls {:>4}% standing   {}",
            title,
            status.reach.label(),
            status.imports,
            // A dash where nothing measured it. Without this the line shows a run
            // replacing one with MORE calls and gives no reason - which is what the
            // frontier snapshot showed the moment `answered` started deciding (D563).
            status
                .unanswered
                .map_or_else(|| "-".to_owned(), |_| status.answered().to_string()),
            status.calls,
            status.standing,
            status.outcome
        );
        if status.propped_up() {
            let _ = writeln!(
                out,
                "{:<22} ! measured with stubs answering {}, not comparable with the rest",
                "",
                status.describe_policy()
            );
        }
    }
    out
}

/// One row of the compatibility table: a title, the result to show for it, whether that result
/// came from the `experiment` slot (a run with overrides, recorded apart because it is less
/// comparable - D181), and a screenshot path if the guest produced one.
#[derive(Debug, Clone)]
pub struct Row {
    /// The title id.
    pub title: String,
    /// The name a person would call it, where the title says one.
    ///
    /// Beside the id rather than replacing it: the id is what every other artefact in this
    /// project keys on - traces, records, requests to other projects - and a table that showed
    /// only names could not be read against any of them (D660).
    pub name: Option<String>,
    /// The result to display - the title's `status`, or its `experiment` when it has no status.
    pub status: Status,
    /// Whether `status` above is actually the experiment slot, shown so a reader is not misled.
    pub experiment: bool,
    /// A screenshot for a guest with graphical output, as a path relative to the written file.
    pub screenshot: Option<String>,
}

/// The compatibility table as markdown, ranked closest-to-running first.
///
/// Rendered here beside [`render_frontier`] and for the same reason (D184): the markdown and the
/// terminal table are two views of one ranking, and putting both here is what stops them
/// disagreeing about which title is furthest. A guest with a screenshot gets a camera mark in the
/// table and an embedded image below it.
#[must_use]
pub fn render_markdown(rows: &[Row]) -> String {
    use core::fmt::Write as _;

    let mut ranked: Vec<&Row> = rows.iter().collect();
    ranked.sort_by(|a, b| {
        b.status
            .ranking_key()
            .cmp(&a.status.ranking_key())
            .then_with(|| a.title.cmp(&b.title))
    });

    let mut out = String::new();
    out.push_str(
        "| Title | Reach | Imports | Answered | Calls | Standing | Outcome | From | Measured |\n",
    );
    out.push_str("|---|---|--:|--:|--:|--:|---|---|---|\n");
    for r in &ranked {
        let mark = if r.screenshot.is_some() { " 📷" } else { "" };
        // Linked to the page rather than merely named, so the table is a way in to them.
        let shown = match &r.name {
            Some(name) => format!("[{}](docs/titles/{}.md)", md_cell(name), r.title),
            None => format!("[{}](docs/titles/{}.md)", r.title, r.title),
        };
        let from = if r.experiment { "experiment" } else { "run" };
        // A dash where nothing measured it, rather than a zero - the two mean opposite
        // things and a column of zeroes would read as "nothing works anywhere" (D563).
        let answered = match r.status.unanswered {
            Some(_) => r.status.answered().to_string(),
            None => "-".to_owned(),
        };
        let _ = writeln!(
            out,
            "| {}{} | {} | {} | {} | {} | {}% | {} | {} | {} |",
            shown,
            mark,
            r.status.reach.label(),
            r.status.imports,
            answered,
            r.status.calls,
            r.status.standing,
            md_cell(&r.status.outcome),
            from,
            md_cell(&r.status.measured_on),
        );
    }

    let shots: Vec<&&Row> = ranked.iter().filter(|r| r.screenshot.is_some()).collect();
    out.push_str("\n## Screenshots\n\n");
    if shots.is_empty() {
        out.push_str(concat!(
            "_None yet. A screenshot needs a captured guest framebuffer, which the video ",
            "subsystem does not surface yet; a guest that produces graphics gains an image ",
            "here once it does._\n"
        ));
    } else {
        for r in shots {
            if let Some(path) = &r.screenshot {
                let _ = writeln!(out, "### {}\n\n![{}]({})\n", r.title, r.title, path);
            }
        }
    }
    out
}

/// Where a title's captures live, relative to the page that embeds them.
///
/// One directory of images beside one directory of pages, so a page's links are the same shape
/// whether or not the image exists yet.
const CAPTURES: &str = "../../compat/screenshots";

/// The stand-in for a capture nobody has taken.
///
/// **A file rather than an omission**, because a page with a gap where an image should be reads
/// as broken, and a page that silently drops the row reads as though the question were never
/// asked. It is drawn once and shared by every title without that capture (D660).
const NO_CAPTURE: &str = "../../compat/screenshots/no-capture.svg";

/// One title's page: what it is, how far it got, and what has been captured of it.
///
/// # Why generated rather than written
///
/// The status half of a record is derived from a trace precisely so a grade cannot drift, and a
/// hand-written page per title would put that drift back one level up - stale the first time
/// somebody records a run and forgets to edit prose. So the page is regenerated from the record
/// every time, and says so at the top.
///
/// Everything missing is shown as missing. A title with no metadata, no screenshot and no
/// experiment still gets every row, each saying what is absent - a page that omits what it lacks
/// cannot be told from one that was never finished.
#[must_use]
pub fn render_title_page(row: &Row, title: &Title, notes: &str) -> String {
    use core::fmt::Write as _;

    let name = title.display(&row.title);
    let mut out = String::new();
    let _ = writeln!(out, "# {name}\n");
    let _ = writeln!(
        out,
        concat!(
            "_Generated by `orbistoun-cli compat markdown` from `compat/{}.toml`. ",
            "Do not edit by hand; re-run it._\n"
        ),
        row.title
    );

    let _ = writeln!(out, "| | |\n|---|---|");
    let _ = writeln!(out, "| Identifier | `{}` |", row.title);
    // **"ships no `param.json`" is not the same answer as "the field is missing".** A homebrew
    // payload has no metadata file at all, and four rows of "not recorded" reads as a failure to
    // read one - which sends somebody looking for a parser bug that is not there (D660).
    if title.is_empty() {
        let _ = writeln!(
            out,
            "| Metadata | _none - this guest ships no `sce_sys/param.json`_ |"
        );
    }
    for (label, value) in [
        ("Title id", title.id.as_deref()),
        ("Name", title.name.as_deref()),
        ("Content version", title.content_version.as_deref()),
        ("Master version", title.master_version.as_deref()),
    ] {
        if title.is_empty() {
            continue;
        }
        // **"not recorded" rather than a blank cell.** A blank one reads as a rendering fault and
        // sends somebody looking for a bug in this function.
        let _ = writeln!(
            out,
            "| {label} | {} |",
            value.map_or("_not recorded_".to_owned(), |v| format!("`{v}`"))
        );
    }

    let s = &row.status;
    let from = if row.experiment {
        "an experiment - a run resting on an answer nothing measured"
    } else {
        "a run of the emulator as it stands"
    };
    let _ = writeln!(out, "\n## How far it gets\n");
    let _ = writeln!(out, "| | |\n|---|--:|");
    let _ = writeln!(out, "| Reach | {} |", s.reach.label());
    let _ = writeln!(out, "| Outcome | {} |", md_cell(&s.outcome));
    let _ = writeln!(out, "| Distinct imports | {} |", s.imports);
    let _ = writeln!(
        out,
        "| Answered by an implementation | {} |",
        match s.unanswered {
            Some(_) => s.answered().to_string(),
            None => "_not measured in this run_".to_owned(),
        }
    );
    let _ = writeln!(out, "| Calls | {} |", s.calls);
    let _ = writeln!(out, "| Standing | {}% |", s.standing);
    let _ = writeln!(out, "| Frames to the output layer | {} |", s.frames);
    let _ = writeln!(out, "| Measured on | {} |", s.measured_on);
    let _ = writeln!(out, "\nThis is {from}.\n");
    if !notes.is_empty() {
        let _ = writeln!(out, "> {}\n", md_cell(notes));
    }

    let _ = writeln!(out, "## Captures\n");
    // **Both rows always, whichever exists.** A title that reaches a menu and one that has never
    // drawn a pixel should produce the same shape of page, so the difference between them is the
    // image and not the layout.
    for (what, held) in [("Menu", row.screenshot.as_deref()), ("Gameplay", None)] {
        match held {
            Some(path) => {
                let _ = writeln!(
                    out,
                    "**{what}**\n\n![{what} of {name}]({CAPTURES}/{path})\n"
                );
            }
            None => {
                let _ = writeln!(
                    out,
                    "**{what}** - not captured yet.\n\n![no capture yet]({NO_CAPTURE})\n"
                );
            }
        }
    }
    out
}

/// Escape the two characters that break a markdown table cell.
fn md_cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

/// What a title says it is.
///
/// # Why this is derived and not typed
///
/// The rest of this file already splits into what orbistoun **sets** for a title and what it
/// **got** from a run, and the second half is written from a trace rather than by hand precisely
/// so a grade cannot drift the moment somebody is optimistic. A display name typed into a record
/// would be the same hazard in a smaller way - wrong for a re-released title, missing for the next
/// one somebody adds, and unfalsifiable either way.
///
/// So it comes from `sce_sys/param.json`, which the title ships and which names itself. Nothing
/// here is guest material: an id, a name and two version strings, the same class of thing the
/// filename already is (D660).
///
/// Every field is optional because a homebrew payload has no such file, and saying so is the
/// honest answer for the two dozen of them in this directory.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Title {
    /// The title id the container declares, which should match the record's filename.
    ///
    /// Kept even though it is redundant, because a mismatch is worth seeing: a record named for
    /// one title holding another's metadata means somebody copied a file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The name a person would call it - `Earthion`, not `PPSA28061`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The content version, as the title states it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_version: Option<String>,
    /// The master version, as the title states it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub master_version: Option<String>,
}

impl Title {
    /// Whether anything is known at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// The name to show, falling back to the id and then to nothing.
    ///
    /// **Never invents one.** A title with no `param.json` is displayed by its identifier, which
    /// is what it has always been displayed by; inventing a prettier string from the filename
    /// would make a homebrew payload look like it had metadata it does not.
    #[must_use]
    pub fn display<'a>(&'a self, fallback: &'a str) -> &'a str {
        self.name.as_deref().unwrap_or(fallback)
    }
}

/// What a real console does with this title, attested from outside orbistoun.
///
/// Ground truth orbistoun cannot measure itself: somebody ran the title on hardware and said what
/// they saw. Recorded so a session that hits a wall reads it before reaching for the seductive,
/// work-stopping conclusion that the *title* is at fault. A fault in a title known to render on
/// hardware is orbistoun's gap to close, and the burden of blaming the title's own code is a
/// hardware observation of the same failure - which a guest `TODO` print is not (D708).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hardware {
    /// What it does on a real console, in plain words - `renders`, `boots to menu`, `plays`, or a
    /// specific failure. Free text, not a measured rung: this is somebody's observation, and the
    /// point of it is that it exists and says whether the title itself is sound.
    pub does: String,
    /// Who says so - `operator`, or an obSCEne / probe id.
    pub attested_by: String,
    /// When, so a stale attestation can be re-checked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on: Option<String>,
    /// Anything worth carrying - the console firmware, how it was seen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl Hardware {
    /// Whether this attestation says the title is sound on hardware (it does *something* real),
    /// rather than recording a hardware failure. A blank `does` is treated as no claim.
    #[must_use]
    pub fn is_sound_on_hardware(&self) -> bool {
        let does = self.does.trim().to_ascii_lowercase();
        !does.is_empty()
            && !does.contains("fault")
            && !does.contains("crash")
            && !does.contains("fail")
    }
}

/// One override file, as it appears on disk.
///
/// `BTreeMap` throughout so serialisation is deterministic: run reports are diffed
/// between runs, and map ordering churn would show up as spurious change.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OverrideFile {
    /// What the title says it is, read from its own `param.json`.
    ///
    /// Empty for anything that ships no such file - every homebrew payload here - and written
    /// as absent rather than as blank strings, so "not known" and "known to be empty" stay
    /// different answers (D660).
    #[serde(default, skip_serializing_if = "Title::is_empty")]
    pub title: Title,
    /// What a real console does with this title, attested from outside orbistoun (D708).
    ///
    /// Absent means nobody has said - and the run report then defaults a fault to orbistoun's gap
    /// and says the hardware status is unknown, rather than letting a session guess the title is
    /// broken.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hardware: Option<Hardware>,
    /// Compatibility entries, keyed by behaviour name.
    #[serde(default)]
    pub compat: BTreeMap<String, CompatEntry>,
    /// Ordinary settings scoped to this title.
    #[serde(default)]
    pub settings: BTreeMap<String, Value>,
    /// What the title last did, where anyone has run it.
    ///
    /// Absent means nobody has recorded a run, which is different from a run that got
    /// nowhere - [`Reach::Rejected`] says that, and says it deliberately.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<Status>,
    /// The furthest a run got while being **helped** - a loosened default, or functions
    /// answered by name.
    ///
    /// **Kept rather than refused.** This used to be turned away at the door: a propped-up
    /// run could not be compared with the honest record, so it was not written at all and a
    /// person had to pass `--force` to keep it. That made the loop need a human on every
    /// measured policy, and threw away the one number that says whether a patch is worth
    /// pursuing (D312).
    ///
    /// A separate slot rather than a flag on [`Self::status`], because they answer different
    /// questions - "how far does the emulator take this title" and "how far could it, if the
    /// thing being measured were implemented" - and a single best-ever entry cannot hold both
    /// without one silently overwriting the other.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experiment: Option<Status>,
}

impl OverrideFile {
    /// Parses TOML.
    pub fn from_toml(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }

    /// Serialises to TOML.
    pub fn to_toml(&self) -> Result<String, toml::ser::Error> {
        toml::to_string_pretty(self)
    }

    /// How to frame a fault this title just hit: whose gap it is, by the hardware ground truth
    /// (D708).
    ///
    /// Always leads with orbistoun as the default owner of a fault, because it is the incomplete
    /// party and the title is a thing that ran on a console. A `[hardware]` attestation that the
    /// title is sound makes that unambiguous; an absent one still defaults to orbistoun and says
    /// the status is unknown rather than letting a reader assume the title is broken. This is the
    /// tool half of the doctrine - the principle in `CLAUDE.md` is what stops the mistake, this
    /// puts the ground truth in front of the reader at the moment of the fault.
    #[must_use]
    pub fn fault_attribution(&self) -> Vec<String> {
        let mut lines = vec![
            "whose gap is this? orbistoun's, by default (D708): a work-in-progress HLE faulted a"
                .to_owned(),
            "  title that ships and runs on a console. Blaming the title's own code needs a hardware"
                .to_owned(),
            "  observation of the same failure - a guest TODO print or an unresolved import is not it."
                .to_owned(),
        ];
        lines.push(match &self.hardware {
            None => {
                "  hardware: unknown - nobody has attested what a console does with this title."
                    .to_owned()
            }
            Some(hw) if hw.is_sound_on_hardware() => format!(
                "  hardware: {} on a console ({}{}) - so this fault is orbistoun's to close.",
                hw.does,
                hw.attested_by,
                hw.on
                    .as_deref()
                    .map(|d| format!(", {d}"))
                    .unwrap_or_default(),
            ),
            Some(hw) => format!(
                "  hardware: a console also {} ({}) - the title's own code may be in play here.",
                hw.does, hw.attested_by,
            ),
        });
        lines
    }
}

/// Which layer a value came from. Ordering is precedence: later wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Layer {
    /// Built-in defaults.
    Global,
    /// Compatibility knowledge shipped with orbistoun.
    Repo,
    /// The user's own file, in their data directory.
    User,
}

impl fmt::Display for Layer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Global => "global",
            Self::Repo => "repo",
            Self::User => "user",
        })
    }
}

/// One effective value, with where it came from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolvedValue {
    /// The value in force.
    pub value: Value,
    /// The layer that set it.
    pub layer: Layer,
    /// Present when this key came from a compatibility entry rather than a plain
    /// setting.
    pub compat: Option<CompatMeta>,
}

/// The compatibility metadata of a resolved value, without repeating the value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompatMeta {
    /// What kind of deviation.
    pub kind: CompatKind,
    /// Why it is set.
    pub reason: String,
}

/// The effective configuration for one title, with per-key provenance.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Resolved {
    /// Effective values, keyed by setting name.
    pub values: BTreeMap<String, ResolvedValue>,
}

impl Resolved {
    /// Merges layers in precedence order, per key.
    ///
    /// Each `(Layer, OverrideFile)` is applied over the accumulated result; a later
    /// layer replaces only the keys it names. Within one file, compatibility entries
    /// and settings share a namespace, and a compat entry wins if a file somehow
    /// declares both - a deviation with a stated reason is more informative than a
    /// bare value.
    pub fn merge(layers: &[(Layer, OverrideFile)]) -> Self {
        let mut values: BTreeMap<String, ResolvedValue> = BTreeMap::new();
        for (layer, file) in layers {
            for (key, value) in &file.settings {
                values.insert(
                    key.clone(),
                    ResolvedValue {
                        value: value.clone(),
                        layer: *layer,
                        compat: None,
                    },
                );
            }
            for (key, entry) in &file.compat {
                values.insert(
                    key.clone(),
                    ResolvedValue {
                        value: entry.value.clone(),
                        layer: *layer,
                        compat: Some(CompatMeta {
                            kind: entry.kind,
                            reason: entry.reason.clone(),
                        }),
                    },
                );
            }
        }
        Self { values }
    }

    /// The effective value for `key`, if any layer set it.
    pub fn get(&self, key: &str) -> Option<&ResolvedValue> {
        self.values.get(key)
    }

    /// Convenience for the common boolean case.
    ///
    /// Returns `None` if unset *or* set to a non-boolean, rather than coercing - a
    /// type confusion in a config file should surface, not be papered over.
    pub fn bool(&self, key: &str) -> Option<bool> {
        match self.get(key).map(|r| &r.value) {
            Some(Value::Bool(v)) => Some(*v),
            _ => None,
        }
    }

    /// Convenience for the common integer case. Same non-coercing rule as [`Self::bool`].
    pub fn int(&self, key: &str) -> Option<i64> {
        match self.get(key).map(|r| &r.value) {
            Some(Value::Int(v)) => Some(*v),
            _ => None,
        }
    }

    /// Every entry of a given compatibility kind.
    ///
    /// `workaround` answers "what are we papering over"; `unsupported` aggregates into
    /// a feature-level work list across the corpus.
    pub fn of_kind(&self, kind: CompatKind) -> Vec<(&str, &ResolvedValue)> {
        self.values
            .iter()
            .filter(|(_, v)| v.compat.as_ref().is_some_and(|c| c.kind == kind))
            .map(|(k, v)| (k.as_str(), v))
            .collect()
    }

    /// Whether anything at all is in force. An empty resolution means the title runs
    /// on stock behaviour.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// How many keys are in force.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// A title's effective settings for a run: the shipped record's `[settings]` (the repository
    /// layer, carried in at build time), then the user's own `<overrides_dir>/<title>.toml` over it.
    ///
    /// A user file that does not parse contributes nothing rather than failing the run - the run
    /// still has the shipped layer, and the user file is theirs to fix.
    #[must_use]
    pub fn for_run(title: &str, overrides_dir: &std::path::Path) -> Self {
        let mut layers = Vec::new();
        if let Some(file) = shipped(title) {
            layers.push((Layer::Repo, file));
        }
        if let Ok(text) = std::fs::read_to_string(overrides_dir.join(format!("{title}.toml")))
            && let Ok(file) = OverrideFile::from_toml(&text)
        {
            layers.push((Layer::User, file));
        }
        Self::merge(&layers)
    }

    /// Convenience for a named mode. Same non-coercing rule as [`Self::bool`].
    pub fn text(&self, key: &str) -> Option<&str> {
        match self.get(key).map(|r| &r.value) {
            Some(Value::Text(v)) => Some(v),
            _ => None,
        }
    }
}

mod shipped_records {
    include!(concat!(env!("OUT_DIR"), "/shipped.rs"));
}

/// The shipped record's `[settings]` for `title`, as built into this binary.
#[must_use]
pub fn shipped(title: &str) -> Option<OverrideFile> {
    shipped_records::SHIPPED
        .iter()
        .find(|(name, _)| *name == title)
        .and_then(|(_, text)| OverrideFile::from_toml(text).ok())
}

/// The setting naming which filesystem a title sees: absent is its own sandbox, and
/// [`FILESYSTEM_VIEW_SYSTEM`] is the console's whole tree, as a system application sees it.
pub const FILESYSTEM_VIEW: &str = "filesystem_view";

/// [`FILESYSTEM_VIEW`]'s value for the system view.
pub const FILESYSTEM_VIEW_SYSTEM: &str = "system";

#[cfg(test)]
mod tests {
    use super::{
        CompatEntry, CompatKind, DELIBERATE_EXIT, Hardware, Layer, OverrideFile, Reach, Resolved,
        Row, Status, Value, frontier, render_markdown,
    };
    use std::collections::BTreeMap;

    /// **A fault in a title known-good on hardware is framed as orbistoun's, and a title with no
    /// attestation still defaults to orbistoun rather than to the title's fault (D708).**
    ///
    /// The whole point of the mechanism: it must never let a reader conclude the title is broken
    /// off an absent attestation. The negative case - unknown hardware - is the one that matters,
    /// so it is asserted, not just the happy path.
    #[test]
    fn fault_attribution_defaults_to_orbistoun_and_reads_the_hardware_ground_truth() {
        let text = |file: &OverrideFile| file.fault_attribution().join("\n");

        // Sound on hardware: unambiguously orbistoun's to close.
        let sound = OverrideFile {
            hardware: Some(Hardware {
                does: "renders".to_owned(),
                attested_by: "operator".to_owned(),
                on: Some("2026-09-19".to_owned()),
                note: None,
            }),
            ..OverrideFile::default()
        };
        let said = text(&sound);
        assert!(said.contains("orbistoun's"), "leads with orbistoun: {said}");
        assert!(
            said.contains("renders on a console") && said.contains("orbistoun's to close"),
            "a sound attestation makes it orbistoun's: {said}"
        );

        // No attestation: still orbistoun by default, and says the status is unknown - never
        // "the title is broken".
        let unknown = OverrideFile::default();
        let said = text(&unknown);
        assert!(
            said.contains("orbistoun's, by default"),
            "unknown still defaults to orbistoun: {said}"
        );
        assert!(said.contains("hardware: unknown"), "and says so: {said}");
        assert!(
            !said.to_ascii_lowercase().contains("title is broken")
                && !said.to_ascii_lowercase().contains("debug build"),
            "must not suggest the title is at fault: {said}"
        );

        // A hardware *failure* is the only case that opens the title's own code to suspicion.
        let broken = Hardware {
            does: "faults at boot".to_owned(),
            attested_by: "operator".to_owned(),
            on: None,
            note: None,
        };
        assert!(
            !broken.is_sound_on_hardware(),
            "a hardware fault is not soundness"
        );
        assert!(
            Hardware {
                does: "renders".to_owned(),
                attested_by: "operator".to_owned(),
                on: None,
                note: None,
            }
            .is_sound_on_hardware(),
            "rendering is soundness"
        );
    }

    fn file(
        settings: &[(&str, Value)],
        compat: &[(&str, Value, CompatKind, &str)],
    ) -> OverrideFile {
        let mut f = OverrideFile::default();
        for (k, v) in settings {
            f.settings.insert((*k).to_owned(), v.clone());
        }
        for (k, v, kind, reason) in compat {
            f.compat.insert(
                (*k).to_owned(),
                CompatEntry {
                    value: v.clone(),
                    kind: *kind,
                    reason: (*reason).to_owned(),
                },
            );
        }
        f
    }

    #[test]
    fn later_layers_win_per_key() {
        let global = file(&[("resolution_scale", Value::Int(1))], &[]);
        let user = file(&[("resolution_scale", Value::Int(2))], &[]);
        let r = Resolved::merge(&[(Layer::Global, global), (Layer::User, user)]);
        assert_eq!(r.int("resolution_scale"), Some(2));
        assert_eq!(r.get("resolution_scale").expect("set").layer, Layer::User);
    }

    /// The failure this whole design exists to prevent.
    #[test]
    fn a_user_setting_does_not_drop_repo_compatibility_entries() {
        let repo = file(
            &[],
            &[(
                "raytracing_enabled",
                Value::Bool(false),
                CompatKind::Unsupported,
                "no RT pipeline yet",
            )],
        );
        let user = file(&[("resolution_scale", Value::Int(2))], &[]);

        let r = Resolved::merge(&[(Layer::Repo, repo), (Layer::User, user)]);

        assert_eq!(r.int("resolution_scale"), Some(2), "user setting applied");
        assert_eq!(
            r.bool("raytracing_enabled"),
            Some(false),
            "repo compatibility entry must survive a user file that never mentions it"
        );
        assert_eq!(r.get("raytracing_enabled").expect("set").layer, Layer::Repo);
    }

    #[test]
    fn a_user_may_deliberately_override_a_compatibility_entry_and_it_is_visible() {
        // Allowed - but provenance shows it, so "you overrode the compat setting" is
        // answerable from the run report rather than a mystery.
        let repo = file(
            &[],
            &[(
                "raytracing_enabled",
                Value::Bool(false),
                CompatKind::Unsupported,
                "no RT pipeline yet",
            )],
        );
        let user = file(&[("raytracing_enabled", Value::Bool(true))], &[]);
        let r = Resolved::merge(&[(Layer::Repo, repo), (Layer::User, user)]);

        assert_eq!(r.bool("raytracing_enabled"), Some(true));
        let v = r.get("raytracing_enabled").expect("set");
        assert_eq!(v.layer, Layer::User);
        assert!(
            v.compat.is_none(),
            "a plain user setting is not a compat entry"
        );
    }

    #[test]
    fn provenance_is_recorded_for_every_key() {
        let r = Resolved::merge(&[
            (Layer::Global, file(&[("a", Value::Int(1))], &[])),
            (Layer::Repo, file(&[("b", Value::Int(2))], &[])),
            (Layer::User, file(&[("c", Value::Int(3))], &[])),
        ]);
        assert_eq!(r.get("a").expect("a").layer, Layer::Global);
        assert_eq!(r.get("b").expect("b").layer, Layer::Repo);
        assert_eq!(r.get("c").expect("c").layer, Layer::User);
        assert_eq!(r.len(), 3);
    }

    #[test]
    fn kinds_are_queryable_for_the_what_are_we_papering_over_report() {
        let repo = file(
            &[],
            &[
                (
                    "raytracing_enabled",
                    Value::Bool(false),
                    CompatKind::Unsupported,
                    "no RT pipeline",
                ),
                (
                    "direct_memory_alignment",
                    Value::Int(4096),
                    CompatKind::Workaround,
                    "our allocator rejects 16K alignment; bug #1",
                ),
                (
                    "tolerates_null_handle",
                    Value::Bool(true),
                    CompatKind::Quirk,
                    "title passes a null handle on purpose",
                ),
            ],
        );
        let r = Resolved::merge(&[(Layer::Repo, repo)]);

        assert_eq!(r.of_kind(CompatKind::Workaround).len(), 1);
        assert_eq!(r.of_kind(CompatKind::Unsupported).len(), 1);
        assert_eq!(r.of_kind(CompatKind::Quirk).len(), 1);
        assert_eq!(
            r.of_kind(CompatKind::Workaround)[0].0,
            "direct_memory_alignment"
        );
    }

    #[test]
    fn typed_values_do_not_coerce() {
        // A type confusion in a config file should surface, not be silently accepted.
        let r = Resolved::merge(&[(Layer::User, file(&[("x", Value::Int(1))], &[]))]);
        assert_eq!(r.int("x"), Some(1));
        assert_eq!(r.bool("x"), None, "an int is not a bool");
    }

    #[test]
    fn the_worked_example_from_the_decision_log_round_trips() {
        let toml = r#"
[compat.raytracing_enabled]
value = false
kind = "unsupported"
reason = "no RT pipeline yet; title proceeds without it"

[settings]
resolution_scale = 2
"#;
        let f = OverrideFile::from_toml(toml).expect("parses");
        assert_eq!(f.compat.len(), 1);
        assert_eq!(f.settings.len(), 1);

        let entry = &f.compat["raytracing_enabled"];
        assert_eq!(entry.kind, CompatKind::Unsupported);
        assert_eq!(entry.value, Value::Bool(false));
        assert!(!entry.reason.is_empty());

        let round = OverrideFile::from_toml(&f.to_toml().expect("serialises")).expect("reparses");
        assert_eq!(round, f);
    }

    #[test]
    fn a_compat_entry_without_a_reason_is_rejected() {
        // Mandatory by construction: an entry with no reason is how the file becomes
        // a graveyard of unexplained exceptions.
        let toml = r#"
[compat.raytracing_enabled]
value = false
kind = "unsupported"
"#;
        assert!(
            OverrideFile::from_toml(toml).is_err(),
            "reason must be mandatory"
        );
    }

    #[test]
    fn an_unknown_compat_kind_is_rejected_rather_than_defaulted() {
        let toml = r#"
[compat.x]
value = true
kind = "probably_fine"
reason = "..."
"#;
        assert!(OverrideFile::from_toml(toml).is_err());
    }

    #[test]
    fn an_empty_file_parses_to_nothing_in_force() {
        let f = OverrideFile::from_toml("").expect("empty is valid");
        let r = Resolved::merge(&[(Layer::Repo, f)]);
        assert!(r.is_empty(), "no overrides means stock behaviour");
    }

    #[test]
    fn ordering_is_deterministic_for_diffing() {
        // Run reports are diffed between runs; map ordering churn would read as
        // spurious change and pollute the signal the agent loop depends on.
        let mut settings = BTreeMap::new();
        for k in ["zebra", "alpha", "mike"] {
            settings.insert(k.to_owned(), Value::Int(1));
        }
        let f = OverrideFile {
            title: super::Title::default(),
            hardware: None,
            compat: BTreeMap::new(),
            settings,
            status: None,
            experiment: None,
        };
        let r = Resolved::merge(&[(Layer::Repo, f)]);
        let keys: Vec<_> = r.values.keys().map(String::as_str).collect();
        assert_eq!(keys, ["alpha", "mike", "zebra"]);
    }

    /// A plain honest result at a given rung.
    fn status(reach: Reach, imports: usize, calls: u64) -> Status {
        Status {
            reach,
            outcome: "image+0x1000".to_owned(),
            imports,
            calls,
            standing: 85,
            default_return: "unimplemented".to_owned(),
            overrides: 0,
            propping: 0,
            frames: 0,
            unanswered: None,
            limit_seconds: Some(20),
            build: "0.1.0".to_owned(),
            measured_on: "2026-08-21".to_owned(),
            notes: String::new(),
        }
    }

    #[test]
    fn the_markdown_table_ranks_furthest_first_and_marks_screenshots() {
        let rows = vec![
            Row {
                name: None,
                title: "near".to_owned(),
                status: status(Reach::Linked, 0, 0),
                experiment: true,
                screenshot: None,
            },
            Row {
                name: None,
                title: "far".to_owned(),
                status: status(Reach::Entered, 100, 5000),
                experiment: false,
                screenshot: Some("screenshots/far.png".to_owned()),
            },
        ];
        let md = render_markdown(&rows);
        assert!(md.contains("| Title | Reach |"), "has a header row");
        // The further guest (entered, 100 imports) ranks above the linked one, despite input order.
        assert!(
            md.find("far").unwrap() < md.find("near").unwrap(),
            "further title must come first"
        );
        // The camera follows the link now that the title cell is one, so the assertion names the
        // whole cell rather than the bare title - a substring of "far" alone would also match
        // the link target and pass whatever the mark did.
        assert!(
            md.contains("[far](docs/titles/far.md) 📷"),
            "the guest with a screenshot is marked, beside its link"
        );
        assert!(
            md.contains("[near](docs/titles/near.md) |"),
            "a guest without one is linked and unmarked"
        );
        assert!(md.contains("experiment"), "the experiment slot is labelled");
        assert!(
            md.contains("![far](screenshots/far.png)"),
            "the screenshot is embedded"
        );
    }

    #[test]
    fn the_markdown_says_so_when_there_are_no_screenshots() {
        let rows = vec![Row {
            name: None,
            title: "t".to_owned(),
            status: status(Reach::Entered, 1, 1),
            experiment: false,
            screenshot: None,
        }];
        let md = render_markdown(&rows);
        assert!(md.contains("## Screenshots"));
        assert!(md.contains("None yet"));
    }

    #[test]
    fn a_looser_policy_can_never_beat_an_honest_record() {
        // **The reason `beats` exists at all.** One line of configuration makes a run
        // reach further than the emulator can actually take it. Ranking on the numbers
        // alone would let that overwrite an honestly measured entry permanently, and the
        // database would then carry a best-ever nobody can reproduce.
        let honest = status(Reach::Entered, 47, 933);
        let inflated = Status {
            default_return: "ok".to_owned(),
            ..status(Reach::Entered, 480, 90_000)
        };

        assert!(
            !inflated.beats(&honest),
            "a much bigger number under a looser policy is still not an improvement"
        );
        assert!(!inflated.comparable_with(&honest));
        assert!(inflated.propped_up());
    }

    #[test]
    fn an_honest_result_cannot_beat_an_inflated_record_either() {
        // Symmetry matters: the refusal is "these are not comparable", not "the bigger
        // number wins". Otherwise a contaminated entry could be displaced only by another
        // contaminated one, and the file would never recover.
        let inflated = Status {
            default_return: "ok".to_owned(),
            ..status(Reach::Entered, 480, 90_000)
        };
        assert!(!status(Reach::Entered, 47, 933).beats(&inflated));
    }

    #[test]
    fn the_ladder_outranks_the_counts() {
        // A title that got linked and never entered has told us nothing about the guest,
        // however many imports were resolved statically. Reach decides first.
        assert!(status(Reach::Entered, 1, 1).beats(&status(Reach::Linked, 500, 0)));
    }

    #[test]
    fn within_a_rung_imports_outrank_calls() {
        // Calls are the weakest signal: a guest spinning on one unimplemented function
        // accumulates millions of them without learning anything. Distinct imports is the
        // number that says how much of the interface was actually exercised.
        let spinning = status(Reach::Entered, 12, 466_000_000);
        let broader = status(Reach::Entered, 47, 933);
        assert!(broader.beats(&spinning));
        assert!(!spinning.beats(&broader));
    }

    #[test]
    fn surviving_the_time_limit_is_not_a_higher_rung_than_faulting() {
        // Found by populating the record and reading the table: a title spinning on four
        // unimplemented functions for ninety-one million calls sorted above one that
        // reached forty-seven imports and faulted. Not dying is an outcome, not a distance.
        let spinning = Status {
            outcome: "ran to the time limit".to_owned(),
            ..status(Reach::Entered, 4, 91_455_278)
        };
        let informative = status(Reach::Entered, 47, 933);
        assert!(informative.beats(&spinning));
        assert!(!spinning.beats(&informative));
    }

    #[test]
    fn implementing_something_the_guest_already_called_counts_as_progress() {
        // **The most common kind of progress in this project, and the ranking could not
        // see it.** Implementing a function the guest was already calling moves no import
        // and no call - the guest makes exactly the same calls and gets real answers to
        // more of them. Found by the record refusing to accept a run that had plainly
        // improved: seventy-six calls moved from placeholder to implementation and every
        // number `beats` looked at was identical.
        let before = Status {
            standing: 85,
            ..status(Reach::Entered, 47, 933)
        };
        let after = Status {
            standing: 93,
            ..status(Reach::Entered, 47, 933)
        };
        assert!(after.beats(&before));
        assert!(!before.beats(&after));
    }

    #[test]
    fn breadth_still_outranks_quality() {
        // A run reaching far less of the interface is not better for having implemented
        // all of the little it touched. Standing breaks ties within a breadth, rather
        // than substituting for it.
        let narrow_and_clean = Status {
            standing: 100,
            ..status(Reach::Entered, 13, 131)
        };
        let broad_and_rough = Status {
            standing: 40,
            ..status(Reach::Entered, 47, 933)
        };
        assert!(broad_and_rough.beats(&narrow_and_clean));
    }

    #[test]
    fn an_identical_rerun_is_not_an_improvement() {
        // Otherwise every run rewrites the record with the same numbers and a new date,
        // and the file's history stops meaning anything.
        let now = status(Reach::Entered, 47, 933);
        assert!(!now.beats(&status(Reach::Entered, 47, 933)));
    }

    /// A run that reaches exactly as far but **ended somewhere else** is worth recording.
    ///
    /// The guest dies at a different address having got precisely as far. Nothing is ranked
    /// differently, so `beats` is false - and the record would otherwise name a fault site the
    /// guest no longer reaches, for as long as nothing else changed (D687).
    #[test]
    fn an_equal_run_that_ended_somewhere_else_is_recorded() {
        let mut was = status(Reach::Entered, 187, 4914);
        was.outcome = "0x5e2d".to_owned();
        let mut now = was.clone();
        now.outcome = "image+0x3f258".to_owned();

        assert!(!now.beats(&was), "it did not get further");
        assert!(
            now.worth_recording(&was),
            "but the record should name where it dies now"
        );
        assert!(was.worth_recording(&now), "and the same in reverse");
    }

    /// **The same run again writes nothing.** A record that churns on every rerun is one nobody
    /// reads diffs of, and `measured_on` differs by construction so it cannot count as a change.
    #[test]
    fn an_identical_rerun_is_not_worth_recording() {
        let first = status(Reach::Entered, 187, 4914);
        let mut again = first.clone();
        again.measured_on = "2099-01-01".to_owned();
        assert!(!again.worth_recording(&first));
    }

    /// A run that reached less does not overwrite one that reached more, however it ended.
    #[test]
    fn a_shorter_run_is_not_worth_recording_even_if_it_exited() {
        let far = status(Reach::Flipped, 223, 432_211);
        let mut short = status(Reach::Exited, 187, 4914);
        short.outcome = DELIBERATE_EXIT.to_owned();
        assert!(!short.worth_recording(&far), "a lower rung is still worse");
    }

    #[test]
    fn a_measurement_is_not_layered_the_way_configuration_is() {
        // Settings merge per key across layers; a measurement must not. There is no sense
        // in which a user's run "overrides" the repository's recorded one - they are facts
        // about two different runs, and merging them would silently discard one.
        let repo = OverrideFile {
            status: Some(status(Reach::Entered, 99, 5000)),
            ..OverrideFile::default()
        };
        let mut user = OverrideFile {
            status: Some(status(Reach::Entered, 1, 1)),
            ..OverrideFile::default()
        };
        user.settings
            .insert("direct_memory_alignment".to_owned(), Value::Int(4096));

        let resolved = Resolved::merge(&[(Layer::Repo, repo), (Layer::User, user)]);
        assert_eq!(resolved.len(), 1, "only the setting resolves");
        assert!(resolved.get("status").is_none(), "status is not a key");
    }

    #[test]
    fn a_record_round_trips_through_toml_with_its_settings() {
        // The two halves share a file, so a writer that dropped one on save would be the
        // whole reason not to share the file. Held here.
        let mut file = OverrideFile {
            status: Some(status(Reach::Entered, 47, 933)),
            ..OverrideFile::default()
        };
        file.compat.insert(
            "direct_memory_alignment".to_owned(),
            CompatEntry {
                value: Value::Int(4096),
                kind: CompatKind::Workaround,
                reason: "our allocator over-aligns; remove when fixed".to_owned(),
            },
        );

        let text = file.to_toml().expect("render");
        let back = OverrideFile::from_toml(&text).expect("parse");
        assert_eq!(back, file);
    }

    /// **A measured policy props a run up without touching the default.**
    ///
    /// `Learned::policy()` deliberately leaves `default_return` at `unimplemented` and puts
    /// its answers in per-function overrides, so a check on the default alone waved it
    /// straight through - and the entry would have been ranked against honestly measured
    /// ones for ever. The count was already recorded; nothing read it (D312).
    #[test]
    fn a_run_helped_by_named_overrides_is_not_an_honest_measurement() {
        let honest = status(Reach::Entered, 23, 222);
        let helped = Status {
            overrides: 1,
            propping: 1,
            ..status(Reach::Entered, 40, 90_000)
        };

        assert_eq!(
            helped.default_return, "unimplemented",
            "the default is untouched - that is the whole shape of the hole"
        );
        assert!(helped.propped_up(), "and it was still helped along");
        assert!(!helped.comparable_with(&honest));
        assert!(
            !helped.beats(&honest),
            "a bigger number bought by answering a function is not an improvement"
        );
    }

    /// **The narrowing does not let the old case through.**
    ///
    /// [`Status::propped_up`] used to fire on `overrides > 0` and now fires on `propping > 0`,
    /// which is a strictly smaller set - so the thing worth asserting is not that the new rule
    /// works but that it still rejects everything the old one did. A run whose answers rest on
    /// nothing measured is the whole of what D312 was written to catch, and it is caught.
    ///
    /// # What this cannot assert
    ///
    /// That the two rules agree on every input, because they deliberately do not: the case
    /// immediately below is one the old rule rejected and this one accepts, on purpose. What is
    /// pinned here is the direction - a run can move from experiment to honest by being
    /// *measured*, and never by being counted differently.
    #[test]
    fn a_guessed_override_still_props_a_run_up_exactly_as_it_used_to() {
        for guessed in 1..5_usize {
            let helped = Status {
                overrides: guessed,
                propping: guessed,
                ..status(Reach::Entered, 40, 90_000)
            };
            assert!(
                helped.propped_up(),
                "{guessed} answers resting on nothing measured, and the run read as honest"
            );
        }
        // And the region-only case the old field could not see at all: no answers, one write
        // into guest memory behind a byte count nothing measured.
        let writes_only = Status {
            overrides: 1,
            propping: 1,
            ..status(Reach::Entered, 40, 90_000)
        };
        assert!(
            writes_only.propped_up(),
            "a policy that writes guest memory and answers nothing is not an honest run"
        );
    }

    /// **A measured answer is the emulator being right, and does not prop a run up.**
    ///
    /// The change D557 made, and the reason it is not a loosening: `propping` counts entries
    /// resting on nothing measured, so an answer taken from the target leaves it at zero. A run
    /// using one measures the emulator as it stands, which is exactly what the honest slot is
    /// for - and for twelve days no title with a learned fact loaded could reach it (D555).
    ///
    /// # What this cannot assert
    ///
    /// **That the answer is right**, only that its provenance says somebody measured it. The
    /// grading is [`orbistoun_hle::knowledge::Oracle::is_evidence`]'s to make, and a
    /// mislabelled entry is indistinguishable from a correct one here by construction - which
    /// is why the label is set from the measurement rather than by hand.
    #[test]
    fn a_measured_answer_does_not_prop_a_run_up() {
        let honest = status(Reach::Entered, 23, 222);
        let measured = Status {
            overrides: 7,
            propping: 0,
            frames: 0,
            unanswered: None,
            ..status(Reach::Entered, 40, 90_000)
        };

        assert!(
            !measured.propped_up(),
            "seven answers, every one of them measured, and the run still read as an experiment"
        );
        assert!(
            measured.comparable_with(&honest),
            "so it compares with an honest run rather than being set apart from it"
        );
        assert!(
            measured.beats(&honest),
            "and a better result under a measured policy is an improvement worth recording"
        );
    }

    /// **The three things that rank results agree, because there is only one of them.**
    ///
    /// `beats` decides what is recorded, `frontier` decides what a shim shows, and
    /// `render_markdown` decides the table. They held three copies of one ordering and two had
    /// already drifted - neither gained `frames` when D558 added it this morning. This asserts
    /// they agree by construction rather than by care (D563).
    ///
    /// # What this cannot assert
    ///
    /// That the ordering is *right*. It asserts only that a disagreement between the record and
    /// the table is impossible, which is the failure `frontier`'s own documentation warned about
    /// and the one nobody would have noticed.
    #[test]
    fn the_record_the_frontier_and_the_table_cannot_rank_differently() {
        let entries = vec![
            (
                "far-but-unanswered".to_owned(),
                Status {
                    unanswered: Some(60),
                    ..status(Reach::Flipped, 215, 431_448)
                },
            ),
            (
                "near-and-answered".to_owned(),
                Status {
                    unanswered: Some(0),
                    ..status(Reach::Flipped, 197, 418_464)
                },
            ),
            (
                "presented-more".to_owned(),
                Status {
                    frames: 90,
                    unanswered: Some(0),
                    ..status(Reach::Flipped, 197, 418_464)
                },
            ),
            (
                "only-entered".to_owned(),
                Status {
                    unanswered: Some(1),
                    ..status(Reach::Entered, 400, 900_000)
                },
            ),
        ];

        let ranked = frontier(entries.clone());
        // Every adjacent pair must agree with `beats`, which is the relation the record uses.
        for pair in ranked.windows(2) {
            let (upper, lower) = (&pair[0], &pair[1]);
            assert!(
                !lower.1.beats(&upper.1),
                "{} sorted below {} but beats it",
                lower.0,
                upper.0
            );
        }

        // And the table renders in that same order.
        let rows: Vec<Row> = ranked
            .iter()
            .map(|(title, status)| Row {
                name: None,
                title: title.clone(),
                status: status.clone(),
                experiment: false,
                screenshot: None,
            })
            .collect();
        let table = render_markdown(&rows);
        let order: Vec<usize> = ranked
            .iter()
            .map(|(title, _)| {
                table
                    .find(title.as_str())
                    .expect("every title is in the table")
            })
            .collect();
        assert!(
            order.windows(2).all(|w| w[0] < w[1]),
            "the table's order differs from the frontier's"
        );
    }

    /// **Answering more of the same interface is an improvement the record can see.**
    ///
    /// The gap D563 closes. Implementing a function the guest already called moves no reach, no
    /// import and no call - it moves only how many of those calls were real - and `standing`, an
    /// integer percentage of calls, could not see it: 914 stubbed of 419,091 and 32 of 418,464
    /// both round to 100.
    #[test]
    fn answering_more_of_the_same_imports_is_an_improvement() {
        let before = Status {
            unanswered: Some(35),
            ..status(Reach::Flipped, 197, 419_091)
        };
        let after = Status {
            unanswered: Some(20),
            // Fewer calls, and the same rounded standing - so nothing else in the tuple can be
            // what carries this.
            ..status(Reach::Flipped, 197, 418_464)
        };
        assert_eq!(
            before.standing, after.standing,
            "standing cannot tell them apart"
        );
        assert!(
            after.beats(&before),
            "fifteen more functions answered is progress"
        );
        assert!(!before.beats(&after));
    }

    /// **Answering more must never outrank reaching more.**
    ///
    /// The trap this ordering could fall into, and it is D182's shape a third time: a guest that
    /// goes further calls *more* imports, and some of those will be unimplemented - so a run can
    /// legitimately get further and have **more** unanswered than before. Ranked above `imports`,
    /// that would report going further as going backwards.
    ///
    /// # What this cannot assert
    ///
    /// That the position between `imports` and `standing` is the right one, only that it is below
    /// `imports`. Whether answering ten functions is worth more than one percent of standing is a
    /// judgement nothing here measures.
    #[test]
    fn answering_more_does_not_outrank_reaching_more() {
        let narrow = Status {
            unanswered: Some(0),
            ..status(Reach::Flipped, 197, 418_464)
        };
        let further = Status {
            unanswered: Some(33),
            ..status(Reach::Flipped, 215, 431_448)
        };

        assert!(
            further.beats(&narrow),
            "reaching 18 more imports is progress even though 33 of them are unanswered"
        );
        assert!(!narrow.beats(&further));
    }

    /// **A record that never measured this cannot claim a perfect score.**
    ///
    /// Every entry written before D563 has no value, and `None` must not read as *nothing left
    /// unanswered* - that would let a stale record outrank every honest run that followed it, and
    /// the title would never record another result. The same trap `propping` sprang this morning
    /// when 33 records deserialised as honest (D557).
    #[test]
    fn an_unmeasured_record_does_not_outrank_a_measured_one() {
        let old = Status {
            unanswered: None,
            ..status(Reach::Flipped, 197, 418_464)
        };
        let measured = Status {
            // Deliberately poor: almost nothing answered, and it still must win, because it
            // measured something and the other did not.
            unanswered: Some(196),
            ..status(Reach::Flipped, 197, 418_464)
        };

        assert_eq!(old.answered(), 0, "a record that cannot say claims nothing");
        assert!(
            measured.beats(&old),
            "one answered function beats an unmeasured record"
        );
        assert!(!old.beats(&measured));
    }

    /// **A guest that presented ranks above one that only entered.**
    ///
    /// The rung itself. A flip is accepted only after an output has been opened, its
    /// attributes set, its buffers registered and its mode configured - so unlike surviving to
    /// the time limit, it cannot be arrived at by doing nothing (D182, D558).
    #[test]
    fn presenting_a_frame_outranks_merely_entering() {
        let entered = status(Reach::Entered, 400, 900_000);
        let presented = Status {
            frames: 1,
            ..status(Reach::Flipped, 12, 40)
        };

        assert!(
            presented.beats(&entered),
            concat!(
                "a guest that got a frame to the output layer with a twelfth of the ",
                "imports is still further along than one that never reached it"
            )
        );
        assert!(!entered.beats(&presented));
    }

    /// **Frames do not outrank imports, which is D182's mistake refused a second time.**
    ///
    /// A guest can sit in its present loop handing over the same buffer for ever. If frames
    /// ranked above imports, that run would sort above one that presented three times and then
    /// got twice as far into the engine - which is exactly how `Entered` came to be the last
    /// rung: the least informative run in the corpus sorted to the top of the table.
    ///
    /// # What this cannot assert
    ///
    /// That the ordering is *right*, only that it is the one D182 argued for. A corpus where
    /// every title presents would want a different tiebreak, and nothing here would notice.
    #[test]
    fn a_guest_spinning_on_present_does_not_outrank_one_that_got_further() {
        let spinning = Status {
            frames: 100_000,
            ..status(Reach::Flipped, 12, 466_000_000)
        };
        let further = Status {
            frames: 3,
            ..status(Reach::Flipped, 47, 933)
        };

        assert!(
            further.beats(&spinning),
            "a hundred thousand identical frames is not more of the interface than 47 imports"
        );
        assert!(!spinning.beats(&further));
    }

    /// **Frames still break a tie that nothing else can.**
    ///
    /// The other half: ranked below imports and standing, but above raw calls, because a frame
    /// is something achieved where a call is only something counted.
    #[test]
    fn frames_decide_between_two_runs_that_are_otherwise_identical() {
        let one = Status {
            frames: 1,
            ..status(Reach::Flipped, 47, 933)
        };
        let many = Status {
            frames: 60,
            ..status(Reach::Flipped, 47, 933)
        };

        assert!(many.beats(&one), "sixty frames is further than one");
        assert!(!one.beats(&many));
    }

    /// **The rung is not reachable by claiming it.**
    ///
    /// A title's recorded reach comes from [`crate::Reach`], and a run that presented nothing
    /// cannot sit at [`Reach::Flipped`] with zero frames - the promotion is driven by the port
    /// table's own count. This pins the pairing that makes the rung mean anything; the
    /// promotion itself is tested where it happens, against a trace.
    #[test]
    fn the_rung_and_the_count_agree() {
        let honest = status(Reach::Entered, 47, 933);
        assert_eq!(
            honest.frames, 0,
            "a title that never presented has no frames"
        );

        let presented = Status {
            frames: 1,
            ..status(Reach::Flipped, 47, 933)
        };
        assert!(
            presented.frames > 0,
            "and one at the rung above has at least one"
        );
    }

    /// Two experiments compare with each other; differing by one override is not a
    /// difference in kind.
    #[test]
    fn two_helped_runs_are_comparable_with_each_other() {
        let one = Status {
            overrides: 1,
            propping: 1,
            ..status(Reach::Entered, 23, 222)
        };
        let two = Status {
            overrides: 3,
            propping: 3,
            ..status(Reach::Entered, 40, 900)
        };

        assert!(two.comparable_with(&one));
        assert!(
            two.beats(&one),
            "more reached, under the same kind of policy"
        );
    }

    /// The line a person reads names both halves of the policy.
    #[test]
    fn the_policy_description_names_the_overrides_not_only_the_default() {
        let helped = Status {
            overrides: 2,
            ..status(Reach::Entered, 23, 222)
        };
        let said = helped.describe_policy();

        assert!(said.contains('2'), "{said}");
        assert!(
            said.contains("answered by name"),
            "a message naming only the default is how this went unnoticed: {said}"
        );
    }

    /// **A shipped record's `[settings]` reach a run, and the user's file wins per key.**
    ///
    /// The launcher's system view is the case that needs it: without the shipped layer it would
    /// run sandboxed and list no titles at all.
    #[test]
    fn a_shipped_setting_reaches_a_run_and_the_user_file_overrides_it() {
        let empty =
            std::env::temp_dir().join(format!("orbistoun-no-overrides-{}", std::process::id()));
        let launcher = Resolved::for_run("SCSH00001", &empty);
        assert_eq!(
            launcher.text(super::FILESYSTEM_VIEW),
            Some(super::FILESYSTEM_VIEW_SYSTEM)
        );
        assert_eq!(
            launcher.get(super::FILESYSTEM_VIEW).map(|v| v.layer),
            Some(Layer::Repo)
        );
        assert_eq!(
            Resolved::for_run("GLCB00001", &empty).text(super::FILESYSTEM_VIEW),
            None
        );

        let dir = std::env::temp_dir().join(format!("orbistoun-overrides-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("SCSH00001.toml"),
            "[settings]\nfilesystem_view = \"sandbox\"\n",
        )
        .unwrap();
        let mine = Resolved::for_run("SCSH00001", &dir);
        assert_eq!(mine.text(super::FILESYSTEM_VIEW), Some("sandbox"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
