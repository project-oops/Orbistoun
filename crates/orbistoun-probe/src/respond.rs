//! Answering the same commands a probe answers, so one driver can drive either.
//!
//! # Which way this seam points
//!
//! orbistoun is a **responder, never a driver**. obSCEne owns the protocol and the record
//! format; this implements against whatever they define. That direction is not a courtesy -
//! the emulator has no business knowing what is on the other end of a comparison, and a
//! test harness that also owned the protocol could quietly define away a disagreement
//! (`docs/BACKLOG.md`, D043).
//!
//! So there is no vocabulary invented here. Every token this module writes is one
//! [`super::parse`] already reads, and the round-trip is held by a test.
//!
//! # What it is worth before `call` exists
//!
//! Very little of the emulator can be invoked live yet, and this declares only what it can
//! actually serve - a capability announced and then refused is principle 3's failure mode
//! with a handshake in front of it. What it *can* do is answer `report`, and a report
//! carries symbol presence: **whether a name exists, in which library, and how it is
//! reached**. Pointing one driver at a probe and at this, and diffing the `sym` records,
//! asks "does the emulator know the same names the platform has" - live, from one place,
//! without a capture step in between.
//!
//! # The ack is written before the work, and flushed
//!
//! The protocol's central discipline, and it survives being on this side of it. A command
//! that kills the responder must still have been acknowledged, because an acknowledgement
//! with no `done` after it names the command that did the killing, and silence names
//! nothing. Buffering the ack until after the work would make every death look like a
//! connection that dropped for its own reasons.

use std::io::{BufRead, BufReader, Read, Write};

use crate::{Capability, Line, Outcome, Provenance, Record, Refusal, Status, VERSION};

// ---------------------------------------------------------------------------------------
// Tokens
// ---------------------------------------------------------------------------------------

impl Capability {
    /// The token this is written as.
    ///
    /// The inverse of [`Self::parse`], including for [`Self::Other`] - a capability this
    /// version does not understand is carried through unchanged rather than dropped,
    /// because a responder relaying one it was told about is not the place to decide it
    /// was meaningless.
    pub fn token(&self) -> &str {
        match self {
            Self::Call => "call",
            Self::Resolve => "resolve",
            Self::Read => "read",
            Self::Write => "write",
            Self::Blob => "blob",
            Self::Reset => "reset",
            Self::Report => "report",
            Self::Gpu => "gpu",
            Self::Other(token) => token,
        }
    }
}

impl Refusal {
    /// The token this is written as.
    pub fn token(&self) -> &str {
        match self {
            Self::UnknownVerb => "unknown-verb",
            Self::Unsupported => "unsupported",
            Self::BadArgument => "bad-argument",
            Self::Busy => "busy",
            Self::NotNegotiated => "not-negotiated",
            Self::Unmapped => "unmapped",
            Self::Unauthorised => "unauthorised",
            Self::Other(token) => token,
        }
    }
}

impl Status {
    /// The token this is written as.
    pub fn token(&self) -> &str {
        match self {
            Self::Pass => "pass",
            Self::Partial => "partial",
            Self::Fail => "fail",
            Self::Skip => "skip",
            Self::Other(token) => token,
        }
    }
}

impl Provenance {
    /// The token this is written as.
    pub fn token(&self) -> &str {
        match self {
            Self::Assumed => "assumed",
            Self::Derived => "derived",
            Self::Spec => "spec",
            Self::Documented => "documented",
            Self::Hardware => "hardware",
            Self::Unrecognised(token) => token,
        }
    }
}

impl Outcome {
    /// The outcome word and the value beside it.
    ///
    /// Two fields rather than one, because that is how the wire carries them - and because
    /// the empty value on a non-answer is load-bearing. A reader refuses a record whose
    /// outcome did not answer and yet carries a value, so a writer that put something
    /// there would be producing lines its own parser rejects.
    pub fn fields(&self) -> (&str, String) {
        match self {
            Self::Ok => ("ok", String::new()),
            Self::Absent => ("absent", String::new()),
            Self::Died => ("died", String::new()),
            Self::Timeout => ("timeout", String::new()),
            Self::Lost => ("lost", String::new()),
            Self::Returned(value) => ("returned", format!("{value:#x}")),
            Self::Unrecognised(word) => (word, String::new()),
        }
    }
}

