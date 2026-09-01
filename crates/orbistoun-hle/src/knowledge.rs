//! What we know about guest functions, as opposed to what a tool worked out.
//!
//! There are exactly two kinds of fact about a guest function, and they want different
//! homes:
//!
//! - **Derived** - which pattern generated a name, at which index, on which day. A
//!   search produces this, `symbols/generated.json` holds it, and that file is
//!   **overwritten on every search**. Hand-editing it would be a lie, and anything
//!   irreplaceable stored there would be destroyed on the next run.
//! - **Known** - how many arguments a function takes, what they mean, what it is for,
//!   what it does at its edges. No tool can produce any of this. Only observation can,
//!   and once observed it must never be lost.
//!
//! This is the second kind. It accumulates and is never regenerated (D122).
//!
//! # This is the output of the loop, not documentation about it
//!
//! Every turn of the development cycle produces exactly these facts: run a title, watch
//! what a function does, learn something. Until now that landed in decision-log prose
//! and could only be recovered by grepping - so `sceKernelDirectMemoryQuery` had its
//! argument layout, its ignored return value and its buffer-clearing requirement
//! established by measurement, and none of it attached to the function.
//!
//! Written by tooling as much as by hand, for the same reason: a session that has just
//! learned something should record it with a command, not by editing TOML and hoping the
//! formatting survives.
//!
//! # The NID is not in here
//!
//! Derived from the name, never stored, so a file cannot hold a pair that disagrees with
//! itself. Same rule `docs/SYMBOLS.md` sets for symbol databases.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Knowledge files shipped with the tool, embedded so a portable build carries them.
const EMBEDDED: &[(&str, &str)] = &[
    (
        "libkernel",
        include_str!("../data/knowledge/libkernel.toml"),
    ),
    (
        "libSceAgc",
        include_str!("../data/knowledge/libSceAgc.toml"),
    ),
    ("libc", include_str!("../data/knowledge/libc.toml")),
    (
        "libSceVideoOut",
        include_str!("../data/knowledge/libSceVideoOut.toml"),
    ),
    (
        "libkernel_fs",
        include_str!("../data/knowledge/libkernel_fs.toml"),
    ),
    (
        "libSceSystemService",
        include_str!("../data/knowledge/libSceSystemService.toml"),
    ),
    (
        "libScePosix",
        include_str!("../data/knowledge/libScePosix.toml"),
    ),
    (
        "libScePad",
        include_str!("../data/knowledge/libScePad.toml"),
    ),
    (
        "libSceGnmDriver",
        include_str!("../data/knowledge/libSceGnmDriver.toml"),
    ),
    (
        "libSceSysmodule",
        include_str!("../data/knowledge/libSceSysmodule.toml"),
    ),
    (
        "libSceAudioOut",
        include_str!("../data/knowledge/libSceAudioOut.toml"),
    ),
    (
        "libSceUlt",
        include_str!("../data/knowledge/libSceUlt.toml"),
    ),
];

/// One argument, as far as it is understood.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Argument {
    /// What to call it. Empty when only its position is known.
    #[serde(default)]
    pub name: String,
    /// Its shape - `u64`, `ptr`, `u32`, and so on. Deliberately loose: a guess at a
    /// width is worth recording, a guess at a C type is not.
    #[serde(default)]
    pub kind: String,
    /// Anything a reader would want and cannot infer.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

/// How a *behavioural* claim was established.
///
/// # Why this exists, and why there is no value for "I already knew it"
///
/// [`FunctionKnowledge::found_by`] records how a **name** was arrived at, and CI re-derives
/// every committed name from this repository's own inputs. Nothing did the same for
/// behaviour - an arity, a return kind, what happens at an edge - and those are the facts
/// that change what the emulator does.
///
/// The gap matters more than it used to. Facts increasingly arrive by way of a model that
/// has read the public internet, so "this is what the function does" can be *recalled* and
/// then dressed as reasoning. That is the convergence problem principle 1 exists to
/// prevent, arriving by a route the principle does not name.
///
/// So the vocabulary is the enforcement. **Every value here is falsifiable**, and there is
/// deliberately none meaning "known from experience": recording a fact requires committing
/// to a checkable claim about where it came from, which is a different act from absorbing
/// one silently.
///
/// One field answers three separate worries - a licence question (did this come from
/// someone else's source?), a quality one (is this reasoned or generated?), and an
/// operational one (which of our facts are actually guesses?).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Oracle {
    /// A published standard or published source: ISO C, POSIX, or the FreeBSD tree the
    /// target C library derives from. The strongest reference available, and citable.
    Published,
    /// Measured on real hardware by a conformance probe.
    ///
    /// The cleanest provenance in the list. Observing what a box you own does with an
    /// input you chose is nobody else's work, and unlike every other entry here it scales:
    /// one hardware run answers a batch of questions rather than one.
    Measured,
    /// The guest itself - it proceeded when answered this way, and stopped otherwise.
    ///
    /// One bit per boot, and the bit is *consistency*, not correctness: a guest proceeds
    /// happily past an answer that is wrong in a way it never checks. Enough to rule things
    /// out, never enough to call something confirmed.
    GuestObserved,
    /// Nobody knows. The value recorded is a placeholder chosen to be least harmful.
    ///
    /// **Not a failure state, and not rare.** Much of this project is here, and saying so
    /// is the entire point: an assumption that is written down can be counted, ranked,
    /// probed and retired, where one written as though it were a fact never will be.
    Assumed,
}

