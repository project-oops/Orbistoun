//! Driving a session, without a socket.
//!
//! # Why none of this opens a connection
//!
//! The client talks to anything that reads and writes bytes, so these drive it over
//! in-memory buffers. That is not a convenience: **CI must never require a socket or a
//! plugged-in console**, and the paths worth testing are the ones where the far end stops
//! answering - which are unreachable from a happy-path run against real hardware even when
//! hardware is to hand.
//!
//! A probe that dies mid-command is the normal case, not the exceptional one. These are the
//! tests for the normal case.

use std::io::{Cursor, Read, Write};
use std::time::Duration;

use orbistoun_probe::client::{Client, ClientError, DEFAULT_PORT};
use orbistoun_probe::{Capability, Outcome, Refusal};

/// A stream that replays canned lines and records what was written to it.
///
/// Stands in for a socket. Reading returns whatever the probe would have said; writing is
/// captured so a test can assert what the client actually put on the wire, which is where
/// the sequence-number rules live.
struct Fake {
    incoming: Cursor<Vec<u8>>,
    outgoing: Vec<u8>,
}

impl Fake {
    fn new(script: &str) -> Self {
        Self {
            incoming: Cursor::new(script.as_bytes().to_vec()),
            outgoing: Vec::new(),
        }
    }
}

impl Read for Fake {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.incoming.read(buffer)
    }
}

