//! The responder side: what orbistoun says when a driver asks it the probe's questions.
//!
//! Every test here drives a `Cursor` over a scripted command stream and reads what came
//! back out of a `Vec`. No socket is opened, which is what lets these run in the gate on a
//! machine with nothing plugged into it (D016).

use std::io::{Cursor, Read, Write};

use orbistoun_probe::respond::{Answers, Responder, render};
use orbistoun_probe::{Capability, Outcome, Provenance, Record, Refusal, Status};

/// A stream that reads from a script and writes into a buffer.
///
/// Both halves in one type because `Responder` takes a single stream, exactly as a socket
/// is a single stream. Splitting them in the test would test a shape the real thing does
/// not have.
struct Wire {
    incoming: Cursor<Vec<u8>>,
    outgoing: Vec<u8>,
}

impl Wire {
    fn new(script: &str) -> Self {
        Self {
            incoming: Cursor::new(script.as_bytes().to_vec()),
            outgoing: Vec::new(),
        }
    }
}

impl Read for Wire {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.incoming.read(buffer)
    }
}

impl Write for Wire {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.outgoing.write(buffer)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.outgoing.flush()
    }
}

/// A backend that answers everything, so the framing can be tested without a guest.
#[derive(Default)]
struct Fake {
    secret: Option<String>,
    memory: Vec<u8>,
    calls: Vec<(u64, Vec<u64>)>,
}

impl Answers for Fake {
    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::Report, Capability::Call, Capability::Read]
    }
    fn session(&self) -> String {
        "test-session".to_owned()
    }
    fn secret(&self) -> Option<String> {
        self.secret.clone()
    }
    fn describe(&self) -> Vec<(String, String)> {
        vec![("kind".to_owned(), "emulator".to_owned())]
    }
    fn report(&mut self) -> Result<Vec<Record>, Refusal> {
        Ok(vec![Record::Sym {
            library: "libkernel".to_owned(),
            symbol: "sceKernelCreateSema".to_owned(),
            presence: "present".to_owned(),
            availability: "shared".to_owned(),
        }])
    }
    fn call(&mut self, address: u64, arguments: &[u64]) -> Result<(Outcome, String), Refusal> {
        self.calls.push((address, arguments.to_vec()));
        Ok((Outcome::Returned(0), "fake".to_owned()))
    }
    fn read(&mut self, address: u64, length: u64) -> Result<Vec<u8>, Refusal> {
        let start = usize::try_from(address).map_err(|_| Refusal::Unmapped)?;
        let length = usize::try_from(length).map_err(|_| Refusal::BadArgument)?;
        self.memory
            .get(start..start + length)
            .map(<[u8]>::to_vec)
            .ok_or(Refusal::Unmapped)
    }
}

/// Runs a script and returns the lines that came back.
fn serve(script: &str, answers: Fake) -> Vec<String> {
    let mut responder = Responder::new(Wire::new(script), answers);
    responder.serve().expect("the stream never fails");
    let wire = responder.into_stream();
    String::from_utf8(wire.outgoing)
        .expect("records are text")
        .lines()
        .map(str::to_owned)
        .collect()
}

// --- the writer -------------------------------------------------------------------------