impl Oracle {
    /// How it is written in a file, and in a report.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Published => "published",
            Self::Measured => "measured",
            Self::GuestObserved => "guest-observed",
            Self::Assumed => "assumed",
        }
    }

    /// Whether this claims support from something outside this repository.
    ///
    /// Those have to say where. A citation is what lets someone who was not there check
    /// the claim, and an uncheckable claim of external support is worth strictly less than
    /// an honest [`Oracle::Assumed`] - it looks like evidence and is not.
    pub const fn needs_citation(self) -> bool {
        matches!(self, Self::Published | Self::Measured)
    }

    /// Whether the claim rests on nothing yet.
    pub const fn is_guess(self) -> bool {
        matches!(self, Self::Assumed)
    }

    /// Whether a conformance probe on real hardware could settle it.
    ///
    /// What makes the assumption count a worklist rather than an apology.
    pub const fn is_probeable(self) -> bool {
        matches!(self, Self::Assumed | Self::GuestObserved)
    }
}

/// Everything known about one guest function.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionKnowledge {
    /// The symbol name, exactly as the guest imports it.
    pub name: String,
    /// How many integer arguments it takes, where that has been established.
    ///
    /// `None` means unknown rather than zero - and the difference matters, because zero
    /// is a real answer that a trace would render very differently.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arity: Option<u8>,
    /// What the function is for, in prose.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub purpose: String,
    /// What kind of value it hands back.
    ///
    /// **Load-bearing, not documentation.** An unimplemented function has to answer
    /// something, and the right answer depends entirely on this: an error code is
    /// correct for a function returning status and is a **wild pointer** for one
    /// returning a handle - which the guest then dereferences (D125).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub returns: Option<Returns>,
    /// The arguments, in register order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arguments: Vec<Argument>,
    /// Behaviour a reimplementation would otherwise get wrong.
    ///
    /// The expensive knowledge. Each entry here cost an experiment.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edge_cases: Vec<String>,
    /// Experiments that would answer this entry's open questions, by name.
    ///
    /// # Why a label rather than prose
    ///
    /// An open question is written for a person - *"the map shape the guest will accept is
    /// unknown"* - and classifying one by its words is guesswork wearing a rule's clothes. A
    /// label is a claim somebody made deliberately: **this experiment would settle it**.
    ///
    /// It is what lets the dispatcher act on what the project already knows it does not know,
    /// rather than only on what crashed this run. 277 questions were recorded and ranked, and
    /// nothing read them (D356).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub answerable_by: Vec<String>,
    /// How the name was arrived at - one of the labels in [`FOUND_BY_LABELS`].
    ///
    /// # A second copy of a fact the symbol database already audits
    ///
    /// `symbols/generated.json` records, for every name it worked out, exactly how - and
    /// CI re-runs each of those records rather than reading them. This field is the same
    /// claim, hand-written, in a file nothing was checking.
    ///
    /// It drifted, as a second copy does. Eleven entries disagreed with the audited record:
    /// six libc names recorded as `observed` that the published-standard list produces,
    /// three C++ ABI names recorded as `published-standard` that no shipped list contains,
    /// and `sceKernelWrite` recorded as **`supplied`** - the one label that says "this
    /// project did not derive this name" - when the generator produces it (D213).
    ///
    /// **A gate was running the whole time.** `the_shipped_files_account_for_everything_they_claim`
    /// asserts [`Knowledge::provenance_faults`] is empty on every `cargo test`, and it
    /// passed on all eleven - because the function checked [`Self::known_by`] and its
    /// citation and had never looked at this field at all. It is the same shape as the
    /// other three: a guard reporting success on something it was not examining.
    ///
    /// It checks both halves now: the label must be current vocabulary, and where the symbol
    /// database has a record for the same name, the two must agree. The duplication remains,
    /// because a name that is *implemented* never enters the unnamed set and so gets no
    /// record in the symbol database at all: 57 of the 95 declared functions have none. See
    /// `docs/BACKLOG.md`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub found_by: String,
    /// How the *behaviour* recorded above was established.
    ///
    /// Deliberately separate from [`Self::found_by`], which is about the name. A function
    /// can carry a name straight out of the C standard and a return value nobody has ever
    /// checked, and one field could not say both.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub known_by: Option<Oracle>,
    /// Where to look to check it - a standard clause, a source file and revision, a probe.
    ///
    /// Required by [`Oracle::needs_citation`]. Free text, because the sources are not
    /// uniform and a schema would only move the vagueness somewhere less visible.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cites: String,
    /// Claims inside this entry that [`Self::known_by`] does **not** cover.
    ///
    /// The mixed entry is the normal one: shape from the standard, arity measured, and the
    /// behaviour at one specific edge a guess. A single provenance per function would have
    /// to round that up or down, and rounding up is exactly how a guess becomes a fact.
    ///
    /// **This list is a worklist.** Each line is a question a conformance probe on real
    /// hardware could answer, so everything counted here is work that can be retired
    /// rather than debt that merely accrues.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assumptions: Vec<String>,
    /// Which guest modules it was seen in.
    ///
    /// Title identifiers only, never paths: the modules themselves are never tracked,
    /// and an identifier is enough to repeat a measurement.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub found_in: Vec<String>,
    /// The day it was first recorded.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub found_on: String,
    /// Anything else worth keeping.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

impl FunctionKnowledge {
    /// Whether anything beyond the name is known.
    ///
    /// A bare entry is not useless - it records that we have seen the function - but it
    /// is worth being able to count them, because that count is the size of the job.
    pub fn is_bare(&self) -> bool {
        self.arity.is_none()
            && self.purpose.is_empty()
            && self.arguments.is_empty()
            && self.edge_cases.is_empty()
    }