// ---------------------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------------------

/// Renders one record as a wire line, without its terminator.
///
/// Every field is written verbatim. There is deliberately no escaping: the protocol has
/// none, so inventing some here would produce lines only this implementation could read,
/// which is the one thing a responder must not do.
#[allow(
    clippy::too_many_lines,
    reason = "one arm per record kind; splitting it scatters the wire order"
)]
pub fn render(record: &Record) -> String {
    let join = |kind: &str, fields: &[String]| {
        let mut line = format!("{}{}{kind}", crate::RECORD, crate::SEPARATOR);
        for field in fields {
            line.push(crate::SEPARATOR);
            line.push_str(field);
        }
        line
    };
    match record {
        Record::Ack { seq, verb } => join("ack", &[seq.to_string(), verb.clone()]),
        Record::Hello {
            version,
            session,
            capabilities,
        } => join(
            "hello",
            &[
                version.to_string(),
                session.clone(),
                capabilities
                    .iter()
                    .map(Capability::token)
                    .collect::<Vec<_>>()
                    .join(","),
            ],
        ),
        Record::Part {
            session,
            key,
            value,
        } => join("part", &[session.clone(), key.clone(), value.clone()]),
        Record::Done {
            seq,
            outcome,
            detail,
        } => {
            let (word, value) = outcome.fields();
            join(
                "done",
                &[seq.to_string(), word.to_owned(), value, detail.clone()],
            )
        }
        Record::Refused { seq, reason } => {
            join("refused", &[seq.to_string(), reason.token().to_owned()])
        }
        Record::Build { build, kind } => join("build", &[build.clone(), kind.clone()]),
        Record::Sym {
            library,
            symbol,
            presence,
            availability,
        } => join(
            "sym",
            &[
                library.clone(),
                symbol.clone(),
                presence.clone(),
                availability.clone(),
            ],
        ),
        Record::Resolve {
            library,
            symbol,
            presence,
            address,
        } => join(
            "resolve",
            &[
                library.clone(),
                symbol.clone(),
                presence.clone(),
                address.clone(),
            ],
        ),
        Record::Section { id, title, purpose } => {
            join("section", &[id.clone(), title.clone(), purpose.clone()])
        }
        Record::SysInfo {
            field,
            state,
            value,
        } => join("sysinfo", &[field.clone(), state.clone(), value.clone()]),
        Record::Sink { path } => join("sink", std::slice::from_ref(path)),
        Record::Bytes {
            id,
            source,
            kind,
            offset,
            hex,
        } => join(
            "bytes",
            &[
                id.clone(),
                source.clone(),
                kind.clone(),
                offset.to_string(),
                hex.clone(),
            ],
        ),
        Record::SectionTally {
            id,
            pass,
            partial,
            fail,
            skip,
        } => join(
            "sectiontally",
            &[
                id.clone(),
                pass.to_string(),
                partial.to_string(),
                fail.to_string(),
                skip.to_string(),
            ],
        ),
        Record::Try {
            check,
            library,
            symbol,
        } => join("try", &[check.clone(), library.clone(), symbol.clone()]),
        Record::Res {
            check,
            status,
            value,
            detail,
            provenance,
        } => join(
            "res",
            &[
                check.clone(),
                status.token().to_owned(),
                value.clone(),
                detail.clone(),
                // Absent stays absent. Writing a default grade onto a record that claimed
                // none would manufacture provenance, which is the failure this whole crate
                // is arranged to prevent.
                provenance
                    .as_ref()
                    .map_or_else(String::new, |p| p.token().to_owned()),
            ],
        ),
        Record::Other { kind, fields } => join(kind, fields),
    }
}

// ---------------------------------------------------------------------------------------
// Answering
// ---------------------------------------------------------------------------------------

