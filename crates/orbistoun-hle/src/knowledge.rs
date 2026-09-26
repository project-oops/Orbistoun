//! What is known about guest functions, as opposed to what a tool worked out.
//!
//! Derived facts (which pattern generated a name, at which index) live in
//! `symbols/generated.json`, which every search overwrites. Known facts (arity, argument meaning,
//! purpose, edge behaviour) come only from observation and are kept here, accumulated and never
//! regenerated (D122). This is the output of the development loop, written by tooling as much as
//! by hand. The NID is not stored: it is derived from the name, so no entry can disagree with
//! itself.

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
        "libkernel_sync_on_address",
        include_str!("../data/knowledge/libkernel_sync_on_address.toml"),
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
    // The rest, in directory order. The guard test below checks that every file on disk is listed,
    // so none is written and never loaded.
    (
        "libSceAgcDriver",
        include_str!("../data/knowledge/libSceAgcDriver.toml"),
    ),
    (
        "libSceAppContent",
        include_str!("../data/knowledge/libSceAppContent.toml"),
    ),
    (
        "libSceCommonDialog",
        include_str!("../data/knowledge/libSceCommonDialog.toml"),
    ),
    (
        "libSceCoredump",
        include_str!("../data/knowledge/libSceCoredump.toml"),
    ),
    (
        "libSceErrorDialog",
        include_str!("../data/knowledge/libSceErrorDialog.toml"),
    ),
    (
        "libSceJson2",
        include_str!("../data/knowledge/libSceJson2.toml"),
    ),
    (
        "libSceKeyboard",
        include_str!("../data/knowledge/libSceKeyboard.toml"),
    ),
    (
        "libSceMouse",
        include_str!("../data/knowledge/libSceMouse.toml"),
    ),
    (
        "libSceNet",
        include_str!("../data/knowledge/libSceNet.toml"),
    ),
    (
        "libSceNetCtl",
        include_str!("../data/knowledge/libSceNetCtl.toml"),
    ),
    (
        "libSceSaveData_native",
        include_str!("../data/knowledge/libSceSaveData_native.toml"),
    ),
    (
        "libSceUserService",
        include_str!("../data/knowledge/libSceUserService.toml"),
    ),
    (
        "libSceVideoRecording",
        include_str!("../data/knowledge/libSceVideoRecording.toml"),
    ),
    (
        "libkernel_unity",
        include_str!("../data/knowledge/libkernel_unity.toml"),
    ),
];

/// One argument, as far as it is understood.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Argument {
    /// What to call it. Empty when only its position is known.
    #[serde(default)]
    pub name: String,
    /// Its shape - `u64`, `ptr`, `u32`, and so on. Loose on purpose: a width is worth recording,
    /// a guessed C type is not.
    #[serde(default)]
    pub kind: String,
    /// Anything a reader would want and cannot infer.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

/// How a behavioural claim was established.
///
/// [`FunctionKnowledge::found_by`] covers the name; this covers behaviour (an arity, a return
/// kind, an edge), which changes what the emulator does. Every value is falsifiable and none
/// means "known from experience", so recording a fact commits to a checkable source rather than
/// absorbing a recalled one (D180). It answers licence, quality and "which facts are guesses".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Oracle {
    /// A published standard or published source: ISO C, POSIX, or the FreeBSD tree the
    /// target C library derives from. The strongest reference available, and citable.
    Published,
    /// Run against a published implementation of the same interface, and they agreed.
    ///
    /// Not a measurement of the target: it shows orbistoun implements the analogue as that
    /// implementation does, and the hardware may still differ. Stronger than [`Self::Published`],
    /// weaker than [`Self::Measured`], and probeable (D478).
    Differential,
    /// Measured on real hardware by a conformance probe: nobody else's work, and one run answers a
    /// batch of questions.
    Measured,
    /// The guest itself - it proceeded when answered this way, and stopped otherwise.
    ///
    /// One bit per boot, and the bit is consistency, not correctness: enough to rule things out,
    /// never to confirm.
    GuestObserved,
    /// Nobody knows. The value recorded is a placeholder chosen to be least harmful.
    ///
    /// Common and not a failure: an assumption written down can be counted, ranked, probed and
    /// retired.
    Assumed,
}