    /// Whether this entry makes a claim about behaviour that provenance has to account
    /// for.
    ///
    /// A bare entry records only that the function was seen, which needs no source.
    pub fn claims_behaviour(&self) -> bool {
        !self.is_bare()
    }

    /// What is wrong with this entry's provenance, in words, or nothing.
    ///
    /// Returned rather than asserted so that one caller can fail a build with it and
    /// another can show a person what is missing. A check only CI can run gets fixed only
    /// when CI complains, which is the slowest possible moment.
    pub fn provenance_faults(&self) -> Vec<String> {
        let mut faults = self.name_provenance_faults();
        let Some(known) = self.known_by else {
            if self.claims_behaviour() {
                faults.push(format!(
                    "{}: records behaviour but does not say how it is known",
                    self.name
                ));
            }
            return faults;
        };
        if known.needs_citation() && self.cites.is_empty() {
            faults.push(format!(
                "{}: known_by = {} claims an outside source but cites none",
                self.name,
                known.label()
            ));
        }
        // A citation naming a filesystem path is not a citation. `cites` exists so that
        // somebody else can check a claim, and the whole value of that is defeated by a
        // location only this machine has - one entry cited a relay file in `C:	emp`,
        // owned by neither repository and resolvable by no reviewer and no CI job (D239).
        //
        // A named external document is fine and is the ordinary case: "ISO C 7.21.6.5"
        // travels. What is refused is a path, absolute or relative, that has to exist
        // somewhere for the claim to be checkable.
        for fragment in self.cites.split_whitespace() {
            let looks_like_a_path = fragment.contains(":\\")
                || fragment.contains(":/")
                || fragment.starts_with('/')
                || fragment.starts_with("./")
                || fragment.starts_with("..");
            if looks_like_a_path {
                faults.push(format!(
                    "{}: cites a filesystem path ({fragment}) - a citation must name a document, not a location on one machine",
                    self.name
                ));
            }
        }
        if known.is_guess() && !self.cites.is_empty() {
            // Citing a source for something nobody has established is the precise
            // confusion this field exists to stop - it reads as evidence at a glance.
            faults.push(format!(
                "{}: known_by = assumed, so there is nothing to cite",
                self.name
            ));
        }
        faults
    }

    /// Every open question this entry admits, in the words a report prints.
    ///
    /// # Why the list is the definition and the count derives from it
    ///
    /// There were two definitions. This one added a whole-function penalty for an
    /// `assumed` entry **on top of** its itemised assumptions; `questions` counted the
    /// items and added the penalty only when nothing was itemised. So `knows` printed 80
    /// open questions and `questions` printed 70, of the same knowledge base, and neither
    /// line said which definition it meant (D239).
    ///
    /// The second rule is the right one and both comments already described it: an entry
    /// resting on a guess and listing nothing still counts as one, so a total cannot be
    /// shrunk by leaving the detail out - and an entry that *does* itemise is already
    /// counted by its items. Adding both charges the candid entry twice for being candid.
    ///
    /// Returning the list rather than a number is what stops it happening again: the
    /// count is `.len()` of this, so the two cannot disagree. The allocation is paid once
    /// per function in a report over ninety-five of them, which is not a path worth
    /// optimising into a second definition.
    pub fn open_questions_asked(&self) -> Vec<String> {
        if !self.assumptions.is_empty() {
            return self.assumptions.clone();
        }
        if self.known_by.is_some_and(Oracle::is_guess) {
            return vec![NOTHING_ESTABLISHED.to_owned()];
        }
        Vec::new()
    }

    /// How many separate things this entry admits it is guessing at.
    pub fn open_questions(&self) -> usize {
        self.open_questions_asked().len()
    }
}

/// What an entry admits when it rests on a guess and itemises nothing.
///
/// Here rather than in the reporting shim, because it is part of the definition of an open
/// question rather than a way of printing one - and the two counters disagreed precisely
/// because half the definition lived in the shim (D239).
pub const NOTHING_ESTABLISHED: &str = "Nothing about this entry has been established.";

/// Every label [`FunctionKnowledge::found_by`] may carry.
///
/// The same vocabulary `symbols/generated.json` serialises, because they are the same
/// claim about the same name. Spelling it out here rather than deriving it from
/// `orbistoun_nid::Method` is deliberate: the serialised form is a tagged enum with
/// per-variant fields, and this field is a bare string - a `Method` cannot be built from
/// one, but it can be compared against one, which is what matters (D213).
pub const FOUND_BY_LABELS: &[&str] = &[
    "published-standard",
    "generated",
    "static",
    "runtime",
    "supplied",
];

impl FunctionKnowledge {
    /// What is wrong with how this entry says its **name** was arrived at.
    ///
    /// Two checks, and the second is the one that matters.
    ///
    /// The label must be current vocabulary, which catches a value left behind by a change
    /// to it - eight entries still said `observed` after that value was split in two.
    ///
    /// And where `symbols/generated.json` holds a record for the same name, the two must
    /// **agree**. That file's records are re-run by CI rather than read; this field is
    /// hand-written and was checked by nothing, so when they differed the audited one was
    /// right every time. The direction varied, which is why both halves are reported: some
    /// entries sold this project's own work short, and one claimed a name came from
    /// *outside* the project that the generator produces (D213).
    ///
    /// Silent when the symbol database has no record. That is the normal state for an
    /// implemented function - its name is resolved by declaration, so it never enters the
    /// unnamed set a search records against. A gap, and named as one in `docs/BACKLOG.md`,
    /// rather than something to guess about here.
    #[must_use]
    pub fn name_provenance_faults(&self) -> Vec<String> {
        let mut faults = Vec::new();
        if self.found_by.is_empty() {
            return faults;
        }
        if !FOUND_BY_LABELS.contains(&self.found_by.as_str()) {
            faults.push(format!(
                "{}: found_by = {} is not one of {}",
                self.name,
                self.found_by,
                FOUND_BY_LABELS.join(", ")
            ));
            return faults;
        }
        if let Some(recorded) = audited_label(&self.name) {
            if recorded != self.found_by {
                faults.push(format!(
                    "{}: found_by = {} but symbols/generated.json re-derives it as {}",
                    self.name, self.found_by, recorded
                ));
            }
        }
        faults
    }
}

