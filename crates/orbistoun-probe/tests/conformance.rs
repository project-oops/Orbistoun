//! Every captured exchange, read against the protocol.
//!
//! # Why these files are here rather than referenced
//!
//! They are copies of obSCEne's `docs/examples/protocol/`, taken deliberately. A test that
//! reads a sibling checkout fails for anyone without one, and a build-time dependency
//! between the two projects is the coupling D207 exists to prevent. The contract is the
//! specification plus these transcripts; a transcript is data, so copying it is the right
//! kind of duplication and copying code would be the wrong kind.
//!
//! If they drift, that is a fact worth discovering here rather than against hardware.
//!
//! # What passing means
//!
//! That this crate can read what a real probe emits, without a probe. The transcripts cover
//! negotiation, a call that returns, a call that dies, a timeout, a refusal, a memory read,
//! blob and run, reset, no-reset, and a malformed sequence - which is the whole grammar
//! including every path that is *not* a clean answer.
//!
//! Those paths are the point. A consumer that only handles success is one that turns a
//! crash into a plausible number.

use std::collections::BTreeSet;
use std::path::PathBuf;

use orbistoun_probe::{
    Capability, ObservedBy, Origin, Outcome, Refusal, SymbolFact, Transcript, parse,
};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("protocol")
}

fn transcripts() -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = std::fs::read_dir(fixtures())
        .expect("fixture directory")
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|e| e == "txt"))
        .map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let text = std::fs::read_to_string(entry.path()).expect("readable fixture");
            (name, text)
        })
        .collect();
    found.sort();
    found
}