/// What a responder needs from whatever is behind it.
///
/// # Why every verb returns records rather than values
///
/// Because the framing rules are the part that must not be reimplemented per backend.
/// Sequence echo, ack-before-execute, which refusal a missing capability earns, when a
/// session counts as negotiated - all of that lives in [`Responder`] and none of it is a
/// backend's business. A backend answers *what happened*; the responder decides how that
/// is said.
///
/// A verb the backend cannot serve returns [`Refusal::Unsupported`]. It should also not
/// have announced the capability - see [`Self::capabilities`].
pub trait Answers {
    /// What this responder can actually do.
    ///
    /// **Announce only what is served.** The reply to `hello` is what a driver plans
    /// against, so a capability listed here and refused later is worse than one never
    /// offered: the driver has already decided the comparison is possible.
    fn capabilities(&self) -> Vec<Capability>;

    /// Identifier for this process. A new one means the responder restarted.
    fn session(&self) -> String;

    /// The secret a caller must present, when one is required.
    ///
    /// [`None`] accepts any `hello`. That is the right default for a responder bound to
    /// the loopback interface by a person who started it deliberately, and the wrong one
    /// for anything reachable from a network - see [`Responder::serve`].
    fn secret(&self) -> Option<String> {
        None
    }

    /// Key/value metadata written as `part` records after a successful `hello`.
    ///
    /// **This is where a responder says what it is**, and the honest answer for this one
    /// is that it is an emulator. A driver that treats an emulator's answer as the
    /// platform's has been misled by the thing it was comparing against, which is the one
    /// failure mode a comparison tool cannot survive.
    fn describe(&self) -> Vec<(String, String)> {
        Vec::new()
    }

    /// Everything the `report` verb produces.
    ///
    /// # Errors
    ///
    /// [`Refusal::Unsupported`] when this backend has no report to give.
    fn report(&mut self) -> Result<Vec<Record>, Refusal> {
        Err(Refusal::Unsupported)
    }

    /// Invokes a function, returning what it did and free text describing it.
    ///
    /// # Errors
    ///
    /// [`Refusal::Unsupported`] when nothing can be invoked, [`Refusal::Unmapped`] when
    /// the address is known not to be mapped.
    fn call(&mut self, address: u64, arguments: &[u64]) -> Result<(Outcome, String), Refusal> {
        let _ = (address, arguments);
        Err(Refusal::Unsupported)
    }

    /// Reads guest memory.
    ///
    /// # Errors
    ///
    /// [`Refusal::Unsupported`] when there is no guest to read, [`Refusal::Unmapped`] when
    /// the range is not mapped.
    fn read(&mut self, address: u64, length: u64) -> Result<Vec<u8>, Refusal> {
        let _ = (address, length);
        Err(Refusal::Unsupported)
    }
}

/// Serves the command protocol over one stream.
///
/// Generic over the transport for the same reason the client is: the whole exchange is
/// testable from a pair of in-memory buffers, so the tests never open a socket and the
/// gate never needs one (D016).
#[derive(Debug)]
pub struct Responder<S, A> {
    stream: BufReader<S>,
    answers: A,
    negotiated: bool,
    highest_seq: Option<u64>,
}

impl<S: Read + Write, A: Answers> Responder<S, A> {
    /// Wraps a stream and a backend.
    pub fn new(stream: S, answers: A) -> Self {
        Self {
            stream: BufReader::new(stream),
            answers,
            negotiated: false,
            highest_seq: None,
        }
    }

    /// The stream back, so a test can read what was written.
    pub fn into_stream(self) -> S {
        self.stream.into_inner()
    }

    /// The backend, so a test can see what it was asked to do.
    ///
    /// Checking the records is not enough on its own: a responder that refused a malformed
    /// command *and* executed it would produce exactly the right transcript. What has to be
    /// asserted is that the backend was never reached.
    pub const fn answers(&self) -> &A {
        &self.answers
    }