/// What the audited symbol database says produced a name, if it says anything.
///
/// Parsed once. The database is embedded in `orbistoun-nid` and is a few hundred
/// kilobytes; re-parsing it per entry would make a seventy-entry check quadratic in a
/// file that never changes during a run.
fn audited_label(name: &str) -> Option<&'static str> {
    use std::sync::OnceLock;
    static RECORDS: OnceLock<BTreeMap<String, &'static str>> = OnceLock::new();
    let records = RECORDS.get_or_init(|| {
        orbistoun_nid::SymbolDbFile::builtin()
            .derivations
            .into_iter()
            .map(|(name, derivation)| {
                let label = match derivation.method {
                    orbistoun_nid::Method::PublishedStandard { .. } => "published-standard",
                    orbistoun_nid::Method::Generated { .. } => "generated",
                    orbistoun_nid::Method::Static { .. } => "static",
                    orbistoun_nid::Method::Runtime { .. } => "runtime",
                    orbistoun_nid::Method::Supplied { .. } => "supplied",
                };
                (name, label)
            })
            .collect()
    });
    records.get(name).copied()
}

/// What kind of value a function hands back.
///
/// Coarse on purpose. The distinction that matters is whether the guest will
/// *dereference* the answer, and three categories cover it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Returns {
    /// Zero for success, non-zero for failure. An error code is the honest stub.
    Status,
    /// An address the guest will dereference. **Null is the honest stub** - it is what a
    /// real allocator or symbol lookup returns when it cannot do the job, guests already
    /// check for it, and a null dereference faults somewhere recognisable.
    Pointer,
    /// An opaque identifier the guest passes back rather than dereferences. Zero is the
    /// conventional "no such object".
    Handle,
    /// A count, a length, a size. Zero is the safe answer: a caller that loops over the
    /// result then does nothing, where a large value walks off the end of a buffer.
    Count,
}

impl Returns {
    /// What an unimplemented function of this kind should hand back.
    ///
    /// `None` means "the caller's usual error code" - only [`Returns::Status`] can carry
    /// one without being mistaken for data.
    pub const fn stub_value(self) -> Option<u64> {
        match self {
            Self::Status => None,
            // Every other kind is read as data by the caller, so the only safe answer is
            // the one the caller already tests for.
            Self::Pointer | Self::Handle | Self::Count => Some(0),
        }
    }
}

/// One library's knowledge file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KnowledgeFile {
    /// The library these functions belong to.
    #[serde(default)]
    pub library: String,
    /// The functions, in whatever order the file lists them.
    #[serde(default, rename = "function")]
    pub functions: Vec<FunctionKnowledge>,
}

/// One finding, in the shape something records it.
///
/// **Not a `FunctionKnowledge`.** That is the whole of what is known about a function; this
/// is what one turn or one command has to say about it, and the difference is that every
/// field here is optional in the sense that leaving it out means *"I have nothing to add"*
/// rather than *"it is empty"* (D292).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Record {
    /// The function this is about, bare.
    pub function: String,
    /// How many integer arguments, where that has been established.
    pub arity: Option<u8>,
    /// What it is for.
    pub purpose: Option<String>,
    /// Behaviour a reimplementation would otherwise get wrong. Appended, never replaced.
    pub edge_cases: Vec<String>,
    /// Guest modules it was seen in. Title ids, never paths.
    pub found_in: Vec<String>,
    /// How the behaviour here was established.
    pub known_by: Option<Oracle>,
    /// Where to look to check it.
    pub cites: Option<String>,
    /// Claims `known_by` does not cover, each one a question hardware could settle.
    pub assumptions: Vec<String>,
    /// Anything else worth keeping.
    pub note: Option<String>,
}

impl KnowledgeFile {
    /// Merges one record in, and says what is wrong with the result.
    ///
    /// **Merges rather than replaces.** A session recording one edge case must not have to
    /// restate what was established three sessions ago, and must not silently drop it either -
    /// so a field is written only when the record carries one, and lists append without
    /// duplicating.
    ///
    /// Returns the provenance faults rather than refusing: they are things to say to a
    /// person, and only the caller knows whether it is a command rejecting input or a loop
    /// declining to record. **An empty list means the entry is admissible**, not that nothing
    /// happened.
    pub fn merge(&mut self, record: &Record, today: &str) -> Vec<String> {
        let existing = self
            .functions
            .iter()
            .position(|f| f.name == record.function);
        let mut entry = existing.map_or_else(
            || FunctionKnowledge {
                name: record.function.clone(),
                found_on: today.to_owned(),
                ..FunctionKnowledge::default()
            },
            |i| self.functions[i].clone(),
        );

        if let Some(arity) = record.arity {
            entry.arity = Some(arity);
        }
        if let Some(purpose) = &record.purpose {
            purpose.clone_into(&mut entry.purpose);
        }
        if let Some(note) = &record.note {
            note.clone_into(&mut entry.note);
        }
        if let Some(known) = record.known_by {
            entry.known_by = Some(known);
        }
        if let Some(cites) = &record.cites {
            cites.clone_into(&mut entry.cites);
        }
        for edge in &record.edge_cases {
            if !entry.edge_cases.contains(edge) {
                entry.edge_cases.push(edge.clone());
            }
        }
        for title in &record.found_in {
            if !entry.found_in.contains(title) {
                entry.found_in.push(title.clone());
            }
        }
        for assumption in &record.assumptions {
            if !entry.assumptions.contains(assumption) {
                entry.assumptions.push(assumption.clone());
            }
        }

        let faults = entry.provenance_faults();
        match existing {
            Some(i) => self.functions[i] = entry,
            None => self.functions.push(entry),
        }
        faults
    }