impl Write for Fake {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.outgoing.extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn budget() -> Duration {
    Duration::from_secs(5)
}

#[test]
fn negotiation_reads_the_session_and_what_the_probe_can_do() {
    let mut client = Client::new(
        Fake::new(concat!(
            "OBS|ack|1|hello\n",
            "OBS|hello|1|abc123|report\n",
            "OBS|part|abc123|target|console\n",
            "OBS|done|1|ok||\n"
        )),
        budget(),
    );

    let session = client.hello(1, None).expect("negotiates");
    assert_eq!(session, "abc123");
    assert!(client.can(&Capability::Report));
    assert!(
        !client.can(&Capability::Call),
        "only what was announced, and `call` is reserved today"
    );
}

#[test]
fn a_verb_the_probe_never_announced_is_not_sent() {
    // The check belongs on this side. A client that sends anyway and waits to be refused
    // has already put a verb on the wire that this probe does not implement - and on a
    // target that faults easily, that is not a free thing to do.
    let mut client = Client::new(
        Fake::new(concat!(
            "OBS|ack|1|hello\n",
            "OBS|hello|1|abc123|report\n",
            "OBS|done|1|ok||\n"
        )),
        budget(),
    );
    client.hello(1, None).expect("negotiates");

    let refused = client.command("call", &["0x1000"]);
    assert!(
        matches!(&refused, Err(ClientError::NotNegotiated(verb)) if verb == "call"),
        "{refused:?}"
    );

    // And nothing went out: the wire carries the negotiation and nothing else.
    assert!(
        !client
            .transcript()
            .iter()
            .any(|line| line.contains("|call")),
        "{:?}",
        client.transcript()
    );
}

#[test]
fn a_command_acknowledged_then_cut_off_is_a_death_and_carries_no_value() {
    // The case the whole design exists for. `ack` is flushed before the command runs, so an
    // acknowledgement followed by a closed stream means exactly one thing: that command did
    // not return. It is not recorded as returning zero. It is not recorded as returning.
    let mut client = Client::new(
        Fake::new(concat!(
            "OBS|ack|1|hello\n",
            "OBS|hello|1|abc123|report\n",
            "OBS|done|1|ok||\n",
            "OBS|ack|2|report\n"
        )),
        budget(),
    );
    client.hello(1, None).expect("negotiates");

    let answer = client.report().expect("the call itself succeeds");
    assert_eq!(answer.outcome, Outcome::Died);
    assert_eq!(
        answer.outcome.value(),
        None,
        "a death has no value, and there is no field for one to hide in"
    );
    assert!(answer.detail.contains("after ack"), "{}", answer.detail);
}

#[test]
fn a_stream_that_closes_before_acknowledging_is_ambiguous_rather_than_a_death() {
    // Without an acknowledgement nothing establishes that the command ran at all, so this
    // is `lost` - recorded as the ambiguity it is rather than resolved into the more
    // specific answer, which would be a guess wearing an observation's clothes.
    let mut client = Client::new(
        Fake::new(concat!(
            "OBS|ack|1|hello\n",
            "OBS|hello|1|abc123|report\n",
            "OBS|done|1|ok||\n"
        )),
        budget(),
    );
    client.hello(1, None).expect("negotiates");

    let answer = client.report().expect("the call itself succeeds");
    assert_eq!(answer.outcome, Outcome::Lost);
    assert_eq!(answer.outcome.value(), None);
}

#[test]
fn silence_within_the_budget_is_a_timeout_and_not_a_death() {
    // A blocked call and a dead process look identical from one end of a socket. The record
    // says which was observed - silence - rather than which was guessed.
    let mut client = Client::new(
        Fake::new(concat!(
            "OBS|ack|1|hello\n",
            "OBS|hello|1|abc123|report\n",
            "OBS|done|1|ok||\n",
            "OBS|ack|2|report\n"
        )),
        // Zero budget: the deadline has passed before the first read, so the client gives
        // up rather than reading on to the end of the script.
        Duration::ZERO,
    );
    // Negotiation is exempt from nothing, so it times out too - which is itself the right
    // behaviour and is why the session is checked rather than assumed.
    let negotiated = client.hello(1, None);
    assert!(
        negotiated.is_err(),
        "with no budget at all even negotiation cannot complete"
    );
}

#[test]
fn sequence_numbers_are_the_clients_and_they_increase() {
    let mut client = Client::new(
        Fake::new(concat!(
            "OBS|ack|1|hello\n",
            "OBS|hello|1|abc123|report\n",
            "OBS|done|1|ok||\n",
            "OBS|ack|2|report\n",
            "OBS|done|2|ok||\n",
            "OBS|ack|3|bye\n",
            "OBS|done|3|ok||\n"
        )),
        budget(),
    );
    client.hello(1, None).expect("negotiates");
    client.report().expect("reports");
    client.bye().expect("closes");

    let sent: Vec<&String> = client
        .transcript()
        .iter()
        .filter(|line| line.starts_with("CMD|"))
        .collect();
    assert_eq!(
        sent,
        vec![
            &"CMD|1|hello|1".to_owned(),
            &"CMD|2|report".to_owned(),
            &"CMD|3|bye".to_owned()
        ]
    );
}

#[test]
fn a_refusal_is_reported_rather_than_read_as_an_answer() {
    let mut client = Client::new(
        Fake::new(concat!(
            "OBS|ack|1|hello\n",
            "OBS|hello|1|abc123|report,resolve\n",
            "OBS|done|1|ok||\n",
            "OBS|ack|2|resolve\n",
            "OBS|refused|2|unknown-verb\n"
        )),
        budget(),
    );
    client.hello(1, None).expect("negotiates");

    // Announced, so the client sends it - and today the probe reserves it and refuses.
    let refused = client.command("resolve", &["libkernel", "sceKernelOpen"]);
    assert!(
        matches!(refused, Err(ClientError::Refused(Refusal::UnknownVerb))),
        "{refused:?}"
    );
}

#[test]
fn the_transcript_can_be_replayed_by_the_reader() {
    // The session is transient and the corpus is the product. What the client saw has to be
    // readable by the same parser that reads a committed corpus, or the live path and the
    // file path would drift into two different truths.
    let mut client = Client::new(
        Fake::new(concat!(
            "OBS|ack|1|hello\n",
            "OBS|hello|1|abc123|report\n",
            "OBS|part|abc123|target|console\n",
            "OBS|done|1|ok||\n"
        )),
        budget(),
    );
    client.hello(1, None).expect("negotiates");

    let replayed = client.transcript().join("\n");
    let transcript = orbistoun_probe::Transcript::read(&replayed).expect("parses");
    assert_eq!(
        transcript
            .sessions
            .first()
            .map(|session| session.session.as_str()),
        Some("abc123")
    );
    assert_eq!(
        transcript
            .sessions
            .first()
            .and_then(orbistoun_probe::Session::claimed_target),
        Some("console"),
        "and what it claimed is carried through as a claim"
    );
}

#[test]
fn the_default_port_is_the_one_the_protocol_names() {
    assert_eq!(DEFAULT_PORT, 9803);
}

#[test]
fn a_call_that_returns_yields_its_value_and_one_that_dies_yields_none() {
    // `call` is live now, and the two outcomes are not variations on a theme. A well-formed
    // but fatal address is *called* - the probe executes it and dies - so it arrives as an
    // acknowledgement with no result. Null is called rather than rejected, because "what
    // does this platform do when you call null" is a real question with a real answer.
    let mut client = Client::new(
        Fake::new(concat!(
            "OBS|ack|1|hello\n",
            "OBS|hello|1|abc123|call,read,report\n",
            "OBS|done|1|ok||\n",
            "OBS|ack|2|call\n",
            "OBS|done|2|returned|0x2a|\n",
            "OBS|ack|3|call\n"
        )),
        budget(),
    );
    client.hello(1, None).expect("negotiates");

    let returned = client.call(0x8001_24a0, &[]).expect("sends");
    assert_eq!(returned.outcome, Outcome::Returned(0x2a));
    assert_eq!(returned.outcome.value(), Some(0x2a));

    // And the null call, which is the one the feedback was explicit about.
    let died = client.call(0, &[0, 0]).expect("sends");
    assert_eq!(died.outcome, Outcome::Died);
    assert_eq!(
        died.outcome.value(),
        None,
        "calling null killed the probe; it did not return zero"
    );

    let sent: Vec<&String> = client
        .transcript()
        .iter()
        .filter(|line| line.starts_with("CMD|") && line.contains("|call"))
        .collect();
    assert_eq!(
        sent,
        vec![
            &"CMD|2|call|0x800124a0".to_owned(),
            &"CMD|3|call|0x0|0x0|0x0".to_owned()
        ],
        "addresses and arguments go out as hexadecimal"
    );
}

#[test]
fn a_read_returns_bytes_and_a_bad_address_can_answer_either_way() {
    // Two legitimate answers for an address that cannot be read, and they are different
    // facts. A platform that can test before touching answers `unmapped`; one that cannot
    // faults, and that arrives as a death. The serving build today does not pre-validate,
    // so a caller must handle both - which build is on the other end is not knowable here.
    let mut client = Client::new(
        Fake::new(concat!(
            "OBS|ack|1|hello\n",
            "OBS|hello|1|abc123|call,read,report\n",
            "OBS|done|1|ok||\n",
            "OBS|ack|2|read\n",
            "OBS|bytes|read/0x8003f510|(memory)|contents|0|7f454c46\n",
            "OBS|done|2|returned|0x4|\n",
            "OBS|ack|3|read\n"
        )),
        budget(),
    );
    client.hello(1, None).expect("negotiates");

    let answer = client.read(0x8003_f510, 4).expect("sends");
    assert_eq!(answer.outcome, Outcome::Returned(4));
    let transcript = orbistoun_probe::Transcript::read(&client.transcript().join("\n"))
        .expect("the wire replays");
    let memory = transcript.memory();
    assert_eq!(
        memory.bytes,
        vec![0x7f, 0x45, 0x4c, 0x46],
        "the ELF magic, read off a live target"
    );
    assert_eq!(memory.address, Some(0x8003_f510), "and where it came from");

    // The same request against an address this build cannot test first.
    let died = client.read(0xdead_0000, 0x10).expect("sends");
    assert_eq!(
        died.outcome,
        Outcome::Died,
        concat!(
            "this build faults rather than pre-validating - and that is a different fact from ",
            "`unmapped`, not a worse spelling of it"
        )
    );
}

#[test]
fn a_read_that_pre_validates_is_refused_rather_than_fatal() {
    // The other half of the same question, so the consumer is pinned to distinguishing them
    // rather than merely tolerating whichever it met first.
    let mut client = Client::new(
        Fake::new(concat!(
            "OBS|ack|1|hello\n",
            "OBS|hello|1|abc123|read,report\n",
            "OBS|done|1|ok||\n",
            "OBS|ack|2|read\n",
            "OBS|refused|2|unmapped\n"
        )),
        budget(),
    );
    client.hello(1, None).expect("negotiates");

    let refused = client.read(0xdead_0000, 0x10);
    assert!(
        matches!(&refused, Err(ClientError::Refused(Refusal::Unmapped))),
        "{refused:?}"
    );
}

#[test]
fn half_a_byte_is_not_a_byte() {
    // An odd number of hexadecimal digits is refused rather than rounded. Guessing which
    // half was meant would put a value in a buffer that nothing observed, and a buffer is
    // exactly where an invented value is least visible.
    let text = concat!(
        "OBS|bytes|read/0x1000|(memory)|contents|0|7f454c4\n",
        "OBS|bytes|read/0x1000|(memory)|contents|4|deadbeef\n"
    );
    let memory = orbistoun_probe::Transcript::read(text)
        .expect("parses")
        .memory();
    assert_eq!(
        memory.undecodable, 1,
        "the ragged run is counted, not dropped"
    );
    assert_eq!(
        memory.bytes,
        vec![0xde, 0xad, 0xbe, 0xef],
        "and the run that did decode is still returned"
    );
}

#[test]
fn a_streamed_report_arrives_between_the_acknowledgement_and_the_answer() {
    // `report` streams its full record set over the socket now, rather than only a summary.
    // That makes this the primary way records arrive, so it is worth proving end to end
    // rather than assuming the collection path handles it.
    //
    // Nothing needed changing to support it, which is the point: records between `ack` and
    // `done` were always collected, and a consumer that ignores kinds it does not recognise
    // needs no change when new ones appear. This test is what makes that claim checkable.
    let mut client = Client::new(
        Fake::new(concat!(
            "OBS|ack|1|hello\n",
            "OBS|hello|1|abc123|call,read,report\n",
            "OBS|part|abc123|target|console\n",
            "OBS|done|1|ok||\n",
            "OBS|ack|2|report\n",
            "OBS|section|010-kernel|Kernel core|Whether the kernel answers at all.\n",
            "OBS|try|010-kernel/stat|libkernel|sceKernelStat\n",
            "OBS|res|010-kernel/stat|pass|0x0||hardware\n",
            "OBS|sym|libkernel|sceKernelStat|present|shared\n",
            "OBS|sectiontally|010-kernel|1|0|0|0\n",
            "OBS|tally|1|0|0|0\n",
            "OBS|done|2|ok||\n"
        )),
        budget(),
    );
    client.hello(1, None).expect("negotiates");

    let answer = client.report().expect("reports");
    assert_eq!(answer.outcome, Outcome::Ok);
    assert!(
        answer.records.len() >= 5,
        "the whole record set arrives, not a summary: {:?}",
        answer.records
    );

    // And the wire replays into the same reader a committed corpus goes through, so the
    // live path and the file path cannot become two different truths.
    let transcript = orbistoun_probe::Transcript::read(&client.transcript().join("\n"))
        .expect("the wire replays");

    let sections = transcript.sections();
    assert_eq!(sections.len(), 1);
    assert!(sections[0].is_wholly_green(), "{:?}", sections[0]);

    // The operator asserts the machine; the session's own claim decides nothing.
    let origin = orbistoun_probe::Origin::asserted("console", "13.520.001", true);
    let findings = transcript.findings(&origin);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].symbol, "sceKernelStat");
    assert!(
        findings[0].is_fact(),
        "measured, because the operator said so"
    );

    assert_eq!(
        transcript
            .symbols(&orbistoun_probe::Origin::unasserted())
            .len(),
        1
    );

    // `tally` is a kind this version does not model, and it survives rather than being
    // dropped - which is the property that lets obSCEne add records without breaking this.
    assert!(
        transcript
            .records
            .iter()
            .chain(transcript.exchanges.iter().flat_map(|e| e.records.iter()))
            .any(|record| matches!(
                record,
                orbistoun_probe::Record::Other { kind, .. } if kind == "tally"
            )),
        "an unmodelled record kind is kept verbatim"
    );
}