    /// Reads commands until the peer says `bye` or the stream ends.
    ///
    /// # Opening a socket is the caller's decision, not this crate's
    ///
    /// Nothing here binds anything. A responder is a socket the emulator did not have to
    /// open, so it is opt-in at run time and stays out of every automated path - a gate
    /// that listens on a port is a gate that behaves differently depending on what else is
    /// running on the machine.
    ///
    /// # Errors
    ///
    /// When the stream fails. A malformed command is **not** an error: it is refused, and
    /// refusing is the behaviour under test.
    pub fn serve(&mut self) -> std::io::Result<()> {
        let mut line = String::new();
        loop {
            line.clear();
            if self.stream.read_line(&mut line)? == 0 {
                return Ok(());
            }
            match crate::parse_line(&line) {
                Ok(Line::Request {
                    seq,
                    verb,
                    arguments,
                }) => {
                    if self.dispatch(seq, &verb, &arguments)? {
                        return Ok(());
                    }
                }
                // A record arriving on the command channel is the peer talking as though
                // it were the probe, and a line that parses as nothing at all is noise.
                // Both are dropped for the same reason as a note: there is no sequence
                // number to refuse them against, and refusing against zero would put a
                // fiction in the transcript.
                Ok(Line::Record(_) | Line::Note(_)) | Err(_) => {}
            }
        }
    }

    /// Handles one command. Returns whether the session should end.
    fn dispatch(&mut self, seq: Option<u64>, verb: &str, args: &[String]) -> std::io::Result<bool> {
        // A sequence that is not a number, or that did not advance, is refused against the
        // number the peer sent as far as it can be read. Refusing against zero would make
        // two different mistakes indistinguishable in a transcript.
        let Some(seq) = seq else {
            return self.refuse(0, &Refusal::BadArgument).map(|()| false);
        };
        if self.highest_seq.is_some_and(|highest| seq <= highest) {
            return self.refuse(seq, &Refusal::BadArgument).map(|()| false);
        }
        self.highest_seq = Some(seq);

        if verb == "hello" {
            return self.hello(seq, args).map(|()| false);
        }
        // Everything else needs a session. One check, whole surface - the same shape
        // obSCEne uses, and the reason the secret is verified before the capability reply
        // rather than beside it.
        if !self.negotiated {
            return self.refuse(seq, &Refusal::NotNegotiated).map(|()| false);
        }
        match verb {
            "bye" => {
                self.ack(seq, verb)?;
                self.emit(&Record::Done {
                    seq,
                    outcome: Outcome::Ok,
                    detail: "session closed".to_owned(),
                })?;
                Ok(true)
            }
            "report" => self.report(seq).map(|()| false),
            "call" => self.call(seq, args).map(|()| false),
            "read" => self.read(seq, args).map(|()| false),
            _ => self.refuse(seq, &Refusal::UnknownVerb).map(|()| false),
        }
    }

    /// Negotiates, checking the secret before anything is disclosed.
    fn hello(&mut self, seq: u64, args: &[String]) -> std::io::Result<()> {
        if let Some(expected) = self.answers.secret() {
            let presented = args.get(1).map(String::as_str).unwrap_or_default();
            if !same_secret(&expected, presented) {
                // Before the capability reply, not beside it: the reply names everything
                // this build can do, and an unauthenticated peer should not learn it.
                // Failing here never sets `negotiated`, so the rule above refuses the rest
                // of the surface for free.
                return self.refuse(seq, &Refusal::Unauthorised);
            }
        }
        self.ack(seq, "hello")?;
        let session = self.answers.session();
        self.emit(&Record::Hello {
            version: VERSION,
            session: session.clone(),
            capabilities: self.answers.capabilities(),
        })?;
        for (key, value) in self.answers.describe() {
            self.emit(&Record::Part {
                session: session.clone(),
                key,
                value,
            })?;
        }
        self.negotiated = true;
        self.emit(&Record::Done {
            seq,
            outcome: Outcome::Ok,
            detail: String::new(),
        })
    }