    /// Parses one file.
    pub fn parse(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }

    /// Renders it back, sorted by name.
    ///
    /// Sorted so an appended entry produces a diff showing what was learned rather than
    /// where it happened to land.
    pub fn render(&self) -> Result<String, toml::ser::Error> {
        let mut sorted = self.clone();
        sorted.functions.sort_by(|a, b| a.name.cmp(&b.name));
        toml::to_string_pretty(&sorted)
    }
}

/// Everything known, across every library.
#[derive(Debug, Clone, Default)]
pub struct Knowledge {
    by_name: BTreeMap<String, FunctionKnowledge>,
    library_of: BTreeMap<String, String>,
}

impl Knowledge {
    /// Loads what ships with the tool.
    ///
    /// # Panics
    ///
    /// If a shipped file is malformed, which a test in this crate rules out.
    pub fn builtin() -> Self {
        let mut out = Self::default();
        for (library, text) in EMBEDDED {
            let file = KnowledgeFile::parse(text)
                .unwrap_or_else(|e| panic!("shipped knowledge file {library} is malformed: {e}"));
            out.absorb(library, &file);
        }
        out
    }

    /// Merges one file in.
    ///
    /// A later entry for the same name wins, so a user file can correct a shipped one
    /// without editing it.
    pub fn absorb(&mut self, library: &str, file: &KnowledgeFile) {
        let library = if file.library.is_empty() {
            library
        } else {
            &file.library
        };
        for function in &file.functions {
            self.library_of
                .insert(function.name.clone(), library.to_owned());
            self.by_name.insert(function.name.clone(), function.clone());
        }
    }

    /// What is known about a function, if anything.
    pub fn get(&self, name: &str) -> Option<&FunctionKnowledge> {
        self.by_name.get(name)
    }

    /// Which library a function belongs to.
    pub fn library_of(&self, name: &str) -> Option<&str> {
        self.library_of.get(name).map(String::as_str)
    }

    /// Every function known, by name.
    pub fn functions(&self) -> impl Iterator<Item = &FunctionKnowledge> {
        self.by_name.values()
    }

    /// How many entries hold something beyond a name.
    ///
    /// The honest progress measure for this file: entries are cheap, understanding is
    /// not, and a count of names would flatter both equally.
    pub fn understood(&self) -> usize {
        self.by_name.values().filter(|f| !f.is_bare()).count()
    }

    /// How many entries rest on a given oracle.
    ///
    /// The shape of what is known, rather than the size of it. Two hundred entries all
    /// resting on [`Oracle::Assumed`] and two hundred measured on hardware are the same
    /// number and completely different projects.
    pub fn resting_on(&self, oracle: Oracle) -> usize {
        self.by_name
            .values()
            .filter(|f| f.known_by == Some(oracle))
            .count()
    }

    /// Every separate thing this project admits it is guessing at.
    ///
    /// **The number to watch.** It is expected to go *up* as more is written down - an
    /// assumption only appears here once someone notices it - and then down as hardware
    /// answers them. A total that only ever falls is measuring candour, not knowledge.
    pub fn open_questions(&self) -> usize {
        self.by_name
            .values()
            .map(FunctionKnowledge::open_questions)
            .sum()
    }

    /// Entries whose provenance does not add up, in words.
    ///
    /// Empty is the passing state, and a test in this crate holds the shipped files to it.
    pub fn provenance_faults(&self) -> Vec<String> {
        self.by_name
            .values()
            .flat_map(FunctionKnowledge::provenance_faults)
            .collect()
    }

    /// How many are recorded at all.
    pub fn len(&self) -> usize {
        self.by_name.len()
    }

    /// Whether nothing is recorded.
    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::{Knowledge, KnowledgeFile, Oracle, Record};

    /// Recording one thing does not erase what was recorded before it.
    ///
    /// **The rule the merge exists for.** A session that notes an edge case must not have to
    /// restate a purpose established three sessions ago, and must not silently drop it - so a
    /// field is written only when the record carries one, and lists append (D292).
    #[test]
    fn a_later_record_adds_without_erasing_an_earlier_one() {
        let mut file = KnowledgeFile {
            library: "libkernel".to_owned(),
            functions: Vec::new(),
        };
        let faults = file.merge(
            &Record {
                function: "sceFoo".to_owned(),
                purpose: Some("reserves a range".to_owned()),
                edge_cases: vec!["writes arg0".to_owned()],
                known_by: Some(Oracle::GuestObserved),
                ..Record::default()
            },
            "2026-08-26",
        );
        assert!(faults.is_empty(), "{faults:?}");

        // A second record mentioning only an edge case.
        let faults = file.merge(
            &Record {
                function: "sceFoo".to_owned(),
                edge_cases: vec!["must answer zero first".to_owned()],
                ..Record::default()
            },
            "2026-08-26",
        );
        assert!(faults.is_empty(), "{faults:?}");

        assert_eq!(file.functions.len(), 1, "one function, merged");
        let entry = &file.functions[0];
        assert_eq!(entry.purpose, "reserves a range", "the purpose survived");
        assert_eq!(entry.edge_cases.len(), 2, "and the edge case was added");
        assert_eq!(entry.known_by, Some(Oracle::GuestObserved));
    }