/// Everything written can be read back as the same thing.
///
/// The one property that matters for a responder: obSCEne's parser is the audience, and
/// this crate's parser is the closest available stand-in for it. A record that survives the
/// round trip is one whose field order and token spellings agree with the reader that has
/// been checked against real transcripts.
#[test]
fn every_record_survives_being_written_and_read_back() {
    let records = vec![
        Record::Ack {
            seq: 7,
            verb: "call".to_owned(),
        },
        Record::Hello {
            version: 1,
            session: "s".to_owned(),
            capabilities: vec![Capability::Call, Capability::Other("future".to_owned())],
        },
        Record::Part {
            session: "s".to_owned(),
            key: "kind".to_owned(),
            value: "emulator".to_owned(),
        },
        Record::Done {
            seq: 7,
            outcome: Outcome::Returned(0x8002_0016),
            detail: "d".to_owned(),
        },
        Record::Done {
            seq: 8,
            outcome: Outcome::Died,
            detail: String::new(),
        },
        Record::Refused {
            seq: 9,
            reason: Refusal::Unauthorised,
        },
        Record::Build {
            build: "b".to_owned(),
            kind: "host".to_owned(),
        },
        Record::Sym {
            library: "libkernel".to_owned(),
            symbol: "sym".to_owned(),
            presence: "present".to_owned(),
            availability: "shared".to_owned(),
        },
        Record::Section {
            id: "01".to_owned(),
            title: "t".to_owned(),
            purpose: "p".to_owned(),
        },
        Record::SysInfo {
            field: "f".to_owned(),
            state: "known".to_owned(),
            value: "v".to_owned(),
        },
        Record::Sink {
            path: "/data/obs.txt".to_owned(),
        },
        Record::Bytes {
            id: "read/0x1000".to_owned(),
            source: "responder".to_owned(),
            kind: "memory".to_owned(),
            offset: 32,
            hex: "deadbeef".to_owned(),
        },
        Record::SectionTally {
            id: "01".to_owned(),
            pass: 1,
            partial: 2,
            fail: 3,
            skip: 4,
        },
        Record::Try {
            check: "01/a".to_owned(),
            library: "libkernel".to_owned(),
            symbol: "sym".to_owned(),
        },
        Record::Res {
            check: "01/a".to_owned(),
            status: Status::Partial,
            value: "0x0".to_owned(),
            detail: "d".to_owned(),
            provenance: Some(Provenance::Hardware),
        },
        Record::Other {
            kind: "unheard-of".to_owned(),
            fields: vec!["a".to_owned(), "b".to_owned()],
        },
    ];
    for record in &records {
        let line = render(record);
        let parsed = orbistoun_probe::parse(&line).expect("what we wrote parses");
        assert_eq!(parsed.len(), 1, "one line is one record: {line}");
        let orbistoun_probe::Line::Record(back) = &parsed[0] else {
            panic!("not a record: {line}");
        };
        assert_eq!(back, record, "round trip changed it: {line}");
    }
}

/// A record whose grade was absent stays absent.
///
/// Writing a default onto it would manufacture provenance, and a consumer cannot tell an
/// invented grade from a measured one - which is the failure this crate is arranged around.
#[test]
fn an_ungraded_result_is_not_given_a_grade_on_the_way_out() {
    let line = render(&Record::Res {
        check: "01/a".to_owned(),
        status: Status::Pass,
        value: String::new(),
        detail: String::new(),
        provenance: None,
    });
    assert!(line.ends_with('|'), "the grade field is empty: {line}");
    let parsed = orbistoun_probe::parse(&line).expect("parses");
    let orbistoun_probe::Line::Record(Record::Res { provenance, .. }) = &parsed[0] else {
        panic!("not a res");
    };
    assert!(provenance.is_none(), "it came back graded");
}

// --- the exchange -----------------------------------------------------------------------

#[test]
fn negotiation_answers_with_capabilities_and_what_this_is() {
    let lines = serve("CMD|1|hello|1\n", Fake::default());
    assert_eq!(lines[0], "OBS|ack|1|hello");
    assert_eq!(lines[1], "OBS|hello|1|test-session|report,call,read");
    assert_eq!(lines[2], "OBS|part|test-session|kind|emulator");
    assert_eq!(lines[3], "OBS|done|1|ok||");
}

/// Everything but `hello` needs a session first.
#[test]
fn a_command_before_negotiation_is_refused_rather_than_served() {
    let lines = serve("CMD|1|report\n", Fake::default());
    assert_eq!(lines, vec!["OBS|refused|1|not-negotiated"]);
}

/// The secret is checked before the capability reply, so an unauthenticated peer learns
/// nothing about what this build can do.
#[test]
fn a_wrong_secret_is_refused_without_disclosing_the_capabilities() {
    let fake = Fake {
        secret: Some("correct-horse".to_owned()),
        ..Fake::default()
    };
    let lines = serve("CMD|1|hello|1|wrong\n", fake);
    assert_eq!(lines, vec!["OBS|refused|1|unauthorised"]);
    assert!(
        !lines.iter().any(|line| line.contains("report")),
        "the capability list leaked: {lines:?}"
    );
}

/// And a failed `hello` does not leave a session behind.
#[test]
fn a_refused_hello_does_not_negotiate_the_rest_of_the_surface() {
    let fake = Fake {
        secret: Some("correct-horse".to_owned()),
        ..Fake::default()
    };
    let lines = serve("CMD|1|hello|1|wrong\nCMD|2|report\n", fake);
    assert_eq!(
        lines,
        vec!["OBS|refused|1|unauthorised", "OBS|refused|2|not-negotiated"]
    );
}

#[test]
fn the_right_secret_negotiates() {
    let fake = Fake {
        secret: Some("correct-horse".to_owned()),
        ..Fake::default()
    };
    let lines = serve("CMD|1|hello|1|correct-horse\n", fake);
    assert_eq!(lines[0], "OBS|ack|1|hello");
    assert!(lines[1].starts_with("OBS|hello|1|test-session|"));
}