impl Oracle {
    /// How it is written in a file, and in a report.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Published => "published",
            Self::Differential => "differential",
            Self::Measured => "measured",
            Self::GuestObserved => "guest-observed",
            Self::Assumed => "assumed",
        }
    }

    /// Whether this claims support from something outside this repository, and so must cite it:
    /// an uncheckable claim of external support is worth less than an honest [`Oracle::Assumed`].
    pub const fn needs_citation(self) -> bool {
        matches!(self, Self::Published | Self::Differential | Self::Measured)
    }

    /// Whether the claim rests on nothing yet.
    pub const fn is_guess(self) -> bool {
        matches!(self, Self::Assumed)
    }

    /// Whether an answer with this provenance is knowledge, or a prop holding a run up.
    ///
    /// An answer from the target or a published implementation is the emulator being right, so a
    /// run using it is honest. [`Self::GuestObserved`] is a prop: answers were tried until the guest
    /// moved. Distinct from [`Self::needs_citation`], which asks whether a claim owes a source; the
    /// two cover the same values but answer different questions (D557).
    pub const fn is_evidence(self) -> bool {
        matches!(self, Self::Published | Self::Differential | Self::Measured)
    }

    /// Whether a conformance probe on real hardware could settle it, which makes the assumption
    /// count a worklist.
    pub const fn is_probeable(self) -> bool {
        // `Differential` is here on purpose: the hardware may disagree with FreeBSD.
        matches!(
            self,
            Self::Assumed | Self::GuestObserved | Self::Differential
        )
    }
}

/// Everything known about one guest function.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionKnowledge {
    /// The symbol name, exactly as the guest imports it.
    pub name: String,
    /// How many integer arguments it takes, where that has been established.
    ///
    /// `None` means unknown, not zero; a trace renders zero differently.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arity: Option<u8>,
    /// What the function is for, in prose.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub purpose: String,
    /// What kind of value it hands back.
    ///
    /// Decides what an unimplemented function answers: an error code suits a status return and is a
    /// wild pointer for a handle the guest dereferences (D125).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub returns: Option<Returns>,
    /// A block of guest memory to reserve and hand this function, and how it arrives.
    ///
    /// A region is reserved before the guest starts, not at the call, since a trampoline on the
    /// guest's stack is the wrong layer to allocate from (D300). A function that returns a pointer
    /// to memory it owns (`sceAgcGetRegisterDefaults2` returns a descriptor read at `+0x38`) records
    /// it here as the shipped, `assumed` home; the fresh region is zero-filled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<crate::StubRegion>,
    /// The arguments, in register order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arguments: Vec<Argument>,
    /// What this function does not do, when it is implemented but not completely.
    ///
    /// A field rather than a comment so the gap report can count it: "implemented" and
    /// "implemented with these edges missing" are different states. Empty means no incompleteness
    /// is declared, which is not the same as complete.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub partial: String,

    /// Behaviour a reimplementation would otherwise get wrong; each entry cost an experiment.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edge_cases: Vec<String>,
    /// Experiments that would answer this entry's open questions, by name.
    ///
    /// A label is a deliberate claim that this experiment would settle it, which the dispatcher can
    /// act on; classifying the prose question would be guesswork (D356).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub answerable_by: Vec<String>,
    /// How the name was arrived at - one of the labels in [`FOUND_BY_LABELS`].
    ///
    /// A hand-written copy of what `symbols/generated.json` records and CI re-runs.
    /// [`Knowledge::provenance_faults`] checks that the label is current vocabulary and, where the
    /// symbol database has a record for the name, that the two agree (D213). Implemented functions
    /// have no record there, since their names never enter the unnamed set.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub found_by: String,
    /// How the behaviour recorded above was established; separate from [`Self::found_by`], since a
    /// standard name can carry an unchecked return value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub known_by: Option<Oracle>,
    /// Where to look to check it - a standard clause, a source file and revision, a probe.
    ///
    /// Required by [`Oracle::needs_citation`]. Free text, since the sources are not uniform.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cites: String,
    /// Claims inside this entry that [`Self::known_by`] does not cover.
    ///
    /// The mixed entry is normal: shape from the standard, arity measured, one edge a guess. Each
    /// line is a question a hardware probe could answer.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assumptions: Vec<String>,
    /// Which guest modules it was seen in: title identifiers only, never paths, since modules are
    /// never tracked.
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
    /// Whether anything beyond the name is known; the count of bare entries is the size of the job.
    pub fn is_bare(&self) -> bool {
        self.arity.is_none()
            && self.purpose.is_empty()
            && self.arguments.is_empty()
            && self.edge_cases.is_empty()
            // A region is a behaviour claim, so it carries a provenance like any other.
            && self.region.is_none()
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
    /// Returned rather than asserted, so one caller can fail a build and another can show a person.
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
        // A citation naming a filesystem path is not a citation: `cites` exists so somebody else can
        // check a claim, and a path exists only on one machine. A named document ("ISO C 7.21.6.5")
        // is the ordinary case.
        for fragment in self.cites.split_whitespace() {
            if fragment_is_a_path(fragment) {
                faults.push(format!(
                    "{}: cites a filesystem path ({fragment}) - a citation must name a document, not a location on one machine",
                    self.name
                ));
            }
        }
        if known.is_guess() && !self.cites.is_empty() {
            // Citing a source for something nobody established reads as evidence at a glance.
            faults.push(format!(
                "{}: known_by = assumed, so there is nothing to cite",
                self.name
            ));
        }
        faults
    }

    /// Every open question this entry admits, in the words a report prints.
    ///
    /// Itemised assumptions count as themselves; an entry resting on a guess and listing nothing
    /// counts as one, so a total cannot be shrunk by leaving detail out. The count is `.len()` of
    /// this list, so the two cannot disagree.
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

