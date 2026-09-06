//! Claims the knowledge base makes about *other* documents, checked against them.
//!
//! # The two failures this is here for
//!
//! **A claim about a function that does not exist.** Thirty-three entries said their semantics
//! follow "the POSIX analogue of the same name". Nine were event-flag and semaphore calls -
//! `sceKernelCreateEventFlag`, `sceKernelWaitSema` and their family - and POSIX has no function
//! of any of those names. It was the only open question `sceKernelWaitSema` had, so nothing
//! recorded that two of its three arguments are ignored (D540).
//!
//! **A measurement filed as a thing nobody knows.** `scePthreadMutexUnlock` carried an
//! assumption beginning *"CONFIRMED ON HARDWARE"*. `questions` ranks assumptions and obSCEne's
//! backlog is generated from that ranking, so the queue was asking a console to establish
//! something a console had already established, in a sentence that said so (D541).
//!
//! Both are wrong *on their face*, before anybody argues about the platform - which is what
//! makes them testable at all.

use std::collections::BTreeSet;

/// Sentences that assert this entry has a namesake in the C library.
///
/// **A list, not a rule.** Matching prose is how you write a check that silently stops
/// matching, so this names the exact wordings in use and nothing else. A claim written afresh
/// slips past - and becomes its own premise, which `questions --premises` shows as a group of
/// one, which is how the next one gets found (D538).
const CLAIMS_A_NAMESAKE: &[&str] = &[
    "Semantics follow the POSIX analogue of the same name.",
    "Modelled on the POSIX call of the same shape.",
    "Behaviour follows the POSIX analogue;",
    "Inferred from the name and the POSIX call it resembles.",
];

/// The sentence asserting the opposite, which is equally checkable and equally wrong if wrong.
const CLAIMS_NO_NAMESAKE: &str = "There is no POSIX function of this name:";

/// Sentences an assumption must never contain, because they announce evidence.
///
/// An `assumptions` line is by definition something nobody has established. One saying a
/// console returned a value is a fact, and belongs in `edge_cases` where the rest of the
/// hardware absorption puts them - not in the queue that asks hardware for answers.
const ANNOUNCES_A_MEASUREMENT: &[&str] = &[
    "CONFIRMED ON HARDWARE",
    "Measured on hardware",
    "a target console has now returned",
    "a target console returned",
];

/// Every name a vendor-spelled function could be named after, by spelling alone.
///
/// **Prefix strips and a case fold - a transformation of the name, never a judgement about
/// behaviour.** `scePthreadCondWait` gives `pthread_cond_wait`; `sceKernelWrite` gives `write`
/// as well as `kernel_write`, because the vendor prefix is `sceKernel` for the file calls and
/// `sce` for the pthread ones, and which it is cannot be decided from the name.
///
/// Deliberately generous: several candidates, and a hit on any of them passes. A narrow rule
/// would fire on `sceKernelWrite` - whose POSIX namesake is plainly `write` - and a guard that
/// fires falsely is one somebody weakens the next time it does.
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
        "the harvest is the oracle here, and {} names is not it - a truncated list would make \
         every check below pass by having nothing to disagree with",
        names.len()
    );
    names
}