    /// Runs the backend's report.
    fn report(&mut self, seq: u64) -> std::io::Result<()> {
        self.ack(seq, "report")?;
        match self.answers.report() {
            Ok(records) => {
                for record in &records {
                    self.emit(record)?;
                }
                self.emit(&Record::Done {
                    seq,
                    outcome: Outcome::Ok,
                    detail: format!("{} records", records.len()),
                })
            }
            Err(reason) => self.emit(&Record::Refused { seq, reason }),
        }
    }

    /// Invokes a function.
    fn call(&mut self, seq: u64, args: &[String]) -> std::io::Result<()> {
        let Some(address) = args.first().and_then(|a| crate::hex(a)) else {
            return self.refuse(seq, &Refusal::BadArgument);
        };
        let mut arguments = Vec::new();
        for raw in args.iter().skip(1) {
            let Some(value) = crate::hex(raw) else {
                return self.refuse(seq, &Refusal::BadArgument);
            };
            arguments.push(value);
        }
        // Before the call, and flushed by `ack`. If invoking the address ends this process
        // the peer still has a record naming what was being invoked.
        self.ack(seq, "call")?;
        match self.answers.call(address, &arguments) {
            Ok((outcome, detail)) => self.emit(&Record::Done {
                seq,
                outcome,
                detail,
            }),
            Err(reason) => self.emit(&Record::Refused { seq, reason }),
        }
    }

    /// Reads guest memory.
    fn read(&mut self, seq: u64, args: &[String]) -> std::io::Result<()> {
        let address = args.first().and_then(|a| crate::hex(a));
        let length = args
            .get(1)
            .and_then(|l| crate::hex(l).or_else(|| l.parse().ok()));
        let (Some(address), Some(length)) = (address, length) else {
            return self.refuse(seq, &Refusal::BadArgument);
        };
        self.ack(seq, "read")?;
        match self.answers.read(address, length) {
            Ok(bytes) => {
                self.emit(&Record::Bytes {
                    id: format!("read/{address:#x}"),
                    source: "responder".to_owned(),
                    kind: "memory".to_owned(),
                    offset: 0,
                    hex: bytes.iter().fold(String::new(), |mut hex, byte| {
                        use std::fmt::Write as _;
                        let _ = write!(hex, "{byte:02x}");
                        hex
                    }),
                })?;
                self.emit(&Record::Done {
                    seq,
                    outcome: Outcome::Ok,
                    detail: format!("{} bytes", bytes.len()),
                })
            }
            Err(reason) => self.emit(&Record::Refused { seq, reason }),
        }
    }

    /// Writes an acknowledgement and **flushes it**.
    ///
    /// The flush is the whole point. An ack sitting in a buffer while the command runs is
    /// an ack that never arrives if the command is fatal, and the peer then sees a
    /// connection that dropped rather than a command that killed something.
    fn ack(&mut self, seq: u64, verb: &str) -> std::io::Result<()> {
        self.emit(&Record::Ack {
            seq,
            verb: verb.to_owned(),
        })
    }

    /// Refuses a command.
    fn refuse(&mut self, seq: u64, reason: &Refusal) -> std::io::Result<()> {
        self.emit(&Record::Refused {
            seq,
            reason: reason.clone(),
        })
    }

    /// Writes one record and flushes.
    fn emit(&mut self, record: &Record) -> std::io::Result<()> {
        let line = render(record);
        let stream = self.stream.get_mut();
        stream.write_all(line.as_bytes())?;
        stream.write_all(b"\n")?;
        stream.flush()
    }
}

/// Compares two secrets without returning early on the first difference.
///
/// The obvious comparison hands a timing adversary the secret one byte at a time. This is
/// not cryptography and the socket is cleartext anyway - it is the cheap half of a
/// mitigation whose expensive half nobody has asked for, and leaving it out would be a
/// choice rather than an oversight.
fn same_secret(expected: &str, presented: &str) -> bool {
    if expected.len() != presented.len() {
        return false;
    }
    expected
        .bytes()
        .zip(presented.bytes())
        .fold(0u8, |difference, (a, b)| difference | (a ^ b))
        == 0
}
