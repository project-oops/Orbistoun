//! Per-title overrides: settings and compatibility entries, layered and merged.
//!
//! Title-specific behaviour never reaches the core: the core reads generic named settings and a
//! per-title file declares what a title needs (D048). [`Layer::Global`] defaults, then
//! [`Layer::Repo`], then [`Layer::User`], merged per key, never wholesale, so a user setting
//! cannot drop the repository's compatibility entries. A [`CompatEntry`] describes a deviation
//! with a [`CompatKind`] and a mandatory reason, keyed by behaviour (`raytracing_enabled`), never
//! by title; a preference is an ordinary setting scoped per title. [`Resolved`] records which
//! layer set every key, so a run report shows effective configuration with provenance.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

/// Why a compatibility record could not be written.
#[derive(Debug, thiserror::Error)]
pub enum OverridesError {
    /// The record could not be rendered as TOML.
    #[error("serialising TOML: {0}")]
    Toml(#[from] toml::ser::Error),
}

/// How the worker words a guest's deliberate exit, and the one string that identifies it.
///
/// `orbistoun-core::StopReason::Exited` produces this text; neither this crate nor
/// `orbistoun-report` can depend on it, so it is repeated here and a test in `orbistoun-worker`
/// asserts the two agree. Here rather than beside the ladder because [`Status`] also recognises
/// it: a run that flipped and exited is recorded as `Flipped`.
pub const DELIBERATE_EXIT: &str = "the guest called exit";

/// A setting value.
///
/// Typed rather than boolean-only: a typed value (`direct_memory_alignment = 4096`) generalises
/// where booleans multiply into a list of exceptions.
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

/// Why a compatibility entry exists; each kind resolves differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompatKind {
    /// The title does something out of spec that real hardware tolerates. Legitimate and permanent.
    Quirk,
    /// This implementation is wrong and the entry masks it. Temporary; deleted with the fix.
    Workaround,
    /// A capability not built. Deleted when the feature ships, and aggregated into a feature-level
    /// work list.
    Unsupported,
}

/// A compatibility entry: a value, why it is set, and which kind of debt it is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompatEntry {
    /// The value to apply.
    pub value: Value,
    /// What kind of deviation this is.
    pub kind: CompatKind,
    /// Why it is here. Mandatory by construction, so no entry is an unexplained exception.
    pub reason: String,
}