    /// A record claiming behaviour without saying how it is known is refused.
    ///
    /// **The negative half, and the one the vocabulary exists for.** Every available default
    /// would be a lie - `assumed` understates work really done, anything stronger overstates
    /// it - so the merge reports the fault and the caller refuses (D180).
    #[test]
    fn behaviour_recorded_without_a_provenance_is_a_fault() {
        let mut file = KnowledgeFile {
            library: "libkernel".to_owned(),
            functions: Vec::new(),
        };
        let faults = file.merge(
            &Record {
                function: "sceFoo".to_owned(),
                purpose: Some("does something".to_owned()),
                ..Record::default()
            },
            "2026-08-26",
        );
        assert!(
            !faults.is_empty(),
            "a claim about behaviour with no oracle behind it must not be admissible"
        );
    }

    /// An empty record is not a claim, so it is not refused either.
    #[test]
    fn a_bare_name_needs_no_provenance() {
        let mut file = KnowledgeFile {
            library: "libkernel".to_owned(),
            functions: Vec::new(),
        };
        let faults = file.merge(
            &Record {
                function: "sceFoo".to_owned(),
                ..Record::default()
            },
            "2026-08-26",
        );
        assert!(
            faults.is_empty(),
            "recording that it exists claims nothing: {faults:?}"
        );
    }

    #[test]
    fn a_found_by_that_contradicts_the_symbol_database_is_a_fault() {
        // Made to fail on purpose. A guard nobody has watched fail is a guard nobody knows
        // anything about - three in this repository reported success while checking nothing
        // (D191, D199, D213).
        let mut entry = super::FunctionKnowledge {
            name: "memcpy".to_owned(),
            found_by: "supplied".to_owned(),
            ..Default::default()
        };
        // `memcpy` is in the shipped published-standard list, so the audited record and
        // this claim disagree - and `supplied` is the direction that matters, because it
        // says this project did not derive a name it demonstrably derives.
        let faults = entry.name_provenance_faults();
        assert_eq!(
            faults.len(),
            1,
            "expected exactly one fault, got {faults:?}"
        );
        assert!(faults[0].contains("published-standard"), "{faults:?}");

        entry.found_by = "published-standard".to_owned();
        assert!(
            entry.name_provenance_faults().is_empty(),
            "agreeing with the audited record is not a fault"
        );

        // And a label left behind by a change to the vocabulary is caught even when the
        // symbol database has nothing to compare against. Eight entries said `observed`
        // after that value was split into `static` and `runtime`.
        entry.name = "sceSomethingNotInTheDatabase".to_owned();
        entry.found_by = "observed".to_owned();
        assert!(
            entry.name_provenance_faults()[0].contains("is not one of"),
            "a retired label must be rejected on its own, with no database record needed"
        );

        // An entry that says nothing about its name claims nothing, and is not a fault.
        entry.found_by = String::new();
        assert!(entry.name_provenance_faults().is_empty());
    }

    #[test]
    fn the_shipped_files_parse_and_carry_real_content() {
        // They are embedded, so a typo breaks the build for everyone and would otherwise
        // surface as a panic at startup.
        let k = Knowledge::builtin();
        assert!(!k.is_empty(), "something should ship");
        assert!(
            k.understood() > 0,
            "at least one entry should say more than a name"
        );
    }

    #[test]
    fn every_oracle_is_falsifiable() {
        // **The property the whole field rests on.** Each value is either checkable
        // against a named outside source or answerable by a probe on real hardware. A
        // value meaning "the model recalled it" would satisfy neither, which is why there
        // is no such value and why this test would fail if somebody added one.
        for oracle in [
            Oracle::Published,
            Oracle::Measured,
            Oracle::GuestObserved,
            Oracle::Assumed,
        ] {
            assert!(
                oracle.needs_citation() || oracle.is_probeable(),
                "{} can be neither cited nor probed, so nothing could ever contradict it",
                oracle.label()
            );
        }
    }

    #[test]
    fn recording_behaviour_requires_saying_how_it_is_known() {
        // The entry an unattended agent produces by default: a confident arity, a return
        // kind, and no account of where either came from.
        let file = KnowledgeFile::parse(
            r#"
            [[function]]
            name = "confident"
            arity = 3
            returns = "status"
            purpose = "does a thing"
            "#,
        )
        .expect("parse");
        let mut k = Knowledge::default();
        k.absorb("libTest", &file);

        let faults = k.provenance_faults();
        assert_eq!(faults.len(), 1, "{faults:?}");
        assert!(faults[0].contains("confident"));
    }

    #[test]
    fn a_name_alone_needs_no_source() {
        // Recording that a function exists is not a claim about what it does, and
        // demanding provenance for it would make the honest act the expensive one.
        let file = KnowledgeFile::parse(
            r#"
            [[function]]
            name = "seen_only"
            found_by = "generated"
            "#,
        )
        .expect("parse");
        let mut k = Knowledge::default();
        k.absorb("libTest", &file);

        assert!(k.provenance_faults().is_empty());
    }

