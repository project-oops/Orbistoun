//! Claims the knowledge base makes about other documents, checked against those documents.
//!
//! Two kinds of claim are wrong on their face and therefore testable: a stated POSIX namesake that
//! does not exist, and a hardware measurement filed as an open assumption. `questions` ranks
//! assumptions and obSCEne's backlog is generated from that ranking, so a measurement filed there
//! asks the hardware again for an answer it has already given.

use std::collections::BTreeSet;

/// Sentences that assert this entry has a namesake in the C library.
///
/// A list of the exact wordings in use, not a pattern: a pattern over prose stops matching
/// silently. A new wording slips past and shows up in `questions --premises` as a group of one
/// (D538).
const CLAIMS_A_NAMESAKE: &[&str] = &[
    "Semantics follow the POSIX analogue of the same name.",
    "Modelled on the POSIX call of the same shape.",
    "Behaviour follows the POSIX analogue;",
    "Inferred from the name and the POSIX call it resembles.",
];

/// The sentence asserting the opposite, equally checkable.
const CLAIMS_NO_NAMESAKE: &str = "There is no POSIX function of this name:";

/// Sentences an assumption never contains, because they announce evidence.
///
/// An assumption is something nobody has established. A hardware result belongs in `edge_cases`,
/// where the hardware absorption files them.
const ANNOUNCES_A_MEASUREMENT: &[&str] = &[
    "CONFIRMED ON HARDWARE",
    "Measured on hardware",
    "a target console has now returned",
    "a target console returned",
];

/// Every name a vendor-spelled function could be named after, by spelling alone.
///
/// Prefix strips and a case fold, never a judgement about behaviour: `scePthreadCondWait` gives
/// `pthread_cond_wait`; `sceKernelWrite` gives both `write` and `kernel_write`, because the prefix
/// is `sceKernel` for file calls and `sce` for pthread calls and the name does not say which. The
/// candidates are generous because a guard that fires falsely gets weakened.
fn spellings_of(vendor: &str) -> BTreeSet<String> {
    fn snake(name: &str) -> String {
        let mut out = String::with_capacity(name.len() + 4);
        for (i, c) in name.char_indices() {
            if i > 0 && c.is_ascii_uppercase() {
                out.push('_');
            }
            out.extend(c.to_lowercase());
        }
        out
    }
    let mut out = BTreeSet::new();
    out.insert(snake(vendor));
    for prefix in ["posix_", "sceKernel", "sce", "_"] {
        if let Some(rest) = vendor.strip_prefix(prefix) {
            if !rest.is_empty() {
                out.insert(snake(rest));
            }
        }
    }
    out.remove("");
    out
}

/// The functions FreeBSD exports, as harvested.
fn harvest() -> BTreeSet<&'static str> {
    let names: BTreeSet<&str> = orbistoun_names::DEFAULT_STANDARD_NAMES
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    assert!(
        names.len() > 1000,
        concat!(
            "the harvest is the oracle here, and {} names is not it - a truncated list would make ",
            "every check below pass by having nothing to disagree with"
        ),
        names.len()
    );
    names
}