/// How far a title got, coarsely.
///
/// Each rung is a phase the loader distinguishes, so a grade is derived from a run rather than
/// typed by a person (D182). Coarse on purpose: within a rung, runs are separated by their
/// measured counts rather than more rungs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reach {
    /// The container did not parse. Nothing about the title is known yet.
    Rejected,
    /// Parsed, but its imports could not be resolved.
    Parsed,
    /// Linked and ready, and never entered - so nothing has been learned about the guest.
    Linked,
    /// Guest code ran.
    ///
    /// Surviving to the time limit earns no rung above this: a guest spinning on a few
    /// unimplemented functions survives while one reaching many imports faults. Not dying is an
    /// outcome, recorded in [`Status::outcome`], not a distance (D182).
    Entered,
    /// The guest ran its program to the end and left by calling `exit`.
    ///
    /// A call the guest made, not something that failed to happen, so it earns a rung (D685). It sits
    /// below a flip because `exit(0)` as a first instruction reaches it having learned nothing, and a
    /// guest that flips and then exits is recorded as `Flipped`, with the stop carried in
    /// [`Status::outcome`].
    Exited,
    /// The guest got a frame to the output layer: it submitted a flip and a real port took it.
    ///
    /// Reached only after opening an output, setting attributes, registering buffers and configuring
    /// it against real implementations, so it cannot be arrived at by spinning (D558). It does not
    /// mean a picture was displayed: a flip completes when accepted. Within this rung, imports and
    /// standing still rank before frames (see [`Status::beats`]).
    Flipped,
    /// The guest put pixels it produced into a buffer that reached the output layer.
    ///
    /// The top rung (D694), awarded only when a flipped buffer is read back holding something the
    /// guest wrote, a positive measurement against a known prior that reaching the interface cannot
    /// fake. Rungs are decided by `orbistoun_report::trace::status_of`, not this crate.
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
/// Kept in the same file as the title's settings, since both are keyed by title and edited
/// together. Not merged: [`Resolved::merge`] layers configuration, while two measurements
/// are facts about different runs, compared with [`Status::beats`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    /// How far it got.
    pub reach: Reach,
    /// How it ended, in words: the fault site, the guest's own decision, or the limit.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub outcome: String,
    /// Distinct imports the guest called.
    #[serde(default)]
    pub imports: usize,
    /// Total calls through any stub.
    #[serde(default)]
    pub calls: u64,
    /// What percentage of those calls reached an implementation rather than a placeholder, so
    /// results compare across policies (D181).
    #[serde(default)]
    pub standing: u32,
    /// What unimplemented functions answered during the run.
    ///
    /// A result under stubs reporting success reaches further and means less, so the entry cannot be
    /// compared without this.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub default_return: String,
    /// How many functions were handed a specific answer instead of the default.
    ///
    /// A measured policy leaves the default at `unimplemented` and puts its answers here, so both are
    /// recorded (D312).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub overrides: usize,
    /// How many of those rest on nothing measured.
    ///
    /// An answer taken from the target is the emulator being right; one guessed until the guest
    /// moved is a prop. `overrides` cannot tell them apart, so this decides whether the run was
    /// honest (D557).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub propping: usize,
    /// Frames the guest handed to the output layer: zero below [`Reach::Flipped`], and the distance
    /// within that rung once reached (D558).
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub frames: u64,
    /// Distinct imports the guest called that had nothing behind them.
    ///
    /// [`Self::standing`] is a percentage of calls, dominated by whatever the guest loops on, so it
    /// rounds real progress away; counting functions is stable and is the work list (D563). [`None`]
    /// means this run did not measure it, which is not `Some(0)`; see [`Self::answered`] for how an
    /// unmeasured record ranks.
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
    /// Either a loosened default or any answer resting on nothing measured counts: loosening the
    /// default and answering one function by name are the same act at different scales.
    /// `propping` counts regions as well as answers, and a measured answer is not a prop (D557).
    pub fn propped_up(&self) -> bool {
        (!self.default_return.is_empty() && self.default_return != "unimplemented")
            || self.propping > 0
    }

    /// The policy in a phrase, for the line that says why an entry is set apart; names both halves,
    /// since either can be doing the propping.
    #[must_use]
    pub fn describe_policy(&self) -> String {
        let default = if self.default_return.is_empty() {
            "unimplemented"
        } else {
            &self.default_return
        };
        // Both numbers, since they answer different questions.
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
    /// Zero where nothing measured it, which is a record that cannot say, not a claim. Ranked as low
    /// as possible, so the next run replaces it with a real number (D563).
    #[must_use]
    pub fn answered(&self) -> usize {
        self.unanswered
            .map_or(0, |missing| self.imports.saturating_sub(missing))
    }

    /// Whether the guest left by calling `exit` rather than dying.
    ///
    /// Read from [`Self::outcome`], since a run that flipped and exited is recorded as `Flipped`
    /// (D685).
    #[must_use]
    pub fn exited_deliberately(&self) -> bool {
        self.outcome == DELIBERATE_EXIT
    }

    /// The order results are ranked in, in one place.
    ///
    /// [`Self::beats`], [`frontier`] and [`render_markdown`] all rank by this, so the record, a
    /// shim's view and the table cannot disagree. Order: the rung; whether the guest left
    /// deliberately; imports reached; imports answered; share of calls answered; frames; calls.
    /// Everything after `imports` is a quality measure and stays below it, since going further means
    /// calling more unimplemented functions. The ending sits above `imports` because a run whose
    /// import count fell by stopping correctly must rank as better (D686).
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
    /// Only the stub policy is checked: a longer time limit genuinely gets further, while a looser
    /// policy moves the numbers without anything being true. Propped-up runs compare with each
    /// other, not with honest ones (D312).
    pub fn comparable_with(&self, other: &Self) -> bool {
        self.default_return == other.default_return && self.propped_up() == other.propped_up()
    }

    /// Whether this result should replace `previous`: not worse, and not the same run again.
    ///
    /// Comparable, not below the record on the ranked key, and different in the key or the outcome,
    /// so a guest that now stops deliberately where it used to fault is recorded (D687).
    /// `measured_on` differs on every run and is not counted as a difference.
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
    /// The verdict, not the recording gate ([`Self::worth_recording`]). A looser policy is never an
    /// improvement on an honest record, since it reaches further by construction. The ladder decides
    /// first; then the ranking key (see `ranking_key`). `standing` lets implementing a function the
    /// guest already called count as progress; frames sit below imports so a guest re-presenting the
    /// same buffer does not outrank one that got further (D558).
    pub fn beats(&self, previous: &Self) -> bool {
        if !self.comparable_with(previous) {
            return false;
        }
        self.ranking_key() > previous.ranking_key()
    }
}