/// What an entry admits when it rests on a guess and itemises nothing; part of the definition
/// of an open question, so it lives here.
pub const NOTHING_ESTABLISHED: &str = "Nothing about this entry has been established.";

/// What a POSIX-named delegation admits, in the one wording all of them use.
///
/// These names resolve to the vendor-named function beside them (D349). The target's name is
/// kept out of the sentence so [`shared_premises`] groups every delegation as one premise; the
/// name is an `edge_cases` line. Shared with `orbistoun-gen`, which writes these entries.
pub const DELEGATION_ASSUMPTION: &str = "That this library's POSIX spelling and the vendor-named function it resolves to are the same behaviour on the target rather than merely similar. Unmeasured - it is inferred from the names and from both being exported by one platform.";

/// A question several entries ask in the same words, and every entry that rests on it.
///
/// Grouped by word-for-word identity, not by judgement; see [`shared_premises`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedPremise {
    /// The question, in the wording most of its entries use.
    pub question: String,
    /// The entries asking it, in the order they were supplied.
    pub functions: Vec<String>,
    /// How many distinct wordings were collapsed into this one.
    ///
    /// Above one means entries punctuate one sentence differently, a defect in the knowledge base;
    /// gated by a test over the shipped data (D538).
    pub wordings: usize,
}

/// The words of a question, lowercased, with everything else dropped: the grouping key, so
/// `shape. The` and `shape; the` agree and a single differing word does not.
fn words_of(question: &str) -> String {
    let mut key = String::with_capacity(question.len());
    let mut between = false;
    for character in question.chars() {
        if character.is_alphanumeric() {
            if between && !key.is_empty() {
                key.push(' ');
            }
            between = false;
            key.extend(character.to_lowercase());
        } else {
            between = true;
        }
    }
    key
}

/// The entries asking one premise, and each wording of it with how many entries used it.
type PremiseGroup = (Vec<String>, Vec<(String, usize)>);
/// Group `asked` - pairs of function name and question - by the premise they share.
///
/// Deduplication, not classification: questions group only when they are the same sequence of
/// words, forgiving punctuation, case and spacing. Nothing reads what a question means. Groups
/// come in first-seen order so output is stable; ranking is the caller's.
#[must_use]
pub fn shared_premises(asked: &[(String, String)]) -> Vec<SharedPremise> {
    let mut index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut groups: Vec<PremiseGroup> = Vec::new();

    for (function, question) in asked {
        let at = *index.entry(words_of(question)).or_insert_with(|| {
            groups.push((Vec::new(), Vec::new()));
            groups.len() - 1
        });
        let (functions, wordings) = &mut groups[at];
        functions.push(function.clone());
        if let Some(seen) = wordings.iter_mut().find(|(text, _)| text == question) {
            seen.1 += 1;
        } else {
            wordings.push((question.clone(), 1));
        }
    }

    groups
        .into_iter()
        .map(|(functions, mut wordings)| {
            // Commonest wording, then alphabetical: a total order, so runs print the same sentence.
            wordings.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            SharedPremise {
                question: wordings
                    .first()
                    .map(|(text, _)| text.clone())
                    .unwrap_or_default(),
                functions,
                wordings: wordings.len(),
            }
        })
        .collect()
}

