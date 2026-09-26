//! Every captured exchange, read against the protocol.
//!
//! The fixtures are copies of obSCEne's `docs/examples/protocol/`, since a test must not
//! read a sibling checkout (D207); a transcript is data, so copying it is the right kind
//! of duplication. They cover the whole grammar, including every path that is not a clean
//! answer: a call that dies, a timeout, a refusal, reset and a malformed sequence.

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
    // A line the grammar does not cover means an unreadable protocol or a drifted
    // transcript; either fails.
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
    // `died` is not `returned 0`: the non-answering variants carry no value, asserted across
    // every real transcript as well as in the type.
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
    // The other direction, so the rule above is not met by refusing everything.
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
    // A faulting command ends the probe, so one transcript holds two sessions, which must
    // not be merged.
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

    // The metadata follows the session it names, not whichever came last.
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
    // Every session records its target, so a number measured on a stand-in is never read
    // as authoritative for the real target.
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
    // A stand-in without the platform's libraries refuses a library question rather than
    // answering it wrongly.
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
    // A repeated sequence is not acknowledged: an `ack` is keyed by sequence, so a second
    // one would share the key.
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

    // A sequence that is not a number is readable rather than a parse failure, since the
    // protocol specifies that case.
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
    // The protocol permits new record kinds within a version; an unrecognised one is kept,
    // not discarded.
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
    // The type makes a value on `died` unrepresentable; a transcript that carries one fails.
    let line = "OBS|done|3|died|0x0|";
    let error =
        orbistoun_probe::parse_line(line).expect_err("a died outcome with a value must be refused");
    assert!(
        error.to_string().contains("did not answer"),
        "the refusal should say why: {error}"
    );
}

#[test]
fn a_hardware_result_is_measured_only_when_the_operator_asserted_is_target() {
    // Whether a number becomes a fact about the platform turns on what the operator
    // asserted, not on what the session claimed: a probe inside an emulator reports the
    // emulator's answers.
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

    // The rest of the mapping does not depend on the machine.
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
    // An absent provenance field is absent, not a default grade.
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
    // A committed report is records only, with no `CMD|` lines. It sits one directory up,
    // outside `protocol/`, so the grammar tests do not treat it as a transcript.
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
    // This report has no provenance field, so it establishes no facts.
    assert_eq!(established.ungraded, established.total());
    assert_eq!(
        established.facts(),
        0,
        "ungraded results are not facts, however many there are"
    );
}

#[test]
fn a_result_becomes_a_finding_only_when_something_named_the_function() {
    // A `res` names its check, never the function; the `try` before it does, and pairing
    // them says what a function returns. Constructed inline rather than filed beside the
    // captured fixtures, so it is never mistaken for evidence.
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

    // The citation says what the value was measured on.
    let origin = Origin::asserted("console", "13.520.001", true);
    let cites = finding.cites(&origin);
    assert!(cites.contains("console"), "{cites}");
    assert!(cites.contains("13.520.001"), "{cites}");
}

#[test]
fn a_probes_own_claim_about_its_machine_decides_nothing() {
    // Identical records claiming `console`; only the operator's assertion differs, and it
    // decides.
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
    // A check that never concluded is not a `fail`: nothing was observed.
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

    // Several checks exercise one function, so a symbol can have a concluded result beside
    // an unconcluded check: here the missing-path case passed and the run ended inside the
    // null-path case.
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
    // `provenance_faults`, which fails the build for a hand-written entry, also checks an
    // entry generated from a probe record.
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
        // The operator asserts the machine: only `console` is asserted as the target.
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

    // The same observation from a stand-in is demoted and cites nothing; its run is still
    // recorded in the note and as an explicit assumption.
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
    // One stated question, counted once: an entry that itemises its assumptions is counted
    // by its items, with no extra whole-function penalty.
    assert_eq!(
        entry.open_questions(),
        entry.assumptions.len(),
        "an entry that itemises is counted by its items, not its items plus a penalty"
    );
    assert_eq!(entry.open_questions(), 1, "and it states one");
}