/// A claimed POSIX namesake exists, and a denied one does not.
///
/// An entry saying its semantics follow the POSIX function of the same name must spell into a
/// harvested name; an entry saying there is none must not. A pass says the name is real, not that
/// the platform's call behaves like it.
#[test]
fn a_claimed_posix_namesake_exists_and_a_denied_one_does_not() {
    let standard = harvest();

    // The broad spelling rule still rejects the event-flag and semaphore calls, which have no POSIX
    // namesake.
    for gone in [
        "sceKernelWaitSema",
        "sceKernelCreateEventFlag",
        "sceKernelPollEventFlag",
    ] {
        assert!(
            spellings_of(gone)
                .iter()
                .all(|s| !standard.contains(s.as_str())),
            concat!(
                "{} spells into {:?}, and if any of those are harvested names then this check ",
                "would no longer catch the entries D540 corrected"
            ),
            gone,
            spellings_of(gone)
        );
    }

    let knowledge = orbistoun_hle::knowledge::Knowledge::builtin();
    let (mut claiming, mut denying) = (0usize, 0usize);
    let (mut unfounded, mut wrongly_denied) = (Vec::new(), Vec::new());
    for f in knowledge.functions() {
        let asked = f.open_questions_asked();
        let found = spellings_of(&f.name)
            .into_iter()
            .find(|s| standard.contains(s.as_str()));
        if asked
            .iter()
            .any(|q| CLAIMS_A_NAMESAKE.iter().any(|c| q.contains(c)))
        {
            claiming += 1;
            if found.is_none() {
                unfounded.push(format!(
                    "{} spells into {:?}",
                    f.name,
                    spellings_of(&f.name)
                ));
            }
        }
        if asked.iter().any(|q| q.contains(CLAIMS_NO_NAMESAKE)) {
            denying += 1;
            if let Some(name) = found {
                wrongly_denied.push(format!("{} - the harvest has `{name}`", f.name));
            }
        }
    }

    assert!(
        claiming > 1 && denying > 1,
        concat!(
            "these are claims a family of entries share; finding them on {} and {} ",
            "entries means a wording changed and this is checking less than it reads as"
        ),
        claiming,
        denying
    );
    assert!(
        unfounded.is_empty(),
        concat!(
            "these say their semantics follow the POSIX function of the same name, and no harvested ",
            "name is spelled that way - so the question sends a probe after something that does not ",
            "exist: {:#?}"
        ),
        unfounded
    );
    assert!(
        wrongly_denied.is_empty(),
        concat!(
            "these say there is no POSIX function of their name, and the harvest has one: ",
            "{:#?}"
        ),
        wrongly_denied
    );
}

/// An open question does not announce its own answer.
///
/// No `assumptions` line says the hardware measured something; such a line spends hardware time
/// re-establishing a known result. A measurement described without any of these words passes, and a
/// stale assumption is indistinguishable from a live one.
#[test]
fn an_open_question_does_not_announce_a_measurement() {
    let mut announcing = Vec::new();
    for f in orbistoun_hle::knowledge::Knowledge::builtin().functions() {
        for question in f.open_questions_asked() {
            if let Some(word) = ANNOUNCES_A_MEASUREMENT
                .iter()
                .find(|w| question.contains(**w))
            {
                announcing.push(format!("{}: says {word:?} - {question}", f.name));
            }
        }
    }
    assert!(
        announcing.is_empty(),
        concat!(
            "an assumption is a thing nobody has established; these say a console established it, ",
            "so the queue asks hardware for an answer it already has: {:#?}"
        ),
        announcing
    );
}

/// An entry holding a measurement does not report that nothing is known about it.
///
/// No entry prints [`NOTHING_ESTABLISHED`] while its edge cases carry a hardware result. An entry
/// with a measurement may still be `assumed` - one measured behaviour does not settle the others it
/// lists - but claiming nothing is known contradicts the entry itself. Only measurements written
/// with the absorption's `Measured on hardware:` prefix are seen here.
#[test]
fn an_entry_holding_a_measurement_does_not_say_nothing_is_established() {
    let knowledge = orbistoun_hle::knowledge::Knowledge::builtin();
    let mut measured = 0usize;
    let mut contradicting = Vec::new();
    for f in knowledge.functions() {
        if !f
            .edge_cases
            .iter()
            .any(|e| e.starts_with("Measured on hardware:"))
        {
            continue;
        }
        measured += 1;
        if f.open_questions_asked()
            .iter()
            .any(|q| q == orbistoun_hle::knowledge::NOTHING_ESTABLISHED)
        {
            contradicting.push(f.name.clone());
        }
    }
    assert!(
        measured > 10,
        concat!(
            "the hardware absorption writes these edge cases; finding them on {} entries ",
            "means the prefix changed and this is checking almost nothing"
        ),
        measured
    );
    assert!(
        contradicting.is_empty(),
        concat!(
            "these carry a hardware result and report that nothing about them has been ",
            "established: {:#?}"
        ),
        contradicting
    );
}