/// Every label [`FunctionKnowledge::found_by`] may carry.
///
/// The vocabulary `symbols/generated.json` serialises. Spelled out rather than derived from
/// `orbistoun_nid::Method`, a tagged enum that cannot be built from a bare string but can be
/// compared with one (D213).
pub const FOUND_BY_LABELS: &[&str] = &[
    "published-standard",
    "generated",
    // Derived from a held name by a rule in the affix file; recorded apart from `generated`, a
    // different claim (D606).
    "affixed",
    "static",
    "runtime",
    "supplied",
];

impl FunctionKnowledge {
    /// What is wrong with how this entry says its name was arrived at.
    ///
    /// The label must be current vocabulary, and where `symbols/generated.json` holds a record for
    /// the name, the two must agree; that file is re-run by CI, so it wins (D213). Silent when the
    /// database has no record, the normal state for an implemented function.
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

/// Whether one whitespace-delimited piece of a citation is a filesystem path.
///
/// Exposed so a generator applies the same rule. Per fragment: a slash inside a word is ordinary
/// (`ISO/IEC 9899`), and a fragment beginning with one is a path.
#[must_use]
pub fn fragment_is_a_path(fragment: &str) -> bool {
    fragment.contains(":\\")
        || fragment.contains(":/")
        || fragment.starts_with('/')
        || fragment.starts_with("./")
        || fragment.starts_with("..")
}

/// Whether a whole citation names a location on one machine rather than a document.
#[must_use]
pub fn citation_is_a_path(cites: &str) -> bool {
    cites.split_whitespace().any(fragment_is_a_path)
}

/// What the audited symbol database says produced a name, if it says anything; parsed once.
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
                    orbistoun_nid::Method::Affixed { .. } => "affixed",
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
/// Coarse on purpose: what matters is whether the guest will dereference the answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Returns {
    /// Zero for success, non-zero for failure. An error code is the honest stub.
    Status,
    /// An address the guest will dereference. Null is the honest stub: it is what a real allocator
    /// or lookup returns on failure, guests check for it, and a null dereference faults visibly.
    Pointer,
    /// An opaque identifier the guest passes back rather than dereferences. Zero is the
    /// conventional "no such object".
    Handle,
    /// A count, a length, a size. Zero is the safe answer: a loop over it does nothing.
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
            // Every other kind is read as data, so the only safe answer is the one the caller tests for.
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
/// Not a `FunctionKnowledge`: every field is optional in the sense that leaving it out means
/// "nothing to add" rather than "empty" (D292).
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
    /// A field is written only when the record carries one, and lists append without duplicating,
    /// so nothing earlier is restated or dropped. Returns the provenance faults rather than refusing,
    /// since only the caller knows whether to reject; an empty list means admissible.
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

    /// Renders it back, sorted by name, so an appended entry diffs as what was learned.
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

    /// Merges one file in. A later entry for the same name wins, so a user file corrects a shipped
    /// one without editing it.
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

    /// How many entries hold something beyond a name: entries are cheap, understanding is not.
    pub fn understood(&self) -> usize {
        self.by_name.values().filter(|f| !f.is_bare()).count()
    }

    /// How many entries rest on a given oracle: the shape of what is known, not its size.
    pub fn resting_on(&self, oracle: Oracle) -> usize {
        self.by_name
            .values()
            .filter(|f| f.known_by == Some(oracle))
            .count()
    }

    /// The regions this knowledge base ships, as a policy to fold under a person's.
    ///
    /// The shipped, `assumed` counterpart to `learned.policy()`: a function returning a pointer to
    /// memory it owns that no probe can call has no measurement, so its region ships here, carrying
    /// the entry's `known_by` so it is never mistaken for evidence (D557). `default_return` is
    /// untouched, as in `learned.policy()`.
    #[must_use]
    pub fn region_policy(&self) -> crate::StubPolicy {
        let mut regions = BTreeMap::new();
        let mut known = BTreeMap::new();
        for function in self.by_name.values() {
            if let Some(region) = function.region {
                regions.insert(function.name.clone(), region);
                known.insert(
                    function.name.clone(),
                    function.known_by.unwrap_or(Oracle::Assumed),
                );
            }
        }
        crate::StubPolicy {
            default_return: crate::StubReturn::Unimplemented,
            overrides: std::collections::HashMap::new(),
            regions: regions.into_iter().collect(),
            known: known.into_iter().collect(),
        }
    }