    #[test]
    fn an_outside_source_has_to_be_checkable() {
        // "It is in the standard" without saying where is indistinguishable from a guess
        // by anyone who was not there - and it *reads* as evidence, which is worse than
        // an honest admission of not knowing.
        let file = KnowledgeFile::parse(
            r#"
            [[function]]
            name = "vague"
            arity = 2
            known_by = "published"
            "#,
        )
        .expect("parse");
        let mut k = Knowledge::default();
        k.absorb("libTest", &file);

        assert_eq!(k.provenance_faults().len(), 1);

        // The same entry, checkable.
        let cited = KnowledgeFile::parse(
            r#"
            [[function]]
            name = "vague"
            arity = 2
            known_by = "published"
            cites = "ISO C 7.24.2.1"
            "#,
        )
        .expect("parse");
        let mut k = Knowledge::default();
        k.absorb("libTest", &cited);
        assert!(k.provenance_faults().is_empty());
    }

    #[test]
    fn a_guess_cites_nothing() {
        // Citing a source for something nobody established is the precise confusion this
        // field exists to remove: at a glance it looks like the entry above.
        let file = KnowledgeFile::parse(
            r#"
            [[function]]
            name = "dressed_up"
            arity = 1
            known_by = "assumed"
            cites = "ISO C 7.24.2.1"
            "#,
        )
        .expect("parse");
        let mut k = Knowledge::default();
        k.absorb("libTest", &file);

        assert_eq!(k.provenance_faults().len(), 1);
    }

    #[test]
    fn open_questions_cannot_be_reduced_by_leaving_the_detail_out() {
        // An entry rating itself `assumed` and listing nothing is not better understood
        // than one that spells its uncertainty out; it is the same guess, less usefully
        // written. Counting only the listed lines would reward the vaguer entry.
        let file = KnowledgeFile::parse(
            r#"
            [[function]]
            name = "silent_guess"
            arity = 1
            known_by = "assumed"

            [[function]]
            name = "spelled_out"
            arity = 1
            known_by = "assumed"
            assumptions = ["the error code at truncation", "whether it writes on failure"]
            "#,
        )
        .expect("parse");
        let mut k = Knowledge::default();
        k.absorb("libTest", &file);

        // One for the entry that admits it knows nothing and lists nothing, and two for
        // the entry that spelled its two out. **Not three for the second**: charging it a
        // whole-function penalty *plus* its items counts the candid entry twice for being
        // candid, which is what made `knows` say 80 and `questions` say 70 (D239).
        assert_eq!(k.get("silent_guess").expect("present").open_questions(), 1);
        assert_eq!(k.get("spelled_out").expect("present").open_questions(), 2);
        assert_eq!(k.open_questions(), 3);

        // The property the test is named for still holds: leaving the detail out does not
        // lower the count below the one it would otherwise carry.
        assert!(
            k.get("silent_guess").expect("present").open_questions() >= 1,
            "a silent guess still costs one"
        );
    }

    /// The count and the list are the same answer, for every shape an entry can take.
    ///
    /// This is the test that was missing. Two counters computed the same quantity two
    /// ways, disagreed by ten, and both printed their number without saying which
    /// definition it was - inside the machinery whose whole purpose is to stop a claim
    /// being reported more confidently than it is held (D239).
    #[test]
    fn the_question_count_always_matches_the_questions_listed() {
        let file = KnowledgeFile::parse(
            r#"
            [[function]]
            name = "assumed_and_itemised"
            arity = 1
            known_by = "assumed"
            assumptions = ["one", "two"]

            [[function]]
            name = "assumed_and_silent"
            arity = 1
            known_by = "assumed"

            [[function]]
            name = "published_and_itemised"
            arity = 1
            known_by = "published"
            cites = "ISO C 7.21.6.5"
            assumptions = ["one"]

            [[function]]
            name = "published_and_certain"
            arity = 1
            known_by = "published"
            cites = "ISO C 7.21.6.5"
            "#,
        )
        .expect("parse");
        let mut k = Knowledge::default();
        k.absorb("libTest", &file);

        let mut listed = 0;
        for f in k.functions() {
            assert_eq!(
                f.open_questions(),
                f.open_questions_asked().len(),
                "{} counts differently from what it lists",
                f.name
            );
            listed += f.open_questions_asked().len();
        }
        assert_eq!(
            k.open_questions(),
            listed,
            "the total is not the sum listed"
        );
        assert_eq!(listed, 4, "2 itemised + 1 silent guess + 1 itemised");

        // And a certain entry asks nothing, so an empty queue means an empty queue.
        assert!(
            k.get("published_and_certain")
                .expect("present")
                .open_questions_asked()
                .is_empty()
        );
    }

    #[test]
    fn a_partly_measured_entry_still_carries_its_open_questions() {
        // The normal case, and the reason provenance is not one flat field per function:
        // the shape comes from the standard and one edge is a guess. A single value would
        // have to round that up or down, and rounding up is how a guess becomes a fact.
        let file = KnowledgeFile::parse(
            r#"
            [[function]]
            name = "mixed"
            arity = 4
            known_by = "published"
            cites = "ISO C 7.21.6.5"
            assumptions = ["what the vendor's bounds-checked variant returns on truncation"]
            "#,
        )
        .expect("parse");
        let mut k = Knowledge::default();
        k.absorb("libTest", &file);

        assert!(k.provenance_faults().is_empty(), "the entry is well formed");
        assert_eq!(k.open_questions(), 1, "and still admits one open question");
        assert_eq!(k.resting_on(Oracle::Published), 1);
    }

    #[test]
    fn the_shipped_files_account_for_everything_they_claim() {
        // Held here rather than only in CI so it fails at the moment somebody writes the
        // entry, not an hour later on a runner.
        let faults = Knowledge::builtin().provenance_faults();
        assert!(faults.is_empty(), "{faults:#?}");
    }