#[test]
fn an_ungraded_record_produces_an_entry_that_claims_nothing() {
    // An ungraded record claims nothing, so its entry is not graded either, not even
    // `assumed`.
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
    // Existence does not depend on arguments or state, so symbols are their own type
    // rather than a `Finding`; a stand-in's result is still recorded, graded by origin.
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
    // `presence` is read as an exact word; an unrecognised answer is not `present`.
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
    // These kinds are in the record format's table but in no captured output, so they have
    // no parser; an unrecognised kind is kept verbatim and stays readable. A kind gets a
    // parser once real output carries it, as `measure` did (`tests/measure.rs`).
    use orbistoun_probe::{Line, Record};

    for line in [
        "OBS|call|libkernel|sceKernelOpen|0|returned|0x2",
        "OBS|progress|boot|reached|0x3|stage",
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
    // A skip is a check that did not run, so a section with one is not green.
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
    // A section with no tally, and a tally naming no section, are incomplete rather than
    // absent; dropping either would shrink the denominator.
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
    // The run died inside the filesystem section: it reports no passes and holds the check
    // that never concluded.
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
    // A read longer than sixteen bytes comes back as several `bytes` records of at most
    // sixteen bytes, each with an ascending decimal offset. The consumer assembles them by
    // offset, not arrival order; `done|returned|<len>` is the total length.
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

    // The first chunk, then the second, split where the offsets say.
    assert_eq!(&memory.bytes[..4], &[0x35, 0x00, 0x00, 0x00]);
    assert_eq!(&memory.bytes[16..20], &[0x37, 0x00, 0x00, 0x00]);

    // The command's own answer is the total length rather than a chunk's.
    let read = transcript
        .exchanges
        .iter()
        .find(|exchange| exchange.verb == "read" && exchange.answered())
        .expect("the successful read");
    assert_eq!(read.outcome.as_ref().and_then(Outcome::value), Some(0x20));
}

#[test]
fn chunks_are_assembled_by_offset_even_when_they_arrive_out_of_order() {
    // Chunks out of order, which the fixture cannot show: the protocol does not promise
    // arrival order.
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
    // All three confidence states can carry `unknown` with different meanings: no such
    // query on the platform, a query the probe does not implement, or a real reading.
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

    // A state this version has never seen stays unrecognised rather than becoming one of
    // the others.
    assert_eq!(
        report[3].confidence,
        Confidence::Unrecognised("drizzling".to_owned())
    );
    assert!(!report[3].confidence.is_reading());
}

#[test]
fn the_targets_account_of_itself_never_reaches_a_grade() {
    // `sysinfo` is observation, not provenance: inside an emulator every field answers as
    // the emulator chooses. Nothing in `Origin` is reachable from a record.
    let text = concat!(
        "OBS|hello|1|abc123|report\n",
        "OBS|sysinfo|generation|known|5\n",
        "OBS|sysinfo|firmware|known|13.520.001\n",
        "OBS|try|010-fs/open|libkernel|sceKernelOpen\n",
        "OBS|res|010-fs/open|pass|0x2||hardware\n"
    );
    let transcript = Transcript::read(text).expect("parses");
    assert_eq!(transcript.self_report().len(), 2, "both are read");

    // The target claims to be the hardware on that firmware; with no operator assertion,
    // nothing is measured.
    let unasserted = transcript.findings(&Origin::unasserted());
    assert!(
        !unasserted[0].is_fact(),
        "a target claiming its own generation and firmware settles nothing"
    );

    // With the operator saying otherwise, the operator wins.
    let emulator = Origin::asserted("shadPS4", "", false);
    assert!(!transcript.findings(&emulator)[0].is_fact());
}

#[test]
fn an_unknown_outcome_degrades_without_ever_becoming_an_answer() {
    // Report enum values are open, so an unknown outcome word parses; and an outcome not
    // understood is not a result, so it carries no value and does not answer.
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

    // It came from the probe, so the probe observed something, which is not silence.
    assert_eq!(outcome.observed_by(), ObservedBy::Probe);
}

#[test]
fn a_grade_this_version_cannot_read_is_not_the_same_as_no_grade() {
    // An absent field claims nothing; an unrecognised value claims something this reader
    // cannot parse. Both end ungraded, but only the second means this reader is out of date.
    use orbistoun_hle::knowledge::Oracle;
    use orbistoun_probe::Provenance;

    assert_eq!(Provenance::parse(""), None, "an empty field is absent");
    assert_eq!(
        Provenance::parse("triangulated"),
        Some(Provenance::Unrecognised("triangulated".to_owned())),
        "a value that was given and not understood is not absence"
    );

    // It grades as the weakest grade, never a measurement.
    let asserted = Origin::asserted("console", "13.520.001", true);
    assert_eq!(
        Provenance::Unrecognised("triangulated".to_owned()).oracle(&asserted),
        Oracle::Assumed,
        "even with the operator asserting real hardware"
    );
}

#[test]
fn generation_says_both_rather_than_collapsing_to_unknown() {
    // `both` is a positive observation of two driver stacks, distinct from `absent|unknown`.
    // It names no generation: a stub-everything loader shows it too, and presence is not
    // implementation.
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
/// Both spellings of the parenthetical read (`5 (current)` and `5 (agc)`), because the open
/// enum rule does not cover a changed existing value and archived reports keep the old one.
/// The parenthetical is evidence, never parsed.
#[test]
fn the_generation_parenthetical_is_carried_verbatim() {
    for (wire, expected) in [
        ("OBS|sysinfo|generation|known|5 (agc)\n", "5 (agc)"),
        ("OBS|sysinfo|generation|known|4 (gnm)\n", "4 (gnm)"),
        // The old spellings still read, because archived transcripts carry them.
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
    // The question is whether the silicon is the thing being emulated, not whether it is
    // real: a real stand-in device must not be graded as the target.
    use orbistoun_probe::Origin;

    for stand_in in ["deck", "Steam Deck", "shadPS4", "host", "some-new-emulator"] {
        assert!(
            Origin::is_known_stand_in(stand_in) || stand_in == "some-new-emulator",
            "{stand_in} should be recognised as a stand-in"
        );
    }

    // The list names stand-ins rather than targets, so an unlisted name is not promoted.
    assert!(
        !Origin::is_known_stand_in("console"),
        "the target itself is not on the stand-in list"
    );
    // A name matching nothing is not recognised; substring matching is not exhaustive, so
    // the safe default lives at the call site.
    assert!(
        !Origin::is_known_stand_in("mystery-box"),
        "an unlisted name is not recognised here, so the caller defaults it to `not the target`"
    );
}

#[test]
fn a_handle_is_recorded_and_not_handed_to_the_guest() {
    // Keyed on the return kind (D225): a status code means the same in any address space,
    // while a handle from the target's process is meaningless in the guest's.
    use orbistoun_hle::knowledge::Returns;
    use orbistoun_probe::{Use, usable};

    assert_eq!(usable(Some(Returns::Status)), Use::Return);

    // Everything that is not a plain status is recorded only, including an unestablished
    // return kind.
    assert_eq!(usable(None), Use::RecordOnly, "unknown is not permission");
    for kind in [Returns::Handle, Returns::Pointer] {
        assert_eq!(usable(Some(kind)), Use::RecordOnly, "{kind:?}");
    }
}

#[test]
fn a_live_answer_records_the_divergence_beside_the_measurement() {
    // The measurement is real, but the guest's process state differs from the probe's, so
    // the grade stays and a caveat travels beside it as an open question.
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
    // Asking killed the probe: a fact about the function, with no value written.
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
/// obSCEne emits `sym` (how a symbol is reached) and `resolve` (where it landed) with the
/// same first three fields; both are read. The census covers symbols no title imports. The
/// line below is the shape `obs_report_resolve` writes: `OBS`, the kind, library, symbol,
/// `present`/`absent`, address.
#[test]
fn a_resolve_record_is_read_as_an_existence_fact() {
    // Joined rather than one literal with `\` continuations, which bake the source
    // indentation into the string.
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

    // Absence is a fact too.
    let absent = symbols
        .iter()
        .find(|s| s.symbol == "sceKernelNoSuchThing")
        .expect("present");
    assert!(!absent.present);

    // The `sym` record carries availability and no address.
    let sym = symbols
        .iter()
        .find(|s| s.symbol == "printf")
        .expect("present");
    assert_eq!(sym.availability.as_deref(), Some("shared"));
    assert_eq!(sym.address, None);
}

/// A stand-in cannot name anything, however confidently it resolves.
///
/// A `resolve` answered by another emulator is that emulator's symbol table speaking, so
/// it must not source a name (D246). `Origin::is_target` asks whether the silicon was the
/// thing being emulated, not whether it was real hardware.
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
        // Real silicon, and not the thing being emulated.
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

    // The target itself, and only then.
    let on_target = Origin::asserted("PS5", "", true);
    let facts = transcript.symbols(&on_target);
    assert_eq!(facts.len(), 2);
    assert!(
        facts.iter().all(SymbolFact::may_source_a_name),
        "a present from the target is a naming source: {facts:?}"
    );

    // Absence never sources a name either.
    let absent =
        Transcript::read("OBS|resolve|libkernel|sceKernelNothing|absent|0x0").expect("parses");
    assert!(
        !absent.symbols(&on_target)[0].may_source_a_name(),
        "an absent symbol names nothing"
    );
}