    /// Every separate thing this project admits it is guessing at.
    ///
    /// Rises as assumptions are written down and falls as hardware answers them.
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

    /// Every knowledge file in the directory is embedded, or it would be written and never read.
    ///
    /// [`EMBEDDED`] is a hand-kept list of literal `include_str!` paths. Read from
    /// `CARGO_MANIFEST_DIR`, the directory those paths are relative to.
    #[test]
    fn every_knowledge_file_on_disk_is_embedded() {
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data/knowledge");
        let mut orphaned: Vec<String> = std::fs::read_dir(&directory)
            .expect("the knowledge directory")
            .filter_map(|entry| {
                let path = entry.ok()?.path();
                (path.extension()? == "toml")
                    .then(|| path.file_stem()?.to_str().map(str::to_owned))?
            })
            .filter(|stem| !super::EMBEDDED.iter().any(|(library, _)| library == stem))
            .collect();
        orphaned.sort();
        assert!(
            orphaned.is_empty(),
            "these knowledge files ship in the repository and no build loads them: {orphaned:?}"
        );
    }

    /// Nothing is embedded under a library name its own file disagrees with, which would put its
    /// functions in another library's namespace.
    #[test]
    fn every_embedded_file_names_the_library_it_is_registered_as() {
        for (library, text) in super::EMBEDDED {
            let file = KnowledgeFile::parse(text).expect("a shipped file parses");
            assert!(
                file.library.is_empty() || file.library == *library,
                "{library} is registered under that name and calls itself {}",
                file.library
            );
        }
    }
    use super::{DELEGATION_ASSUMPTION, Knowledge, KnowledgeFile, Oracle, Record};

    /// A later record adds without erasing an earlier one (D292).
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

    /// A record claiming behaviour without saying how it is known is a fault: every default would
    /// misstate the work (D180).
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

