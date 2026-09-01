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
}

impl Reach {
    /// How to name it in a report.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Rejected => "rejected",
            Self::Parsed => "parsed",
            Self::Linked => "linked",
            Self::Entered => "entered",
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
    pub fn propped_up(&self) -> bool {
        (!self.default_return.is_empty() && self.default_return != "unimplemented")
            || self.overrides > 0
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
        match self.overrides {
            0 => default.to_owned(),
            1 => format!("{default}, with 1 function answered by name"),
            n => format!("{default}, with {n} functions answered by name"),
        }
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

    /// Whether this result is better than `previous`, and therefore worth recording.
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
    pub fn beats(&self, previous: &Self) -> bool {
        if !self.comparable_with(previous) {
            return false;
        }
        (self.reach, self.imports, self.standing, self.calls)
            > (
                previous.reach,
                previous.imports,
                previous.standing,
                previous.calls,
            )
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
        (b.1.reach, b.1.imports, b.1.standing, b.1.calls)
            .cmp(&(a.1.reach, a.1.imports, a.1.standing, a.1.calls))
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
            "{:<22} {:<10} {:>3} imports {:>10} calls {:>4}% standing   {}",
            title,
            status.reach.label(),
            status.imports,
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
        (
            b.status.reach,
            b.status.imports,
            b.status.standing,
            b.status.calls,
        )
            .cmp(&(
                a.status.reach,
                a.status.imports,
                a.status.standing,
                a.status.calls,
            ))
            .then_with(|| a.title.cmp(&b.title))
    });

    let mut out = String::new();
    out.push_str("| Title | Reach | Imports | Calls | Standing | Outcome | From | Measured |\n");
    out.push_str("|---|---|--:|--:|--:|---|---|---|\n");
    for r in &ranked {
        let mark = if r.screenshot.is_some() { " 📷" } else { "" };
        let from = if r.experiment { "experiment" } else { "run" };
        let _ = writeln!(
            out,
            "| {}{} | {} | {} | {} | {}% | {} | {} | {} |",
            md_cell(&r.title),
            mark,
            r.status.reach.label(),
            r.status.imports,
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
        out.push_str(
            "_None yet. A screenshot needs a captured guest framebuffer, which the video \
             subsystem does not surface yet; a guest that produces graphics gains an image here \
             once it does._\n",
        );
    } else {
        for r in shots {
            if let Some(path) = &r.screenshot {
                let _ = writeln!(out, "### {}\n\n![{}]({})\n", r.title, r.title, path);
            }
        }
    }
    out
}

/// Escape the two characters that break a markdown table cell.
fn md_cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

/// One override file, as it appears on disk.
///
/// `BTreeMap` throughout so serialisation is deterministic: run reports are diffed
/// between runs, and map ordering churn would show up as spurious change.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OverrideFile {
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
}

#[cfg(test)]
mod tests {
    use super::{
        CompatEntry, CompatKind, Layer, OverrideFile, Reach, Resolved, Row, Status, Value,
        render_markdown,
    };
    use std::collections::BTreeMap;

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
                title: "near".to_owned(),
                status: status(Reach::Linked, 0, 0),
                experiment: true,
                screenshot: None,
            },
            Row {
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
        assert!(
            md.contains("far 📷"),
            "the guest with a screenshot is marked"
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

    /// Two experiments compare with each other; differing by one override is not a
    /// difference in kind.
    #[test]
    fn two_helped_runs_are_comparable_with_each_other() {
        let one = Status {
            overrides: 1,
            ..status(Reach::Entered, 23, 222)
        };
        let two = Status {
            overrides: 3,
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
}