/// The acknowledgement precedes the work it acknowledges.
///
/// Not a stylistic ordering. A command that ends the responder must already have been
/// acknowledged, because an `ack` with no `done` after it names the command that did the
/// killing and a silent connection names nothing.
#[test]
fn a_command_is_acknowledged_before_it_is_carried_out() {
    let lines = serve("CMD|1|hello|1\nCMD|2|call|0x1000|0x1\n", Fake::default());
    let ack = lines
        .iter()
        .position(|line| line == "OBS|ack|2|call")
        .expect("acknowledged");
    let done = lines
        .iter()
        .position(|line| line.starts_with("OBS|done|2|"))
        .expect("finished");
    assert!(ack < done, "the ack came after the result: {lines:?}");
}

#[test]
fn call_passes_the_address_and_every_argument_through() {
    let mut responder = Responder::new(
        Wire::new("CMD|1|hello|1\nCMD|2|call|0x1000|0x1|0x2|0x3\n"),
        Fake::default(),
    );
    responder.serve().expect("served");
    assert_eq!(
        responder.answers().calls,
        vec![(0x1000, vec![1, 2, 3])],
        "the arguments did not arrive intact"
    );
}

#[test]
fn a_call_to_a_malformed_address_is_refused_and_never_reaches_the_backend() {
    let mut responder = Responder::new(
        Wire::new("CMD|1|hello|1\nCMD|2|call|not-a-number\n"),
        Fake::default(),
    );
    responder.serve().expect("served");
    assert!(
        responder.answers().calls.is_empty(),
        "a malformed command was executed"
    );
}

#[test]
fn read_returns_the_bytes_as_one_run() {
    let fake = Fake {
        memory: vec![0xde, 0xad, 0xbe, 0xef],
        ..Fake::default()
    };
    let lines = serve("CMD|1|hello|1\nCMD|2|read|0x0|0x4\n", fake);
    assert!(
        lines
            .iter()
            .any(|l| l == "OBS|bytes|read/0x0|responder|memory|0|deadbeef"),
        "no bytes record: {lines:?}"
    );
}

#[test]
fn a_read_outside_memory_is_refused_as_unmapped() {
    let lines = serve("CMD|1|hello|1\nCMD|2|read|0x9999|0x4\n", Fake::default());
    assert!(
        lines.iter().any(|l| l == "OBS|refused|2|unmapped"),
        "expected an unmapped refusal: {lines:?}"
    );
}

#[test]
fn an_unknown_verb_is_refused_by_name() {
    let lines = serve("CMD|1|hello|1\nCMD|2|teleport\n", Fake::default());
    assert!(
        lines.iter().any(|l| l == "OBS|refused|2|unknown-verb"),
        "expected an unknown-verb refusal: {lines:?}"
    );
}

/// A sequence number that does not advance is refused.
///
/// It is how a replayed or duplicated command shows up, and answering it twice would put
/// two results against one number in the transcript.
#[test]
fn a_sequence_that_does_not_advance_is_refused() {
    let lines = serve("CMD|1|hello|1\nCMD|1|report\n", Fake::default());
    assert!(
        lines.iter().any(|l| l == "OBS|refused|1|bad-argument"),
        "a repeated sequence was served: {lines:?}"
    );
}

#[test]
fn bye_ends_the_session_and_says_so() {
    let lines = serve("CMD|1|hello|1\nCMD|2|bye\nCMD|3|report\n", Fake::default());
    assert_eq!(lines.last().unwrap(), "OBS|done|2|ok||session closed");
    assert!(
        !lines.iter().any(|l| l.contains("|3|")),
        "commands were served after bye: {lines:?}"
    );
}

/// A backend that cannot do something refuses it rather than inventing an answer.
#[test]
fn a_verb_the_backend_cannot_serve_is_refused_not_faked() {
    struct Empty;
    impl Answers for Empty {
        fn capabilities(&self) -> Vec<Capability> {
            Vec::new()
        }
        fn session(&self) -> String {
            "empty".to_owned()
        }
    }
    let mut responder = Responder::new(Wire::new("CMD|1|hello|1\nCMD|2|report\n"), Empty);
    responder.serve().expect("served");
    let out = String::from_utf8(responder.into_stream().outgoing).expect("text");
    assert!(
        out.contains("OBS|refused|2|unsupported"),
        "expected an unsupported refusal: {out}"
    );
    assert!(
        out.contains("OBS|hello|1|empty|\n"),
        "an empty capability list should be empty: {out}"
    );
}