    /// A `found_by` that contradicts the symbol database, or is not current vocabulary, is a fault.
    #[test]
    fn a_found_by_that_contradicts_the_symbol_database_is_a_fault() {
        // Made to fail on purpose, so the guard is seen rejecting something.
        let mut entry = super::FunctionKnowledge {
            name: "memcpy".to_owned(),
            found_by: "supplied".to_owned(),
            ..Default::default()
        };
        // `memcpy` is in the shipped published-standard list, so `supplied` contradicts the audited
        // record in the direction that matters.
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

        // A label outside current vocabulary is caught even with no database record to compare.
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

    /// The shipped files parse and carry real content.
    #[test]
    fn the_shipped_files_parse_and_carry_real_content() {
        // They are embedded, so a typo would break every build and surface as a panic at startup.
        let k = Knowledge::builtin();
        assert!(!k.is_empty(), "something should ship");
        assert!(
            k.understood() > 0,
            "at least one entry should say more than a name"
        );
    }

    /// A declared region reaches the policy, and a `returns` kind does not become one.
    #[test]
    fn a_declared_region_reaches_the_policy_and_a_status_kind_does_not() {
        // A `returns` kind is a scalar answered at the call, so it must not become a region that
        // reserves address space.
        let file = KnowledgeFile::parse(
            r#"
            [[function]]
            name = "answers_a_descriptor"
            purpose = "returns a pointer to memory it owns"
            region = { via = "return", bytes = 256 }
            known_by = "assumed"

            [[function]]
            name = "answers_a_status"
            arity = 1
            returns = "status"
            known_by = "assumed"
            "#,
        )
        .expect("parse");
        let mut k = Knowledge::default();
        k.absorb("libTest", &file);

        // A region is a behaviour claim, and the entry says how it is known, so it is admissible.
        assert!(
            k.provenance_faults().is_empty(),
            "{:?}",
            k.provenance_faults()
        );

        let policy = k.region_policy();
        let region = policy
            .regions
            .get("answers_a_descriptor")
            .expect("the declared region ships");
        assert_eq!(region.bytes, 256);
        assert_eq!(region.via, crate::Delivery::Return);
        assert_eq!(
            policy.known.get("answers_a_descriptor").copied(),
            Some(Oracle::Assumed),
            "carrying its provenance so a run resting on it is never counted as evidence"
        );
        assert!(
            !policy.regions.contains_key("answers_a_status"),
            "a returns-kind is a scalar answer, not a region to reserve"
        );
    }

    /// Every oracle is falsifiable.
    #[test]
    fn every_oracle_is_falsifiable() {
        // Each value is checkable against a named outside source or answerable by a probe; a value
        // meaning "the model recalled it" would satisfy neither.
        for oracle in [
            Oracle::Published,
            Oracle::Differential,
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

    /// Recording behaviour requires saying how it is known.
    #[test]
    fn recording_behaviour_requires_saying_how_it_is_known() {
        // The entry an unattended agent produces by default: a confident arity, a return kind, and no
        // source.
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

    /// A name alone needs no source.
    #[test]
    fn a_name_alone_needs_no_source() {
        // Recording that a function exists is not a claim about what it does.
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

    /// An outside source has to be checkable.
    #[test]
    fn an_outside_source_has_to_be_checkable() {
        // "It is in the standard" without saying where reads as evidence and cannot be checked.
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

    /// A guess cites nothing.
    #[test]
    fn a_guess_cites_nothing() {
        // Citing a source for something nobody established looks like the entry above at a glance.
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

    /// Open questions cannot be reduced by leaving the detail out.
    #[test]
    fn open_questions_cannot_be_reduced_by_leaving_the_detail_out() {
        // An `assumed` entry listing nothing is the same guess as one that spells it out, and must not
        // count for less.
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

        // One for the entry listing nothing, two for the entry listing two: never a penalty on top of
        // the items.
        assert_eq!(k.get("silent_guess").expect("present").open_questions(), 1);
        assert_eq!(k.get("spelled_out").expect("present").open_questions(), 2);
        assert_eq!(k.open_questions(), 3);

        // Leaving the detail out does not lower the count.
        assert!(
            k.get("silent_guess").expect("present").open_questions() >= 1,
            "a silent guess still costs one"
        );
    }

    /// The count and the list are the same answer, for every shape an entry can take.
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

        // A certain entry asks nothing, so an empty queue means an empty queue.
        assert!(
            k.get("published_and_certain")
                .expect("present")
                .open_questions_asked()
                .is_empty()
        );
    }

    /// A partly measured entry still carries its open questions.
    #[test]
    fn a_partly_measured_entry_still_carries_its_open_questions() {
        // The normal case: the shape from the standard and one edge a guess.
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

    /// The shipped files account for everything they claim.
    #[test]
    fn the_shipped_files_account_for_everything_they_claim() {
        // Held here as well as in CI, so it fails when the entry is written.
        let faults = Knowledge::builtin().provenance_faults();
        assert!(faults.is_empty(), "{faults:#?}");
    }

    /// What was measured about the direct-memory query is still recorded.
    #[test]
    fn what_was_measured_about_direct_memory_query_survived() {
        // Each fact here cost an experiment; if this fails, the knowledge was lost.
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

    /// An unknown arity is distinct from zero.
    #[test]
    fn an_unknown_arity_is_distinct_from_zero() {
        // Zero renders differently from unknown in a trace.
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

    /// A bare entry counts as recorded but not as understood.
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

    /// A later file can correct an earlier one.
    #[test]
    fn a_later_file_can_correct_an_earlier_one() {
        // A user file overrides a shipped one without editing it.
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

    /// A rendered file round-trips and comes back sorted.
    #[test]
    fn a_rendered_file_round_trips_and_comes_back_sorted() {
        // What is written must read back, and sorting keeps a diff about what was learned.
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

    /// Every open question belongs to a function that can be asked about.
    #[test]
    fn every_open_question_belongs_to_a_function_that_can_be_asked_about() {
        // A question whose function is not in the knowledge base has nowhere to record its answer.
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

    /// A question is never recorded against something already measured.
    #[test]
    fn a_question_is_never_recorded_against_something_already_measured() {
        // `measured` means hardware answered; an open question on such an entry contradicts it.
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

    /// The open-question count matches what the entries carry.
    #[test]
    fn the_open_question_count_matches_what_the_entries_carry() {
        // `open_questions` is reported and used to rank work, so it must equal the sum of what each
        // entry prints.
        let k = Knowledge::builtin();
        let listed: usize = k.functions().map(|f| f.open_questions_asked().len()).sum();
        assert_eq!(k.open_questions(), listed);

        // An entry that itemises contributes exactly its items.
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

    /// Punctuation is forgiven and a word is not: two entries punctuating one sentence differently
    /// are one premise, and a single differing word makes two (D538).
    ///
    /// The first pair is real data from `libkernel.toml`. Whether two differently worded questions
    /// mean the same thing is not decided here.
    #[test]
    fn one_sentence_punctuated_two_ways_is_one_premise_and_one_word_apart_is_two() {
        let full_stop = concat!(
            "Modelled on the POSIX call of the same shape. The correspondence is ",
            "inferred from the name."
        );
        let semicolon = concat!(
            "Modelled on the POSIX call of the same shape; the correspondence is ",
            "inferred from the name."
        );
        let one_word_off = concat!(
            "Modelled on the POSIX call of the same name. The correspondence is ",
            "inferred from the name."
        );

        let asked = vec![
            ("sceKernelWrite".to_owned(), full_stop.to_owned()),
            ("sceKernelOpen".to_owned(), semicolon.to_owned()),
            ("sceKernelRead".to_owned(), full_stop.to_owned()),
            ("scePthreadJoin".to_owned(), one_word_off.to_owned()),
        ];
        let premises = super::shared_premises(&asked);

        assert_eq!(
            premises.len(),
            2,
            concat!(
                "a full stop and a semicolon do not make two premises, and one different word ",
                "does: {:#?}"
            ),
            premises
        );
        assert_eq!(premises[0].functions.len(), 3);
        assert_eq!(
            premises[0].wordings, 2,
            concat!(
                "the two punctuations must be reported, not hidden - that count is what says ",
                "the data needs tidying"
            )
        );
        assert_eq!(
            premises[0].question, full_stop,
            "the commonest wording is the one printed"
        );
        assert_eq!(premises[1].functions, vec!["scePthreadJoin".to_owned()]);

        // Nothing is lost or duplicated on the way through.
        let regrouped: usize = premises.iter().map(|p| p.functions.len()).sum();
        assert_eq!(regrouped, asked.len());
    }

    /// No premise in the shipped knowledge base is written two ways (D538).
    ///
    /// Otherwise one unknown reads as two on the ask list obSCEne's backlog is generated from. Only
    /// exact repetition is seen, so the count this protects is an upper bound.
    #[test]
    fn no_premise_in_the_knowledge_base_is_written_two_ways() {
        let k = Knowledge::builtin();
        let asked: Vec<(String, String)> = k
            .functions()
            .flat_map(|f| {
                f.open_questions_asked()
                    .into_iter()
                    .map(move |q| (f.name.clone(), q))
            })
            .collect();

        for premise in super::shared_premises(&asked) {
            assert_eq!(
                premise.wordings,
                1,
                concat!(
                    "{} entries share this premise and write it {} different ways, so it is ",
                    "counted as {} open questions rather than one: {:?}"
                ),
                premise.functions.len(),
                premise.wordings,
                premise.wordings,
                premise.question
            );
        }
    }

    /// Every delegation asks the one question, in [`DELEGATION_ASSUMPTION`]'s wording (D349).
    ///
    /// Any other sentence about two spellings behaving alike is a fault, whoever wrote it.
    /// `orbistoun-gen` emits the shared constant; a hand-written entry could drift, and this stops
    /// it. Whether the premise is true is not asserted.
    #[test]
    fn the_delegation_question_is_asked_in_one_wording() {
        let mut strays = Vec::new();
        let mut asking = 0usize;
        for f in Knowledge::builtin().functions() {
            for question in f.open_questions_asked() {
                if question == DELEGATION_ASSUMPTION {
                    asking += 1;
                } else if question.contains("behave alike")
                    || question.contains("same behaviour")
                    || question.contains("two spellings")
                {
                    strays.push(format!("{}: {question}", f.name));
                }
            }
        }
        assert!(
            strays.is_empty(),
            concat!(
                "these ask the delegation question in words of their own, so each becomes a ",
                "premise of its own: {:#?}"
            ),
            strays
        );
        assert!(
            asking > 1,
            concat!(
                "the shared wording is what makes this one premise - finding it on {} ",
                "entries means it stopped being shared rather than that the entries were fixed"
            ),
            asking
        );
    }
}