/// **A claimed POSIX namesake exists, and a denied one does not.**
///
/// # What this asserts
///
/// Both directions, because both are claims about the same citable list. An entry saying its
/// semantics follow the POSIX function of the same name must spell into a harvested name; an
/// entry saying there is no POSIX function of its name must not.
///
/// The second direction is not symmetry for its own sake - the nine entries D540 corrected now
/// carry that negative sentence, and if one were later given to a function that *does* have a
/// namesake it would be as wrong as what it replaced.
///
/// # What it cannot assert
///
/// **That a claim it passes is true.** `pthread_cond_wait` being in the harvest says the name
/// is real. It says nothing about whether the platform's call behaves like it, which is exactly
/// what these entries are admitting nobody knows. This catches the claim that is wrong before
/// anybody goes near a console, not the one that is merely unverified.
#[test]
fn a_claimed_posix_namesake_exists_and_a_denied_one_does_not() {
    let standard = harvest();

    // The widening must not have neutered the check: the names that prompted it still fail.
    for gone in [
        "sceKernelWaitSema",
        "sceKernelCreateEventFlag",
        "sceKernelPollEventFlag",
    ] {
        assert!(
            spellings_of(gone)
                .iter()
                .all(|s| !standard.contains(s.as_str())),
            "{gone} spells into {:?}, and if any of those are harvested names then this check \
             would no longer catch the entries D540 corrected",
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
        "these are claims a family of entries share; finding them on {claiming} and {denying} \
         entries means a wording changed and this is checking less than it reads as"
    );
    assert!(
        unfounded.is_empty(),
        "these say their semantics follow the POSIX function of the same name, and no harvested \
         name is spelled that way - so the question sends a probe after something that does not \
         exist: {unfounded:#?}"
    );
    assert!(
        wrongly_denied.is_empty(),
        "these say there is no POSIX function of their name, and the harvest has one: \
         {wrongly_denied:#?}"
    );
}

/// **An open question does not announce its own answer.**
///
/// # What this asserts
///
/// That no `assumptions` line says a console measured something. `questions` ranks assumptions
/// and obSCEne's backlog is generated from that ranking, so a measurement sitting there spends
/// hardware time re-establishing what is already established - and it is the entries with the
/// most evidence that are most likely to carry one, because somebody had a result to write down
/// and put it where the reasoning already was.
///
/// The hardware absorption already files its results in `edge_cases`; this catches the
/// hand-written ones. It fails on `scePthreadMutexUnlock` as it stood before D541, whose
/// assumption began "CONFIRMED ON HARDWARE".
///
/// # What it cannot assert
///
/// That an assumption is genuinely open. A measurement described *without* any of these words -
/// "the console answers zero here" - reads as an assumption and passes. And it cannot tell a
/// stale assumption from a live one: the same entry carried "it is a structure worth testing
/// for, not an established encoding" about a rule D398 had measured months earlier, and nothing
/// in that sentence announces anything.
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
        "an assumption is a thing nobody has established; these say a console established it, \
         so the queue asks hardware for an answer it already has: {announcing:#?}"
    );
}

/// **An entry holding a measurement does not report that nothing is known about it.**
///
/// # What this asserts
///
/// That no entry prints [`NOTHING_ESTABLISHED`] while carrying a hardware result in its edge
/// cases. That sentence is what an entry says when `known_by` is a guess and it itemises
/// nothing (D239); an entry a console has answered is not that, whatever else remains open.
///
/// Six were in exactly that state - `sceGnmDispatchDirect`, `sceKernelGetSystemSwVersion`,
/// `sceVideoOutSetFlipRate` and three more - each with reasoning in its implementation, a
/// measured value in its edge cases, and a record saying nothing had been established. They
/// survived five ticks of auditing the ask list because `questions` ranks by call count and
/// these have between none and eleven, so they sat at the bottom of every list that was read
/// from the top (D542).
///
/// # Why this and not the wider rule
///
/// The obvious rule is that an entry with a measurement may not be `assumed` at all. That
/// fires on **22** entries, and sixteen of them are right: a console answering one behaviour
/// does not establish the rest, and check 10 exists precisely to stop one measured fact
/// promoting an entry past the questions it still lists. `known_by` describes the entry.
///
/// The six are different because they claim *nothing at all* is known, which the entry itself
/// disproves two lines above. That is a contradiction rather than a judgement, which is what
/// makes it testable.
///
/// # What it cannot assert
///
/// That the sixteen are labelled correctly - it does not look at them. And it cannot catch the
/// same contradiction stated any other way: an entry with a measured value described in prose
/// that does not begin `Measured on hardware:` is invisible here, because that prefix is what
/// the hardware absorption writes and this checks the absorption's own output.
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
        "the hardware absorption writes these edge cases; finding them on {measured} entries \
         means the prefix changed and this is checking almost nothing"
    );
    assert!(
        contradicting.is_empty(),
        "these carry a hardware result and report that nothing about them has been \
         established: {contradicting:#?}"
    );
}