    #[test]
    fn what_was_measured_about_direct_memory_query_survived() {
        // Every fact here cost an experiment (D083). If this test ever fails, the
        // knowledge was lost and the experiments have to be repeated.
        let k = Knowledge::builtin();
        let f = k
            .get("sceKernelDirectMemoryQuery")
            .expect("the most-called function in every title should be recorded");

        assert_eq!(f.arity, Some(4), "measured from the guest");
        assert_eq!(f.arguments.len(), 4);
        assert!(
            f.edge_cases.iter().any(|e| e.contains("return")),
            "the guest ignoring the return value is the single most surprising fact"
        );
        assert!(!f.found_in.is_empty(), "which title it was seen in");
        assert_eq!(
            k.library_of("sceKernelDirectMemoryQuery"),
            Some("libkernel")
        );
    }

    #[test]
    fn an_unknown_arity_is_distinct_from_zero() {
        // Zero is a real answer that a trace renders very differently from "we have no
        // idea", and collapsing them would make an unmeasured function look measured.
        let file = KnowledgeFile::parse(
            r#"
            library = "libTest"
            [[function]]
            name = "unmeasured"
            [[function]]
            name = "measured"
            arity = 0
            "#,
        )
        .expect("parse");
        let mut k = Knowledge::default();
        k.absorb("libTest", &file);

        assert_eq!(k.get("unmeasured").expect("present").arity, None);
        assert_eq!(k.get("measured").expect("present").arity, Some(0));
    }

    #[test]
    fn a_bare_entry_counts_as_recorded_but_not_as_understood() {
        let file = KnowledgeFile::parse(
            r#"
            [[function]]
            name = "seen_only"
            found_by = "generated"
            "#,
        )
        .expect("parse");
        let mut k = Knowledge::default();
        k.absorb("libTest", &file);

        assert_eq!(k.len(), 1);
        assert_eq!(k.understood(), 0, "a name alone is not understanding");
    }

    #[test]
    fn a_later_file_can_correct_an_earlier_one() {
        // So a user file overrides a shipped one without editing it.
        let mut k = Knowledge::default();
        k.absorb(
            "libTest",
            &KnowledgeFile::parse("[[function]]\nname = \"f\"\narity = 1\n").expect("parse"),
        );
        k.absorb(
            "libTest",
            &KnowledgeFile::parse("[[function]]\nname = \"f\"\narity = 6\n").expect("parse"),
        );
        assert_eq!(k.get("f").expect("present").arity, Some(6));
    }

    #[test]
    fn a_rendered_file_round_trips_and_comes_back_sorted() {
        // Appending is a supported operation, so what is written must read back - and
        // sorting keeps a diff about what was learned rather than where it landed.
        let file = KnowledgeFile::parse(
            r#"
            library = "libTest"
            [[function]]
            name = "zeta"
            arity = 2
            [[function]]
            name = "alpha"
            purpose = "does a thing"
            edge_cases = ["refuses a null destination"]
            "#,
        )
        .expect("parse");

        let text = file.render().expect("render");
        let back = KnowledgeFile::parse(&text).expect("reparse");
        assert_eq!(back.functions.len(), 2);
        assert_eq!(back.functions[0].name, "alpha", "sorted by name");
        assert_eq!(back.functions[0].edge_cases.len(), 1);
        assert_eq!(back.functions[1].arity, Some(2));
    }

    #[test]
    fn every_open_question_belongs_to_a_function_that_can_be_asked_about() {
        // The queue a probe works from is only as good as its join key. A question whose
        // function cannot be found in the knowledge base is one nothing can record the
        // answer against, so it would be asked and then lost.
        let k = Knowledge::builtin();
        for f in k.functions() {
            if !f.assumptions.is_empty() {
                assert!(
                    k.get(&f.name).is_some(),
                    "{} carries questions but cannot be looked up",
                    f.name
                );
                assert!(
                    k.library_of(&f.name).is_some(),
                    "{} carries questions but belongs to no library",
                    f.name
                );
            }
        }
    }

    #[test]
    fn a_question_is_never_recorded_against_something_already_measured() {
        // `measured` means hardware answered it. An entry claiming that *and* listing an
        // open question is contradicting itself, and the queue would send a probe to
        // re-ask something already settled.
        for f in Knowledge::builtin().functions() {
            if f.known_by == Some(Oracle::Measured) {
                assert!(
                    f.assumptions.is_empty(),
                    "{} claims to be measured yet still lists open questions",
                    f.name
                );
            }
        }
    }

    #[test]
    fn the_open_question_count_matches_what_the_entries_carry() {
        // `open_questions` is the number reported to a person and used to rank work. If it
        // could drift from the entries, the queue and the summary would disagree about how
        // much is unknown - and the summary is the one people believe.
        //
        // **This test used to re-implement the counting rule and assert the sum agreed** -
        // a third copy of the definition, guarding the other two. It passed for as long as
        // two of the three matched, and they did, while the number a person read was wrong
        // by ten (D239). Summed from what each entry would print instead.
        let k = Knowledge::builtin();
        let listed: usize = k.functions().map(|f| f.open_questions_asked().len()).sum();
        assert_eq!(k.open_questions(), listed);

        // The invariant that actually has teeth, checked against the real knowledge base:
        // an entry that itemises contributes exactly its items, never its items plus a
        // whole-function penalty for having been candid about resting on a guess.
        for f in k.functions() {
            if !f.assumptions.is_empty() {
                assert_eq!(
                    f.open_questions(),
                    f.assumptions.len(),
                    "{} is charged for itemising",
                    f.name
                );
            }
        }
    }
}