#[test]
fn nothing_is_expected_to_arrive_before_a_command_asks_for_it() {
    // A serving build is interactive-first: it listens immediately and the suite runs on
    // demand. A client that assumed a report on connect would block forever against a probe
    // that is behaving correctly, so negotiation must complete on negotiation alone.
    let mut client = Client::new(
        Fake::new(concat!(
            "OBS|ack|1|hello\n",
            "OBS|hello|1|abc123|call,read,report\n",
            "OBS|done|1|ok||\n"
        )),
        budget(),
    );

    let session = client
        .hello(1, None)
        .expect("negotiation completes with nothing else sent");
    assert_eq!(session, "abc123");
    assert!(
        client
            .transcript()
            .iter()
            .filter(|line| line.starts_with("CMD|"))
            .count()
            == 1,
        "one command went out - the suite is asked for, never assumed"
    );
}

#[test]
fn a_report_that_dies_partway_keeps_what_arrived_before_the_cut() {
    // A run is tens of thousands of records and a faulting check ends it. What arrived
    // before the fault was still observed - discarding it because the command did not
    // complete would throw away most of a run to report the last second of it.
    //
    // So the outcome is `died` and the records are kept. Both halves matter: keeping the
    // records without the death would read as a completed run, and the death without the
    // records would lose the evidence.
    //
    // Written because this was claimed to obSCEne as working before it was tested. It is
    // true of the implementation and was not pinned, which is the same gap this session has
    // found in other people's documents twice.
    let mut client = Client::new(
        Fake::new(concat!(
            "OBS|ack|1|hello\n",
            "OBS|hello|1|abc123|report\n",
            "OBS|done|1|ok||\n",
            "OBS|ack|2|report\n",
            "OBS|section|010-kernel|Kernel core|Whether the kernel answers.\n",
            "OBS|try|010-kernel/stat|libkernel|sceKernelStat\n",
            "OBS|res|010-kernel/stat|pass|0x0||hardware\n",
            "OBS|try|010-kernel/fatal|libkernel|sceKernelFatal\n"
        )),
        budget(),
    );
    client.hello(1, None).expect("negotiates");

    let answer = client.report().expect("the call itself succeeds");
    assert_eq!(
        answer.outcome,
        Outcome::Died,
        "the stream stopped after an ack with no done"
    );
    assert_eq!(answer.outcome.value(), None);
    assert_eq!(
        answer.records.len(),
        4,
        "everything before the cut is kept: {:?}",
        answer.records
    );

    // And the partial run still reads as a run - the section, the finding that concluded,
    // and the check that did not.
    let transcript = orbistoun_probe::Transcript::read(&client.transcript().join("\n"))
        .expect("the partial wire replays");
    assert_eq!(transcript.sections().len(), 1);

    let origin = orbistoun_probe::Origin::asserted("console", "13.520.001", true);
    let findings = transcript.findings(&origin);
    assert_eq!(findings.len(), 1, "one check concluded");
    assert!(findings[0].is_fact());

    let unfinished = transcript.attempted_without_result();
    assert!(
        unfinished
            .iter()
            .any(|(check, _, symbol)| check == "010-kernel/fatal" && symbol == "sceKernelFatal"),
        "and the check the run died inside is named: {unfinished:?}"
    );
}