#[test]
fn every_captured_exchange_parses() {
    // The blunt one. A line the grammar does not cover is either a protocol this crate
    // cannot read or a transcript that has drifted from the specification, and both are
    // worth failing over.
    let all = transcripts();
    assert!(
        all.len() >= 10,
        "expected the full set of captured exchanges, found {}",
        all.len()
    );

    for (name, text) in all {
        let lines = parse(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(!lines.is_empty(), "{name}: empty");
        Transcript::read(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
    }
}

#[test]
fn a_command_that_did_not_answer_carries_no_value() {
    // The single most important rule in the protocol, and the reason this crate models
    // outcomes the way it does.
    //
    // `died` is not `returned 0`. There is no field on the non-answering variants for a
    // value to hide in, so this asserts the property holds across every real transcript
    // rather than only in the type.
    for (name, text) in transcripts() {
        let transcript = Transcript::read(&text).expect("parses");
        for exchange in &transcript.exchanges {
            let Some(outcome) = &exchange.outcome else {
                continue;
            };
            if outcome.answered() {
                continue;
            }
            assert_eq!(
                outcome.value(),
                None,
                concat!(
                    "{}: a {} outcome for `{}` produced a value, which would make a ",
                    "fiction indistinguishable from evidence"
                ),
                name,
                outcome,
                exchange.verb
            );
            assert_eq!(
                outcome.observed_by(),
                ObservedBy::Driver,
                concat!(
                    "{}: a {} outcome can only be established by the driver - the probe ",
                    "was gone or silent, so it cannot have reported this itself"
                ),
                name,
                outcome,
            );
        }
    }
}

#[test]
fn a_value_that_answered_is_readable_and_a_death_is_not() {
    // The other direction, so the rule above cannot be satisfied by refusing everything.
    let text = std::fs::read_to_string(fixtures().join("03-died.txt")).expect("fixture");
    let transcript = Transcript::read(&text).expect("parses");

    let resolve = transcript
        .exchanges
        .iter()
        .find(|exchange| exchange.verb == "resolve")
        .expect("the transcript resolves a symbol");
    assert_eq!(
        resolve.outcome.as_ref().and_then(Outcome::value),
        Some(0x8001_9c40),
        "a resolve that returned an address should yield it"
    );

    let call = transcript
        .exchanges
        .iter()
        .find(|exchange| exchange.verb == "call")
        .expect("the transcript makes a call");
    assert!(call.acknowledged, "the call was acknowledged before it ran");
    assert_eq!(call.outcome, Some(Outcome::Died));
    assert_eq!(call.outcome.as_ref().and_then(Outcome::value), None);
    assert!(!call.answered());
}

#[test]
fn a_restart_is_visible_as_a_new_session() {
    // A faulting command ends the probe, and the protocol makes the discontinuity legible
    // rather than papering over it. Two sessions in one transcript is what that looks like,
    // and a consumer that merged them would attribute one process's answers to another.
    let text = std::fs::read_to_string(fixtures().join("03-died.txt")).expect("fixture");
    let transcript = Transcript::read(&text).expect("parses");

    assert_eq!(
        transcript.sessions.len(),
        2,
        "the probe died and was restarted, which is two processes"
    );
    let identifiers: BTreeSet<&str> = transcript
        .sessions
        .iter()
        .map(|session| session.session.as_str())
        .collect();
    assert_eq!(identifiers.len(), 2, "a restart means a fresh identifier");

    // And the metadata follows the session it names, not whichever came last.
    for session in &transcript.sessions {
        assert_eq!(
            session.parts.get("target").map(String::as_str),
            Some("console"),
            "both processes ran on the same part"
        );
    }
}

#[test]
fn what_produced_an_answer_is_always_recorded() {
    // Without it a number measured on a stand-in and read later as authoritative for the
    // real target is a wrong answer with no way to see that it is wrong. This project has
    // already lost months to exactly that, pointed at the wrong GPU generation with nothing
    // in the record saying so.
    for (name, text) in transcripts() {
        let transcript = Transcript::read(&text).expect("parses");
        for session in &transcript.sessions {
            assert!(
                !session.parts.is_empty(),
                "{name}: session {} announced nothing about what produced its answers",
                session.session
            );
            assert!(
                session.parts.contains_key("target") || session.parts.contains_key("probe"),
                "{name}: session {} does not say what it ran on",
                session.session
            );
        }
    }
}

#[test]
fn a_stand_in_target_announces_that_it_cannot_resolve() {
    // Capability negotiation earning its keep. The Deck has none of the platform's
    // libraries, so a question about library behaviour is refused rather than answered
    // wrongly - and a consumer discovers that here instead of assuming it.
    let text = std::fs::read_to_string(fixtures().join("01-hello.txt")).expect("fixture");
    let transcript = Transcript::read(&text).expect("parses");
    let session = transcript.sessions.first().expect("one session");

    assert_eq!(
        session.parts.get("target").map(String::as_str),
        Some("deck")
    );
    assert!(
        !session.can(&Capability::Resolve),
        "a part with no system libraries must not announce `resolve`"
    );
    assert!(session.can(&Capability::Gpu), "but it does have a GPU");
    assert_eq!(
        session.claimed_target(),
        Some("deck"),
        "and what it claims is readable, as a claim"
    );
}

#[test]
fn a_repeated_sequence_is_refused_without_an_acknowledgement() {
    // The one place the acknowledge-first rule yields, and it yields to the rule that makes
    // acknowledgement useful: an `ack` is keyed by sequence, so acknowledging a repeat puts
    // two on the wire bearing one key.
    let text = std::fs::read_to_string(fixtures().join("10-bad-sequence.txt")).expect("fixture");
    let transcript = Transcript::read(&text).expect("parses");

    let repeated = transcript
        .exchanges
        .iter()
        .filter(|exchange| exchange.refusal == Some(Refusal::BadArgument))
        .collect::<Vec<_>>();
    assert!(
        repeated.len() >= 2,
        "the transcript refuses both a repeated sequence and one that is not a number"
    );
    for exchange in repeated {
        assert!(
            !exchange.acknowledged,
            "`{}` was refused for its sequence, so it must not have been acknowledged",
            exchange.verb
        );
    }

    // A sequence that is not a number at all is readable rather than a parse failure -
    // the protocol specifies that case, so a transcript capturing it has to be loadable
    // or the case cannot be tested.
    assert!(
        transcript
            .exchanges
            .iter()
            .any(|exchange| exchange.seq.is_none()),
        "one request carried a sequence that was not a number"
    );
}

#[test]
fn an_unknown_record_is_kept_rather_than_dropped() {
    // The protocol permits new record kinds and new fields within a version, and requires
    // a consumer to ignore what it does not recognise. Ignoring is not discarding: a reader
    // built today must not silently delete what a newer probe said.
    let line = "OBS|weather|drizzle|7";
    let parsed = orbistoun_probe::parse_line(line).expect("an unknown record still parses");
    match parsed {
        orbistoun_probe::Line::Record(orbistoun_probe::Record::Other { kind, fields }) => {
            assert_eq!(kind, "weather");
            assert_eq!(fields, vec!["drizzle".to_owned(), "7".to_owned()]);
        }
        other => panic!("expected an unrecognised record, got {other:?}"),
    }
}

#[test]
fn a_non_answer_carrying_a_value_is_refused_at_the_door() {
    // Defence in depth. The type makes it unrepresentable; this makes a transcript that
    // tries it fail loudly rather than being quietly normalised into something readable.
    let line = "OBS|done|3|died|0x0|";
    let error =
        orbistoun_probe::parse_line(line).expect_err("a died outcome with a value must be refused");
    assert!(
        error.contains("did not answer"),
        "the refusal should say why: {error}"
    );
}

#[test]
fn a_hardware_result_is_measured_only_when_the_operator_asserted_is_target() {
    // The rule that decides whether a number becomes a fact about this platform, and the
    // correction that mattered most: it does NOT turn on what the session claimed.
    //
    // A probe cannot certify its own machine. Running inside an emulator, its call to the
    // platform's version query returns the emulator's chosen version - so a probe that
    // announced `target|console` would be putting an emulator's answer in a console's
    // badge, and it would look exactly like a measurement of real hardware.
    //
    // The operator is the only one who knows whether the thing on the desk is a console.
    use orbistoun_hle::knowledge::Oracle;
    use orbistoun_probe::Provenance;

    let asserted = Origin::asserted("console", "13.520.001", true);
    let emulator = Origin::asserted("shadPS4", "", false);
    let unasserted = Origin::unasserted();

    assert_eq!(Provenance::Hardware.oracle(&asserted), Oracle::Measured);
    assert_eq!(
        Provenance::Hardware.oracle(&emulator),
        Oracle::Assumed,
        "real to the probe, not real hardware"
    );
    assert_eq!(
        Provenance::Hardware.oracle(&unasserted),
        Oracle::Assumed,
        "nobody said what ran, so nothing can be a measurement of the target"
    );

    // The rest of the mapping does not depend on the machine: a specification says what it
    // says wherever it was read.
    for (probe, expected) in [
        (Provenance::Spec, Oracle::Published),
        (Provenance::Documented, Oracle::Published),
        (Provenance::Derived, Oracle::Published),
        (Provenance::Assumed, Oracle::Assumed),
    ] {
        assert_eq!(probe.oracle(&asserted), expected, "{probe:?} asserted");
        assert_eq!(probe.oracle(&unasserted), expected, "{probe:?} unasserted");
    }
}

#[test]
fn a_result_written_before_provenance_existed_claims_nothing() {
    // The record format gained the field after some kinds were already being written, and
    // its own documentation says the table drifted and a parser written against it would
    // have been wrong about half the stream.
    //
    // So an absent field is absent, not a default. Inventing a grade for a record that
    // never carried one is the same error as recording a value for a call that died.
    use orbistoun_probe::{Line, Provenance, Record, Status};

    let old = "OBS|res|000-boot/write-rejects-bad-fd|pass|0xffffffff80020009|";
    let Ok(Line::Record(Record::Res {
        status, provenance, ..
    })) = orbistoun_probe::parse_line(old)
    else {
        panic!("an older result should still parse");
    };
    assert_eq!(status, Status::Pass);
    assert_eq!(provenance, None, "no field means no claim");

    let current = "OBS|res|020-memory/allocate|pass|0x8804000000||assumed";
    let Ok(Line::Record(Record::Res {
        value, provenance, ..
    })) = orbistoun_probe::parse_line(current)
    else {
        panic!("a current result should parse");
    };
    assert_eq!(value, "0x8804000000");
    assert_eq!(provenance, Some(Provenance::Assumed));
}

#[test]
fn a_corpus_has_no_commands_in_it_and_its_results_still_count() {
    // The artefact that matters is records all the way down.
    //
    // A session transcript is the *interface* - commands and replies. What gets committed
    // is the report a run produced, and it contains no `CMD|` lines at all. A reader that
    // only looked inside exchanges found nothing in the file that actually matters, which
    // is what this crate did until a real report was pointed at it.
    // One directory up: this is a report, not a protocol transcript, and keeping it out of
    // `protocol/` stops the grammar tests treating it as one.
    let path = fixtures()
        .parent()
        .expect("fixtures root")
        .join("emulator-run.txt");
    let text = std::fs::read_to_string(path).expect("fixture");
    let transcript = Transcript::read(&text).expect("parses");

    assert!(
        transcript.exchanges.is_empty(),
        "a corpus carries no commands"
    );
    assert!(
        !transcript.records.is_empty(),
        "and its records must not be dropped for having no command to belong to"
    );

    let established = transcript.established(&Origin::asserted("console", "13.520.001", true));
    assert!(established.total() > 0, "a corpus establishes something");
    // This particular report predates the provenance field, so nothing in it is graded -
    // and the honest reading is that it establishes no facts, not that it establishes 47.
    assert_eq!(established.ungraded, established.total());
    assert_eq!(
        established.facts(),
        0,
        "ungraded results are not facts, however many there are"
    );
}

#[test]
fn a_result_becomes_a_finding_only_when_something_named_the_function() {
    // The step from "this check passed" to "this function returns this". A `res` names its
    // check and never the function; the `try` before it does, and pairing them is what
    // turns a report into something the emulator can act on.
    //
    // Constructed rather than captured, and said so plainly: the fixtures in `protocol/`
    // are real exchanges and this is not one. Building a plausible transcript and filing it
    // beside them would make a fabrication indistinguishable from evidence, which is the
    // failure this whole crate is shaped around.
    let text = "OBS|hello|1|abc123|call,resolve,report
OBS|part|abc123|target|console
OBS|part|abc123|firmware|13.520.001
OBS|try|010-fs/open-missing|libkernel|sceKernelOpen
OBS|res|010-fs/open-missing|pass|0xfffffffe|no such file|hardware
OBS|res|010-fs/orphan|pass|0x1||hardware
";
    let transcript = Transcript::read(text).expect("parses");
    let findings = transcript.findings(&Origin::asserted("console", "13.520.001", true));

    assert_eq!(
        findings.len(),
        1,
        "the orphaned result names no function, so nothing can be concluded about one"
    );
    let finding = &findings[0];
    assert_eq!(finding.library, "libkernel");
    assert_eq!(finding.symbol, "sceKernelOpen");
    assert_eq!(finding.value, "0xfffffffe");
    assert!(finding.is_fact(), "measured on the target");

    // And the citation says on what, because "measured" without saying where is the claim
    // this project has already been burned by.
    let origin = Origin::asserted("console", "13.520.001", true);
    let cites = finding.cites(&origin);
    assert!(cites.contains("console"), "{cites}");
    assert!(cites.contains("13.520.001"), "{cites}");
}

#[test]
fn a_probes_own_claim_about_its_machine_decides_nothing() {
    // Identical records, and the probe claims `console` in every case. The only thing that
    // differs is what the operator asserted - which is the whole point, because the claim
    // is not evidence and therefore cannot be what decides.
    let text = concat!(
        "OBS|hello|1|abc123|call,report\n",
        "OBS|part|abc123|target|console\n",
        "OBS|try|010-fs/open|libkernel|sceKernelOpen\n",
        "OBS|res|010-fs/open|pass|0x2||hardware\n"
    );
    let transcript = Transcript::read(text).expect("parses");
    assert_eq!(
        transcript
            .sessions
            .first()
            .and_then(orbistoun_probe::Session::claimed_target),
        Some("console"),
        "the claim is readable, as a claim"
    );

    let graded = |origin: &Origin| {
        transcript
            .findings(origin)
            .pop()
            .expect("one finding")
            .is_fact()
    };

    assert!(
        graded(&Origin::asserted("console", "13.520.001", true)),
        "the operator asserted real hardware, so this is a measurement"
    );
    assert!(
        !graded(&Origin::asserted("shadPS4", "", false)),
        "the same records, the same claim - and an emulator underneath"
    );
    assert!(
        !graded(&Origin::unasserted()),
        "and with nobody asserting anything, nothing is measured"
    );
}

#[test]
fn a_call_that_was_announced_and_never_concluded_is_not_a_failure() {
    // It is not a `fail`, and it must never be counted as one: the probe said what it was
    // about to do and did not come back, so nothing was concluded. Reporting it as a
    // failing check would record an outcome nobody observed.
    let text = std::fs::read_to_string(
        fixtures()
            .parent()
            .expect("fixtures root")
            .join("emulator-run.txt"),
    )
    .expect("fixture");
    let transcript = Transcript::read(&text).expect("parses");

    let unfinished = transcript.attempted_without_result();
    assert!(
        unfinished
            .iter()
            .any(|(check, _, symbol)| check == "040-file/open-rejects-null"
                && symbol == "sceKernelOpen"),
        "the run announced this check and never reported on it: {unfinished:?}"
    );

    // The distinction this test was written too coarsely to see at first: several checks
    // exercise one function, so the *symbol* can have a concluded result sitting beside an
    // unconcluded check. Here the missing-path case passed and the null-path case is the
    // last line in the file - the run ended inside it, which is precisely the failure the
    // handover notes warn about.
    assert!(
        transcript
            .findings(&Origin::asserted("console", "13.520.001", true))
            .iter()
            .any(|finding| finding.check == "040-file/open-rejects-missing"),
        "the other check on the same symbol did conclude"
    );
    assert!(
        !transcript
            .findings(&Origin::asserted("console", "13.520.001", true))
            .iter()
            .any(|finding| finding.check == "040-file/open-rejects-null"),
        "the check that did not conclude establishes nothing"
    );
}

#[test]
fn a_generated_entry_satisfies_the_knowledge_base_s_own_provenance_rules() {
    // The check that makes this conversion trustworthy: the knowledge base already knows
    // what a well-formed entry looks like, and it is asked rather than second-guessed.
    //
    // `provenance_faults` is the same function that fails the build for a hand-written
    // entry, so an entry generated from a probe record is held to exactly the standard a
    // person would be.
    let graded = |target: &str| {
        let text = format!(
            concat!(
                "OBS|hello|1|abc123|call,report\n",
                "OBS|part|abc123|target|{}\n",
                "OBS|part|abc123|firmware|13.520.001\n",
                "OBS|try|010-fs/open|libkernel|sceKernelOpen\n",
                "OBS|res|010-fs/open|pass|0x80020002|no such file|hardware\n"
            ),
            target
        );
        // The operator asserts the machine. `console` is asserted as real hardware;
        // anything else is not, which is what the grading turns on.
        let origin = Origin::asserted(target, "13.520.001", target == "console");
        let transcript = Transcript::read(&text).expect("parses");
        let finding = transcript.findings(&origin).pop().expect("one finding");
        (finding.knowledge(&origin), origin)
    };

    // Measured on the target: a fact, and it must carry its citation.
    let (entry, _) = graded("console");
    assert_eq!(entry.name, "sceKernelOpen");
    assert_eq!(
        entry.known_by,
        Some(orbistoun_hle::knowledge::Oracle::Measured)
    );
    assert!(entry.cites.contains("console"), "{}", entry.cites);
    assert!(
        entry.provenance_faults().is_empty(),
        "{:?}",
        entry.provenance_faults()
    );

    // The same observation from a stand-in: demoted, and now it must cite *nothing*. The
    // run it came from is still recorded - in the note and as an explicit assumption -
    // because where a guess came from is worth knowing and the citation field is reserved
    // for what has actually been established.
    let (entry, _) = graded("deck");
    assert_eq!(
        entry.known_by,
        Some(orbistoun_hle::knowledge::Oracle::Assumed)
    );
    assert!(
        entry.cites.is_empty(),
        "a citation beside a guess reads as evidence: {}",
        entry.cites
    );
    assert!(entry.note.contains("deck"), "{}", entry.note);
    assert!(
        entry.assumptions[0].contains("deck"),
        "the assumption names what the operator said it ran on: {:?}",
        entry.assumptions
    );
    assert!(
        entry
            .assumptions
            .iter()
            .any(|a| a.contains("did not assert real target hardware")),
        "{:?}",
        entry.assumptions
    );
    assert!(
        entry.provenance_faults().is_empty(),
        "{:?}",
        entry.provenance_faults()
    );
    // One stated question, counted once. This asserted two - the entry's own itemised
    // assumption *plus* a whole-function penalty for resting on a guess - which was the
    // double count that made `knows` report 80 open questions where `questions` reported
    // 70. An entry that itemises is counted by its items (D239).
    assert_eq!(
        entry.open_questions(),
        entry.assumptions.len(),
        "an entry that itemises is counted by its items, not its items plus a penalty"
    );
    assert_eq!(entry.open_questions(), 1, "and it states one");
}

#[test]
fn an_ungraded_record_produces_an_entry_that_claims_nothing() {
    // Most of the existing corpus is ungraded - the field arrived after those runs. Such a
    // record claims nothing, so the entry must not claim anything either, and in particular
    // must not be graded `assumed`: that is still a grade, and nobody assigned it.
    let text = "OBS|hello|1|abc123|call,report
OBS|part|abc123|target|console
OBS|try|010-fs/open|libkernel|sceKernelOpen
OBS|res|010-fs/open|pass|0x80020002|
";
    let origin = Origin::asserted("console", "13.520.001", true);
    let transcript = Transcript::read(text).expect("parses");
    let finding = transcript.findings(&origin).pop().expect("one finding");
    let entry = finding.knowledge(&origin);

    assert_eq!(
        entry.known_by, None,
        "no grade was given, so none is invented"
    );
    assert!(entry.cites.is_empty());
    assert!(entry.note.contains("ungraded"), "{}", entry.note);
    assert!(
        entry.provenance_faults().is_empty(),
        "an entry that claims no behaviour needs no provenance: {:?}",
        entry.provenance_faults()
    );
}

#[test]
fn a_symbol_resolving_is_a_fact_even_from_a_stand_in() {
    // The asymmetry that keeps symbols out of `Finding`.
    //
    // A return value depends on arguments, on state, and on the part. Existence does not: a
    // name that resolves resolves, so a `present` from a stand-in still establishes that
    // the name is spelled correctly and lives in that library - even though nothing it
    // returns there can be trusted for the target.
    //
    // Grading both the same way would either throw away a usable fact or promote an
    // unusable one, so they are separate types and only one is demoted by part.
    let text = std::fs::read_to_string(fixtures().join("03-died.txt")).expect("fixture");
    let transcript = Transcript::read(&text).expect("parses");

    let symbols = transcript.symbols(&Origin::unasserted());
    let found = symbols
        .iter()
        .find(|symbol| symbol.symbol == "sceKernelStat")
        .expect("the transcript resolves this symbol");
    assert_eq!(found.library, "libkernel");
    assert!(found.present);
    assert_eq!(found.availability.as_deref(), Some("shared"));
    assert_eq!(
        found.address, None,
        "a sym record carries no address, and must not invent one"
    );
}

#[test]
fn anything_that_is_not_the_word_present_is_not_a_claim_that_it_is() {
    // `presence` is read as an exact word rather than "not absent". A target that answers
    // something this version has never seen is saying something it does not understand, and
    // reading an unrecognised answer as `present` would invent the one fact the record was
    // being consulted for.
    use orbistoun_probe::{Line, Record};

    for (answer, expected) in [("present", true), ("absent", false), ("maybe", false)] {
        let line = format!("OBS|sym|libkernel|sceKernelOpen|{answer}|shared");
        let Ok(Line::Record(Record::Sym { .. })) = orbistoun_probe::parse_line(&line) else {
            panic!("a sym record should parse");
        };
        let transcript = Transcript::read(&line).expect("parses");
        assert_eq!(
            transcript.symbols(&Origin::unasserted())[0].present,
            expected,
            "presence answered {answer:?}"
        );
    }
}

#[test]
fn record_kinds_with_no_real_material_are_left_unparsed_on_purpose() {
    // `call`, `responsive`, `measure` and `progress` are in the record format's table and
    // appear in **no** output this project has ever seen - not in the captured exchanges,
    // not in the example report. Writing parsers for them would be transcribing a document
    // rather than reading evidence, which is the thing every other table here is built to
    // avoid.
    //
    // Nothing is lost by waiting: an unrecognised kind is kept verbatim, so material
    // arriving later is readable before anyone writes code for it. This test pins that,
    // and it is the reason the gap is safe rather than an oversight.
    use orbistoun_probe::{Line, Record};

    for line in [
        "OBS|call|libkernel|sceKernelOpen|0|returned|0x2",
        "OBS|measure|020-memory/allocate|sceKernelAllocateDirectMemory|bytes|4096|B",
    ] {
        let Ok(Line::Record(Record::Other { kind, fields })) = orbistoun_probe::parse_line(line)
        else {
            panic!("{line}: expected an unrecognised record kept verbatim");
        };
        assert!(!kind.is_empty());
        assert!(
            !fields.is_empty(),
            "the fields must be kept, not merely tolerated"
        );
    }
}

#[test]
fn a_skip_is_not_green() {
    // A skip is a check that did not run, so the section did not establish what it claims
    // to establish. Rounding one up to green is how a subsystem gets relied on for
    // something nobody tested - which is the same error as reading a death as a return
    // value, one level up.
    let text = "OBS|section|010-kernel|Kernel core|Whether the kernel answers at all.
OBS|sectiontally|010-kernel|3|0|0|0
OBS|section|035-libc|C runtime|Whether the C library behaves.
OBS|sectiontally|035-libc|5|0|0|2
";
    let sections = Transcript::read(text).expect("parses").sections();
    assert_eq!(sections.len(), 2);

    let kernel = &sections[0];
    assert!(kernel.is_wholly_green());
    assert_eq!(kernel.total(), 3);

    let libc = &sections[1];
    assert!(
        !libc.is_wholly_green(),
        "two checks did not run, so this section did not establish what it claims"
    );
    assert_eq!(libc.total(), 7, "a skip still counts in the denominator");
}

#[test]
fn a_section_missing_half_its_records_still_appears() {
    // A section with no tally, and a tally naming no section, are incomplete reports rather
    // than absent ones. Dropping either would shrink the denominator, which flatters the
    // result in the one direction nobody should be flattered.
    let text = "OBS|section|050-audio|Audio|Whether anything comes out.
OBS|sectiontally|060-input|1|0|1|0
";
    let sections = Transcript::read(text).expect("parses").sections();
    assert_eq!(sections.len(), 2, "both are reported: {sections:?}");

    let audio = sections
        .iter()
        .find(|s| s.id == "050-audio")
        .expect("audio");
    assert_eq!(audio.total(), 0);
    assert!(
        !audio.is_wholly_green(),
        "a section that reported no checks has established nothing, so it is not green"
    );

    let input = sections
        .iter()
        .find(|s| s.id == "060-input")
        .expect("input");
    assert!(input.title.is_empty(), "no section record described it");
    assert_eq!(input.total(), 2);
}

#[test]
fn the_real_report_shows_where_it_stopped() {
    // The per-area view earning its keep on real material. The filesystem section is the
    // one the run died inside: it reports no passes, and the check that never concluded is
    // in it. A single total would have shown neither.
    let text = std::fs::read_to_string(
        fixtures()
            .parent()
            .expect("fixtures root")
            .join("emulator-run.txt"),
    )
    .expect("fixture");
    let transcript = Transcript::read(&text).expect("parses");

    let sections = transcript.sections();
    assert!(sections.len() >= 8, "{} sections", sections.len());
    let file = sections
        .iter()
        .find(|section| section.id == "040-file")
        .expect("the filesystem section");
    assert_eq!(file.pass, 0, "nothing in it passed");

    let unfinished = transcript.attempted_without_result();
    assert!(
        unfinished
            .iter()
            .any(|(check, _, _)| check.starts_with("040-file/")),
        "and the check that never concluded is in that section: {unfinished:?}"
    );
}

#[test]
fn a_read_arrives_in_chunks_and_is_assembled_by_offset() {
    // This test used to assert the opposite, and that was the point of it.
    //
    // The captured exchange carried sixty-five hexadecimal digits for a thirty-two byte
    // read - one character spare, so the run could not be a whole number of bytes. Rather
    // than work around it, the defect was pinned as a *passing* test, so that correcting it
    // upstream would fail here and say what to replace it with. It did exactly that.
    //
    // Kept, inverted, as the guard on the shape that actually matters.
    //
    // # The shape
    //
    // A read longer than sixteen bytes comes back as **several `bytes` records**, each no
    // more than sixteen bytes, carrying an ascending decimal offset. The consumer
    // concatenates them by offset; `done|returned|<len>` is the total, not the size of any
    // one record.
    //
    // Assembling by offset rather than by arrival order is the part worth pinning. Arrival
    // order is very nearly always offset order, which is exactly what makes a consumer that
    // relies on it work right up until it does not.
    let text = std::fs::read_to_string(fixtures().join("06-read.txt")).expect("fixture");
    let transcript = Transcript::read(&text).expect("parses");
    let memory = transcript.memory();

    assert_eq!(
        memory.undecodable, 0,
        "every run is a whole number of bytes"
    );
    assert_eq!(
        memory.bytes.len(),
        32,
        "two sixteen-byte chunks, assembled into the thirty-two bytes that were asked for"
    );
    assert_eq!(memory.address, Some(0x8003_f510));

    // The first chunk, then the second - and the boundary is where the offsets say, not
    // where the records happened to appear.
    assert_eq!(&memory.bytes[..4], &[0x35, 0x00, 0x00, 0x00]);
    assert_eq!(&memory.bytes[16..20], &[0x37, 0x00, 0x00, 0x00]);

    // And the command's own answer is the total length rather than a chunk's.
    let read = transcript
        .exchanges
        .iter()
        .find(|exchange| exchange.verb == "read" && exchange.answered())
        .expect("the successful read");
    assert_eq!(read.outcome.as_ref().and_then(Outcome::value), Some(0x20));
}

#[test]
fn chunks_are_assembled_by_offset_even_when_they_arrive_out_of_order() {
    // The property the fixture cannot demonstrate, because its chunks arrive in order.
    //
    // Nothing in the protocol promises they will, and a consumer that concatenated in
    // arrival order would be right almost always - which is the worst kind of wrong, since
    // the one time it is not right produces a buffer that is the correct length, full of
    // real bytes, in the wrong sequence.
    let text = concat!(
        "OBS|bytes|read/0x1000|(memory)|contents|16|deadbeefdeadbeefdeadbeefdeadbeef
",
        "OBS|bytes|read/0x1000|(memory)|contents|0|00112233445566778899aabbccddeeff
"
    );
    let memory = Transcript::read(text).expect("parses").memory();

    assert_eq!(memory.bytes.len(), 32);
    assert_eq!(
        &memory.bytes[..4],
        &[0x00, 0x11, 0x22, 0x33],
        "offset zero comes first however it arrived"
    );
    assert_eq!(&memory.bytes[16..20], &[0xde, 0xad, 0xbe, 0xef]);
}

#[test]
fn the_three_ways_of_not_knowing_stay_three_things() {
    // All three states can carry the value `unknown`, and they mean entirely different
    // things: the platform has no such query, the probe has not wired one up yet, or here
    // is a real reading. Collapsing them keeps the least useful part of the record - a
    // consumer would show one blank where there are three distinct findings, and only one
    // of them is anybody's bug.
    use orbistoun_probe::Confidence;

    let text = concat!(
        "OBS|sysinfo|memory|known|441M\n",
        "OBS|sysinfo|firmware|unconfirmed|unknown\n",
        "OBS|sysinfo|temp|absent|unknown\n",
        "OBS|sysinfo|weather|drizzling|unknown\n"
    );
    let report = Transcript::read(text).expect("parses").self_report();
    assert_eq!(report.len(), 4);

    assert_eq!(report[0].confidence, Confidence::Known);
    assert!(report[0].confidence.is_reading());
    assert_eq!(report[0].value, "441M");

    // Two different explanations for the same word.
    assert_eq!(report[1].confidence, Confidence::Unconfirmed);
    assert_eq!(report[2].confidence, Confidence::Absent);
    assert_eq!(report[1].value, report[2].value, "the values are identical");
    assert_ne!(
        report[1].confidence, report[2].confidence,
        "and the records are not - the probe's unfinished wiring is not a platform gap"
    );
    assert!(!report[1].confidence.is_reading());
    assert!(!report[2].confidence.is_reading());

    // A state this version has never seen stays unrecognised rather than being resolved
    // into one of the others. Reading it as `absent` would blame the platform for something
    // it may well do; reading it as `known` would treat an unknown confidence as a reading.
    assert_eq!(
        report[3].confidence,
        Confidence::Unrecognised("drizzling".to_owned())
    );
    assert!(!report[3].confidence.is_reading());
}

#[test]
fn the_targets_account_of_itself_never_reaches_a_grade() {
    // `sysinfo` is observation, not provenance. Inside an emulator every field answers as
    // the emulator chooses - `memory|known|441M` is that emulator's number wearing the
    // target's badge, which is the self-reported-firmware trap one layer along.
    //
    // The separation is structural rather than a habit: nothing in `Origin` can be reached
    // from a record, so a future reader cannot wire one to the other by accident.
    let text = concat!(
        "OBS|hello|1|abc123|report\n",
        "OBS|sysinfo|generation|known|5\n",
        "OBS|sysinfo|firmware|known|13.520.001\n",
        "OBS|try|010-fs/open|libkernel|sceKernelOpen\n",
        "OBS|res|010-fs/open|pass|0x2||hardware\n"
    );
    let transcript = Transcript::read(text).expect("parses");
    assert_eq!(transcript.self_report().len(), 2, "both are read");

    // The target says it is a console on that firmware. Nobody asked the operator, so
    // nothing is measured.
    let unasserted = transcript.findings(&Origin::unasserted());
    assert!(
        !unasserted[0].is_fact(),
        "a target claiming its own generation and firmware settles nothing"
    );

    // And with the operator saying otherwise, the operator wins.
    let emulator = Origin::asserted("shadPS4", "", false);
    assert!(!transcript.findings(&emulator)[0].is_fact());
}

#[test]
fn an_unknown_outcome_degrades_without_ever_becoming_an_answer() {
    // Two rules meeting, and neither yielding.
    //
    // Report enum values are OPEN: obSCEne may add an outcome word without bumping the
    // format version, so a reader that refused the line would break on a stream it was told
    // to expect. It has to parse.
    //
    // And a command that did not answer is NEVER recorded as having answered. An outcome
    // nobody here understands has not been understood, so it cannot be a result.
    //
    // Both hold: the line parses, and the outcome carries no value and does not answer.
    // Degrading is not the same as assuming the best.
    use orbistoun_probe::{Line, Record};

    let Ok(Line::Record(Record::Done { outcome, .. })) =
        orbistoun_probe::parse_line("OBS|done|7|evaporated||the probe wandered off")
    else {
        panic!("an unrecognised outcome must still parse");
    };
    assert_eq!(
        outcome,
        Outcome::Unrecognised("evaporated".to_owned()),
        "kept verbatim so a reader can see what it was"
    );
    assert!(!outcome.answered(), "not understood is not answered");
    assert_eq!(outcome.value(), None, "and it carries no result");

    // It came *from* the probe, so the probe observed something - this reader simply cannot
    // say what. That is a different fact from silence and must not be filed with it.
    assert_eq!(outcome.observed_by(), ObservedBy::Probe);
}

#[test]
fn a_grade_this_version_cannot_read_is_not_the_same_as_no_grade() {
    // An absent field means the record predates grading and claims nothing. An unrecognised
    // value means the record claims something and this reader cannot say what.
    //
    // Both end ungraded - but only one of them means *the consumer is out of date*, and
    // filing them together would hide the signal that this crate needs updating.
    use orbistoun_hle::knowledge::Oracle;
    use orbistoun_probe::Provenance;

    assert_eq!(Provenance::parse(""), None, "an empty field is absent");
    assert_eq!(
        Provenance::parse("triangulated"),
        Some(Provenance::Unrecognised("triangulated".to_owned())),
        "a value that was given and not understood is not absence"
    );

    // And it grades as the weakest thing available, never the strongest: an unknown word
    // must not become a measurement on the strength of being unfamiliar.
    let asserted = Origin::asserted("console", "13.520.001", true);
    assert_eq!(
        Provenance::Unrecognised("triangulated".to_owned()).oracle(&asserted),
        Oracle::Assumed,
        "even with the operator asserting real hardware"
    );
}

#[test]
fn generation_says_both_rather_than_collapsing_to_unknown() {
    // The correction this thread asked for, arriving on the wire.
    //
    // `both` is a positive observation - two driver stacks present, the fingerprint of a
    // stub-everything loader as much as of real back-compatibility - and it is now distinct
    // from `absent|unknown`, which is the absence. They were the same record before.
    //
    // `both` deliberately names no console: presence is not implementation, and naming one
    // from a stub is the bug the field was corrected to stop making.
    use orbistoun_probe::Confidence;

    let text = concat!(
        "OBS|sysinfo|generation|known|both\n",
        "OBS|sysinfo|storage|absent|unknown\n"
    );
    let report = Transcript::read(text).expect("parses").self_report();

    assert_eq!(report[0].confidence, Confidence::Known);
    assert_eq!(report[0].value, "both");
    assert!(
        report[0].confidence.is_reading(),
        "`both` is something the target established, not something it failed to"
    );

    assert_eq!(report[1].confidence, Confidence::Absent);
    assert_ne!(
        (&report[0].confidence, &report[0].value),
        (&report[1].confidence, &report[1].value),
        "the two are distinct records now, which is the whole correction"
    );
}

/// The generation parenthetical is carried verbatim, whatever it says.
///
/// obSCEne changed `5 (current)` / `4 (previous)` to `5 (agc)` / `4 (gnm)` because the old
/// pair had an expiry date: "current" stops being true the day a sixth generation ships, and
/// an archived report cannot be corrected. The new parenthetical is the *evidence* - the
/// graphics driver the inference keyed on.
///
/// **This test exists because that change is the one the open-enum rule does not cover.**
/// Report enum values may be appended without a version bump; changing an existing value is
/// a different act. It cost nothing here only because this reader never parsed the
/// parenthetical, and pinning that is what stops somebody adding a parse later and quietly
/// re-acquiring the expiry date (bridge, obSCEne D147).
#[test]
fn the_generation_parenthetical_is_carried_verbatim() {
    for (wire, expected) in [
        ("OBS|sysinfo|generation|known|5 (agc)\n", "5 (agc)"),
        ("OBS|sysinfo|generation|known|4 (gnm)\n", "4 (gnm)"),
        // The old spellings must still read, because archived transcripts carry them and a
        // report should still be readable when it is read.
        ("OBS|sysinfo|generation|known|5 (current)\n", "5 (current)"),
        ("OBS|sysinfo|generation|known|both\n", "both"),
    ] {
        let transcript = Transcript::read(wire).expect("a well-formed record");
        let report = transcript.self_report();
        let field = report
            .iter()
            .find(|f| f.field == "generation")
            .unwrap_or_else(|| panic!("{wire:?} produced no generation field"));
        assert_eq!(
            field.value, expected,
            concat!(
                "the value must survive unparsed - a reader that interprets it acquires an ",
                "expiry date"
            )
        );
    }
}

#[test]
fn a_stand_in_is_real_hardware_and_is_still_not_the_target() {
    // The bug this rename fixed, pinned so it cannot come back.
    //
    // The field was called `real_hardware`, and a Steam Deck **is** real hardware. Somebody
    // connecting one and reading that name accurately would have asserted it, and every
    // Deck measurement would have been graded as a fact about the console - the silent
    // promotion the whole mechanism exists to prevent, reachable by an honest reading of
    // the field's own name.
    //
    // The question was never whether the silicon was real. It is whether the silicon was
    // the thing being emulated.
    use orbistoun_probe::Origin;

    for stand_in in ["deck", "Steam Deck", "shadPS4", "host", "some-new-emulator"] {
        assert!(
            Origin::is_known_stand_in(stand_in) || stand_in == "some-new-emulator",
            "{stand_in} should be recognised as a stand-in"
        );
    }

    // And the list names stand-ins rather than targets on purpose: something nobody has
    // listed is not promoted by default. A wrong demotion is recoverable; the other
    // direction corrupts a knowledge base.
    assert!(
        !Origin::is_known_stand_in("console"),
        "the target itself is not on the stand-in list"
    );
    // A name matching nothing on the list is not recognised, and that is the case the
    // caller must handle rather than relying on this. Substring matching catches anything
    // containing `emulator`, which is broad and still not exhaustive - somebody will name
    // one something else, and the safe default has to live at the call site.
    assert!(
        !Origin::is_known_stand_in("mystery-box"),
        "an unlisted name is not recognised here, so the caller defaults it to `not the target`"
    );
}

#[test]
fn a_handle_is_recorded_and_not_handed_to_the_guest() {
    // The carve-out D225 turns on, and the reason it keys on the return kind rather than on
    // whether a function "looks pure".
    //
    // A status code means the same thing in any address space. A handle does not: the
    // console hands back a value from its own, the guest dereferences it, and dies somewhere
    // unrelated hours later. Certainly wrong, and it looks right - the one failure this
    // project has no cheap detector for.
    use orbistoun_hle::knowledge::Returns;
    use orbistoun_probe::{Use, usable};

    assert_eq!(usable(Some(Returns::Status)), Use::Return);

    // Everything that is not a plain status is recorded only - including, and especially,
    // a return kind nobody has established. Not knowing what a function returns is exactly
    // when handing its value over is most dangerous.
    assert_eq!(usable(None), Use::RecordOnly, "unknown is not permission");
    for kind in [Returns::Handle, Returns::Pointer] {
        assert_eq!(usable(Some(kind)), Use::RecordOnly, "{kind:?}");
    }
}

#[test]
fn a_live_answer_records_the_divergence_beside_the_measurement() {
    // The measurement is real - a console ran that function with those arguments and
    // returned that value. What is *not* established is that the guest would have seen the
    // same answer, because the probe's process has not done what the guest did.
    //
    // A weaker grade would lose the first half; a bare `measured` would lose the second. So
    // the grade stays and the caveat travels beside it, where it also counts as a worklist
    // item rather than a footnote.
    use orbistoun_probe::{Asked, Origin, Outcome, Use};

    let asked = Asked {
        symbol: "sceKernelOpen".to_owned(),
        arguments: vec![0x8100_0000, 0x2],
        outcome: Outcome::Returned(0x8002_0002),
        usable: Use::Return,
    };
    let entry = asked.knowledge(&Origin::asserted("console", "13.520.001", true));

    assert_eq!(entry.name, "sceKernelOpen");
    assert_eq!(
        entry.known_by,
        Some(orbistoun_hle::knowledge::Oracle::Measured)
    );
    assert!(
        entry.edge_cases[0].contains("0x81000000") && entry.edge_cases[0].contains("0x80020002"),
        concat!(
            "the arguments belong with the value - one without the other is not a fact about a ",
            "function: {:?}"
        ),
        entry.edge_cases
    );
    assert!(
        entry
            .assumptions
            .iter()
            .any(|a| a.contains("probe's state rather than the guest's")),
        "{:?}",
        entry.assumptions
    );
    assert!(
        entry.provenance_faults().is_empty(),
        "{:?}",
        entry.provenance_faults()
    );
}

#[test]
fn a_recorded_only_answer_says_why_it_was_withheld() {
    use orbistoun_probe::{Asked, Origin, Outcome, Use};

    let entry = Asked {
        symbol: "sceKernelAllocateDirectMemory".to_owned(),
        arguments: vec![0x1000],
        outcome: Outcome::Returned(0x8804_0000),
        usable: Use::RecordOnly,
    }
    .knowledge(&Origin::asserted("console", "13.520.001", true));

    assert!(
        entry
            .assumptions
            .iter()
            .any(|a| a.contains("recorded and not handed to the guest")),
        concat!(
            "a value withheld should say why, or the next reader will wonder if it was lost: ",
            "{:?}"
        ),
        entry.assumptions
    );
}

#[test]
fn a_call_that_died_records_the_death_and_never_a_value() {
    // The shape of most first attempts. Asking killed the probe, which is a fact about the
    // function worth keeping - and there is no value, so none is written.
    use orbistoun_probe::{Asked, Origin, Outcome, Use};

    let entry = Asked {
        symbol: "sceKernelOpen".to_owned(),
        arguments: vec![0, 0],
        outcome: Outcome::Died,
        usable: Use::Return,
    }
    .knowledge(&Origin::asserted("console", "13.520.001", true));

    assert!(entry.edge_cases.is_empty(), "a death establishes no value");
    assert_eq!(entry.known_by, None, "and it grades nothing");
    assert!(
        entry
            .assumptions
            .iter()
            .any(|a| a.contains("did not answer")),
        "{:?}",
        entry.assumptions
    );
    assert!(
        entry.provenance_faults().is_empty(),
        "an entry claiming no behaviour needs no provenance: {:?}",
        entry.provenance_faults()
    );
}

/// The probe's by-name census reaches this reader as an existence fact.
///
/// # Why this test is worth more than it looks
///
/// obSCEne emits **two** records with the same first three fields: `sym`, whose fourth
/// field is how the symbol is reached, and `resolve`, whose fourth is where it landed.
/// This reader had an arm for `sym` only, so every `resolve` record was carried as
/// `Record::Other` - kept, correctly, and contributing no symbol fact at all.
///
/// That is the census. It answers for symbols **no title imports**, which is the one thing
/// no collision search over this repository's own candidates can ever reach, and it was
/// arriving and going nowhere (D245).
///
/// The line below is the exact shape `obs_report_resolve` writes: `OBS`, the kind, then
/// library, symbol, `present`/`absent`, address.
#[test]
fn a_resolve_record_is_read_as_an_existence_fact() {
    // Joined rather than written as one literal with `\` continuations: those collapse
    // under `cargo fmt` and bake the source indentation into the string, which this
    // project has a check for and which broke this test's first draft (D184).
    let text = [
        "OBS|resolve|libkernel|sceKernelAllocateDirectMemory|present|0x8000a1c0",
        "OBS|resolve|libkernel|sceKernelNoSuchThing|absent|0x0",
        "OBS|sym|libc|printf|present|shared",
    ]
    .join(
        "
",
    );
    let transcript = Transcript::read(&text).expect("the transcript parses");

    let symbols = transcript.symbols(&Origin::unasserted());
    assert_eq!(symbols.len(), 3, "both kinds contribute: {symbols:?}");

    let resolved = symbols
        .iter()
        .find(|s| s.symbol == "sceKernelAllocateDirectMemory")
        .expect("the census record is a symbol fact");
    assert!(resolved.present);
    assert_eq!(resolved.address.as_deref(), Some("0x8000a1c0"));
    assert_eq!(
        resolved.availability, None,
        "a resolve record does not say how it is reached, and must not claim to"
    );

    // Absence is a fact too, and the one that costs a candidate list nothing to check.
    let absent = symbols
        .iter()
        .find(|s| s.symbol == "sceKernelNoSuchThing")
        .expect("present");
    assert!(!absent.present);

    // And the older record still behaves, carrying availability and no address.
    let sym = symbols
        .iter()
        .find(|s| s.symbol == "printf")
        .expect("present");
    assert_eq!(sym.availability.as_deref(), Some("shared"));
    assert_eq!(sym.address, None);
}

/// A stand-in cannot name anything, however confidently it resolves.
///
/// # The channel this closes
///
/// D242 refuses name lists mined from other emulator projects. A `resolve` answered by one
/// of those emulators is that same list speaking: its symbol table is where the mined names
/// went. Ungraded, "present on shadPS4" would have entered this project as a probe
/// measurement - the strongest provenance it has - having come from the source the rule
/// exists to exclude.
///
/// The grading input is already in the transcript. `Origin::is_target` asks whether the
/// silicon was *the thing being emulated*, which is the right question and not the same as
/// whether it was real hardware (D246).
#[test]
fn only_the_target_may_source_a_name() {
    let text = [
        "OBS|resolve|libkernel|sceKernelSomething|present|0x8000a1c0",
        "OBS|sym|libc|printf|present|shared",
    ]
    .join(
        "
",
    );
    let transcript = Transcript::read(&text).expect("parses");

    for origin in [
        Origin::unasserted(),
        // Real silicon, and not the thing being emulated. The distinction the field name
        // was changed to make.
        Origin::asserted("steam deck", "", false),
    ] {
        for fact in transcript.symbols(&origin) {
            assert!(
                !fact.may_source_a_name(),
                "{} sourced a name from {:?}",
                fact.symbol,
                origin.device
            );
        }
    }

    // The console itself, and only then.
    let on_target = Origin::asserted("PS5", "", true);
    let facts = transcript.symbols(&on_target);
    assert_eq!(facts.len(), 2);
    assert!(
        facts.iter().all(SymbolFact::may_source_a_name),
        "a present from the target is a naming source: {facts:?}"
    );

    // Absence never sources a name either - there is no name in it to take.
    let absent =
        Transcript::read("OBS|resolve|libkernel|sceKernelNothing|absent|0x0").expect("parses");
    assert!(
        !absent.symbols(&on_target)[0].may_source_a_name(),
        "an absent symbol names nothing"
    );
}