/// Whether a count is zero, so an ordinary run writes no line about overrides.
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
/// Below the shims so every view uses one order (D034). Sorted by the relation [`Status::beats`]
/// uses, so the table and the record cannot rank differently.
pub fn frontier(mut titles: Vec<(String, Status)>) -> Vec<(String, Status)> {
    titles.sort_by(|a, b| {
        b.1.ranking_key()
            .cmp(&a.1.ranking_key())
            // Ties broken by name so the order is total and identically measured titles do not reorder
            // between runs.
            .then_with(|| a.0.cmp(&b.0))
    });
    titles
}

/// The frontier as a table, one line per title.
///
/// Rendered here so a test holds the whole shape against real records: a bad ranking is obvious
/// in the real table (D184).
pub fn render_frontier(titles: &[(String, Status)]) -> String {
    use core::fmt::Write as _;

    let mut out = String::new();
    for (title, status) in titles {
        // Writing into the buffer: one allocation fewer per line, as the lint asks.
        let _ = writeln!(
            out,
            "{:<22} {:<10} {:>3} imports ({} answered) {:>10} calls {:>4}% standing   {}",
            title,
            status.reach.label(),
            status.imports,
            // A dash where nothing measured it, so a run replacing one with more calls shows why.
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

/// One row of the compatibility table: a title, the result to show, whether that result came from
/// the `experiment` slot (a run with overrides, recorded apart), and a screenshot path if the
/// guest produced one.
#[derive(Debug, Clone)]
pub struct Row {
    /// The title id.
    pub title: String,
    /// The name a person would call it, where the title says one.
    ///
    /// Beside the id, which every other artefact keys on (D660).
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
/// Beside [`render_frontier`] so the two views share one ranking (D184). A guest with a
/// screenshot gets a camera mark in the table and an embedded image below it.
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
        // Linked to the page, so the table leads to it.
        let shown = match &r.name {
            Some(name) => format!("[{}](docs/titles/{}.md)", md_cell(name), r.title),
            None => format!("[{}](docs/titles/{}.md)", r.title, r.title),
        };
        let from = if r.experiment { "experiment" } else { "run" };
        // A dash where nothing measured it rather than a zero: the two mean opposite things.
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
const CAPTURES: &str = "../../compat/screenshots";

/// The stand-in for a capture nobody has taken: a file rather than an omission, so a page is
/// never read as broken or incomplete (D660).
const NO_CAPTURE: &str = "../../compat/screenshots/no-capture.svg";

/// One title's page: what it is, how far it got, and what has been captured of it.
///
/// Regenerated from the record every time, so it cannot drift from the derived status, and says
/// so at the top (D660). Everything missing is shown as missing.
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
    // "Ships no `param.json`" is not "the field is missing": a homebrew payload has no metadata file.
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
        // "not recorded" rather than a blank cell, which would read as a rendering fault.
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
    // Both rows always, so pages differ by image rather than layout.
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
/// Read from `sce_sys/param.json`, which the title ships, rather than typed into a record where
/// it could drift (D660). An id, a name and two version strings, no guest material. Every field
/// is optional because a homebrew payload has no such file.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Title {
    /// The title id the container declares, which should match the record's filename; kept so a
    /// mismatch shows a copied file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The name a person would call it, rather than the title id.
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

    /// The name to show, falling back to the id and then to nothing; never invented.
    #[must_use]
    pub fn display<'a>(&'a self, fallback: &'a str) -> &'a str {
        self.name.as_deref().unwrap_or(fallback)
    }
}

/// What the hardware does with this title, attested from outside orbistoun.
///
/// Ground truth orbistoun cannot measure: somebody ran the title on hardware. A fault in a title
/// known to work on hardware is orbistoun's to close, and blaming the title needs a hardware
/// observation of the same failure (D708).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hardware {
    /// What it does on the hardware, in plain words - `renders`, `boots to menu`, `plays`, or a
    /// specific failure. An observation, not a measured rung.
    pub does: String,
    /// Who says so - `operator`, or an obSCEne / probe id.
    pub attested_by: String,
    /// When, so a stale attestation can be re-checked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on: Option<String>,
    /// Anything worth carrying - the firmware, how it was seen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl Hardware {
    /// Whether this attestation says the title does something real on hardware, rather than
    /// recording a failure. A blank `does` is no claim.
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
/// `BTreeMap` throughout so serialisation is deterministic and diffs show no ordering churn.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OverrideFile {
    /// What the title says it is, from its own `param.json`; written as absent rather than blank
    /// strings, so "not known" and "known to be empty" differ (D660).
    #[serde(default, skip_serializing_if = "Title::is_empty")]
    pub title: Title,
    /// What the hardware does with this title, attested from outside orbistoun (D708).
    ///
    /// Absent means nobody has said; a run report then treats a fault as orbistoun's gap and the
    /// hardware status as unknown.
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
    /// Absent means no run is recorded, which differs from a run that got nowhere
    /// ([`Reach::Rejected`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<Status>,
    /// The furthest a run got while being helped - a loosened default, or functions answered by name.
    ///
    /// Kept rather than refused, in a slot apart from [`Self::status`], because the two answer
    /// different questions: how far the emulator takes this title, and how far it could if the
    /// measured thing were implemented (D312).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experiment: Option<Status>,
}

impl OverrideFile {
    /// Parses TOML.
    pub fn from_toml(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }

    /// Serialises to TOML.
    ///
    /// # Errors
    ///
    /// When the record cannot be serialised.
    pub fn to_toml(&self) -> Result<String, OverridesError> {
        Ok(toml::to_string_pretty(self)?)
    }

    /// How to frame a fault this title just hit: whose gap it is, by the hardware ground truth
    /// (D708).
    ///
    /// orbistoun is the default owner of a fault. A `[hardware]` attestation that the title is sound
    /// makes that unambiguous; with none, the status is reported as unknown rather than the title
    /// being assumed broken.
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
    /// Present when this key came from a compatibility entry rather than a plain setting.
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
    /// A later layer replaces only the keys it names. Within one file, compatibility entries and
    /// settings share a namespace, and a compatibility entry wins, since it carries a reason.
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
    /// `None` if unset or set to a non-boolean; a type confusion surfaces rather than coercing.
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

    /// Every entry of a given compatibility kind: `workaround` lists what is being masked,
    /// `unsupported` aggregates into a feature-level work list.
    pub fn of_kind(&self, kind: CompatKind) -> Vec<(&str, &ResolvedValue)> {
        self.values
            .iter()
            .filter(|(_, v)| v.compat.as_ref().is_some_and(|c| c.kind == kind))
            .map(|(k, v)| (k.as_str(), v))
            .collect()
    }

    /// Whether anything is in force; an empty resolution means stock behaviour.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// How many keys are in force.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// A title's effective settings for a run: the shipped record's `[settings]` (the repository
    /// layer, built in), then the user's `<overrides_dir>/<title>.toml` over it.
    ///
    /// A user file that does not parse contributes nothing rather than failing the run.
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
/// [`FILESYSTEM_VIEW_SYSTEM`] is the whole tree, as a system application sees it.
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

    /// A fault in a title sound on hardware is framed as orbistoun's, and a title with no
    /// attestation still defaults to orbistoun (D708).
    #[test]
    fn fault_attribution_defaults_to_orbistoun_and_reads_the_hardware_ground_truth() {
        let text = |file: &OverrideFile| file.fault_attribution().join("\n");

        // Sound on hardware: orbistoun's to close.
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

        // No attestation: still orbistoun, with the status unknown.
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

        // Only a hardware failure opens the title's own code to suspicion.
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

    /// Later layers win per key.
    #[test]
    fn later_layers_win_per_key() {
        let global = file(&[("resolution_scale", Value::Int(1))], &[]);
        let user = file(&[("resolution_scale", Value::Int(2))], &[]);
        let r = Resolved::merge(&[(Layer::Global, global), (Layer::User, user)]);
        assert_eq!(r.int("resolution_scale"), Some(2));
        assert_eq!(r.get("resolution_scale").expect("set").layer, Layer::User);
    }

    /// A user setting does not drop the repository's compatibility entries.
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

    /// A user may override a compatibility entry, and the override is visible in the provenance.
    #[test]
    fn a_user_may_deliberately_override_a_compatibility_entry_and_it_is_visible() {
        // Allowed, and the provenance shows it in the run report.
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

    /// Provenance is recorded for every key.
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

    /// Entries are queryable by compatibility kind.
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

    /// Typed values do not coerce.
    #[test]
    fn typed_values_do_not_coerce() {
        // A type confusion surfaces rather than being accepted.
        let r = Resolved::merge(&[(Layer::User, file(&[("x", Value::Int(1))], &[]))]);
        assert_eq!(r.int("x"), Some(1));
        assert_eq!(r.bool("x"), None, "an int is not a bool");
    }

    /// The worked example from the decision log round-trips.
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

    /// A compatibility entry without a reason is rejected.
    #[test]
    fn a_compat_entry_without_a_reason_is_rejected() {
        // An entry with no reason is rejected.
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

    /// An unknown compatibility kind is rejected rather than defaulted.
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

    /// An empty file parses to nothing in force.
    #[test]
    fn an_empty_file_parses_to_nothing_in_force() {
        let f = OverrideFile::from_toml("").expect("empty is valid");
        let r = Resolved::merge(&[(Layer::Repo, f)]);
        assert!(r.is_empty(), "no overrides means stock behaviour");
    }

    /// Ordering is deterministic for diffing.
    #[test]
    fn ordering_is_deterministic_for_diffing() {
        // Run reports are diffed between runs, so map ordering must not churn.
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

    /// The markdown table ranks furthest first and marks screenshots.
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
        // The further guest ranks above the linked one, despite input order.
        assert!(
            md.find("far").unwrap() < md.find("near").unwrap(),
            "further title must come first"
        );
        // The camera follows the linked title cell, so the assertion names the whole cell; the bare
        // title would also match the link target.
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

    /// The markdown says so when there are no screenshots.
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

    /// A looser policy never beats an honest record.
    #[test]
    fn a_looser_policy_can_never_beat_an_honest_record() {
        // One line of configuration makes a run reach further than the emulator can; ranking on numbers
        // alone would let it overwrite an honest entry.
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

    /// An honest result does not beat an inflated record either: the two are incomparable.
    #[test]
    fn an_honest_result_cannot_beat_an_inflated_record_either() {
        // The refusal is "not comparable", not "the bigger number wins", so an honest run can still
        // displace a contaminated entry by other means.
        let inflated = Status {
            default_return: "ok".to_owned(),
            ..status(Reach::Entered, 480, 90_000)
        };
        assert!(!status(Reach::Entered, 47, 933).beats(&inflated));
    }

    /// The ladder outranks the counts.
    #[test]
    fn the_ladder_outranks_the_counts() {
        // A title linked and never entered told nothing about the guest; reach decides first.
        assert!(status(Reach::Entered, 1, 1).beats(&status(Reach::Linked, 500, 0)));
    }

    /// Within a rung, imports outrank calls.
    #[test]
    fn within_a_rung_imports_outrank_calls() {
        // Calls are the weakest signal: a guest spinning on one function accumulates them.
        let spinning = status(Reach::Entered, 12, 466_000_000);
        let broader = status(Reach::Entered, 47, 933);
        assert!(broader.beats(&spinning));
        assert!(!spinning.beats(&broader));
    }

    /// Surviving the time limit is not a higher rung than faulting.
    #[test]
    fn surviving_the_time_limit_is_not_a_higher_rung_than_faulting() {
        // A title spinning to the time limit must not sort above one that reached more imports and
        // faulted.
        let spinning = Status {
            outcome: "ran to the time limit".to_owned(),
            ..status(Reach::Entered, 4, 91_455_278)
        };
        let informative = status(Reach::Entered, 47, 933);
        assert!(informative.beats(&spinning));
        assert!(!spinning.beats(&informative));
    }

    /// Implementing something the guest already called counts as progress.
    #[test]
    fn implementing_something_the_guest_already_called_counts_as_progress() {
        // Implementing a function the guest already calls moves no import and no call, only standing.
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

    /// Breadth still outranks quality.
    #[test]
    fn breadth_still_outranks_quality() {
        // Standing breaks ties within a breadth rather than substituting for it.
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

    /// An identical rerun is not an improvement.
    #[test]
    fn an_identical_rerun_is_not_an_improvement() {
        // Otherwise every run rewrites the record with the same numbers and a new date.
        let now = status(Reach::Entered, 47, 933);
        assert!(!now.beats(&status(Reach::Entered, 47, 933)));
    }

    /// A run that reaches exactly as far but ended somewhere else is worth recording (D687).
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

    /// The same run again writes nothing.
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

    /// A measurement is not layered the way configuration is.
    #[test]
    fn a_measurement_is_not_layered_the_way_configuration_is() {
        // Settings merge per key across layers; a measurement does not, since two runs are two facts.
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

    /// A record round-trips through TOML with its settings.
    #[test]
    fn a_record_round_trips_through_toml_with_its_settings() {
        // The two halves share a file, so a save must keep both.
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

    /// A run helped by named overrides is not an honest measurement, though the default is
    /// untouched (D312).
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

    /// A guessed override still props a run up.
    ///
    /// The rule narrowed to entries resting on nothing measured; this pins that such a run is still
    /// caught, and that a run moves from experiment to honest only by being measured.
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
        // The region-only case: no answers, one write into guest memory behind an unmeasured byte count.
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

    /// A measured answer is the emulator being right, and does not prop a run up (D557).
    ///
    /// Whether the answer is right is the provenance label's claim, graded by
    /// [`orbistoun_hle::knowledge::Oracle::is_evidence`].
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

    /// The record, the frontier and the table cannot rank differently, because they share one key
    /// (D563).
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
        // Every adjacent pair agrees with `beats`, the relation the record uses.
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

    /// Answering more of the same imports is an improvement the record can see (D563).
    #[test]
    fn answering_more_of_the_same_imports_is_an_improvement() {
        let before = Status {
            unanswered: Some(35),
            ..status(Reach::Flipped, 197, 419_091)
        };
        let after = Status {
            unanswered: Some(20),
            // Fewer calls and the same rounded standing, so nothing else in the key carries this.
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

    /// Answering more never outranks reaching more: a guest that goes further may have more
    /// unanswered imports.
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

    /// A record that never measured unanswered imports cannot claim a perfect score and outrank a
    /// measured one (D563).
    #[test]
    fn an_unmeasured_record_does_not_outrank_a_measured_one() {
        let old = Status {
            unanswered: None,
            ..status(Reach::Flipped, 197, 418_464)
        };
        let measured = Status {
            // Almost nothing answered, and it still wins, because it measured something.
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

    /// A guest that presented ranks above one that only entered (D558).
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

    /// A guest spinning on present does not outrank one that got further: frames rank below
    /// imports (D182).
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

    /// Frames break a tie that nothing else can.
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

    /// The rung and the frame count agree: [`Reach::Flipped`] never has zero frames.
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

    /// Two helped runs are comparable with each other.
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

    /// A shipped record's `[settings]` reach a run, and the user's file wins per key.
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
