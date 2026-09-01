//! Driving a live session against a probe.
//!
//! # Why this is generic over the stream
//!
//! [`Client`] talks to anything that reads and writes bytes. A `TcpStream` reaching a
//! console is one; a pair of in-memory buffers replaying a captured transcript is another,
//! and the client cannot tell them apart.
//!
//! That is not a testing convenience bolted on afterwards - it is the whole design. **CI
//! must never open a socket**, and a client that can only be exercised against real
//! hardware is a client whose error paths are never exercised at all. The paths that matter
//! most here are the ones where the far end stops answering, and those are unreachable from
//! a happy-path integration test even when hardware is available.
//!
//! # What this does not do
//!
//! Decide what anything means. It obtains records and hands them over; grading, pairing and
//! knowledge are the caller's, and they take an [`Origin`](crate::Origin) the operator
//! asserted rather than anything read off this wire.

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::time::{Duration, Instant};

use crate::{Capability, Line, Outcome, Record, Refusal};

/// The port a probe listens on unless told otherwise.
pub const DEFAULT_PORT: u16 = 9803;

/// The longest line the protocol permits, including its terminator.
const LINE_LIMIT: usize = 4096;

/// What went wrong talking to a probe.
#[derive(Debug)]
pub enum ClientError {
    /// The stream failed.
    Io(std::io::Error),
    /// A line arrived that the grammar does not cover.
    Malformed {
        /// The line, verbatim.
        line: String,
        /// What was wrong with it.
        detail: String,
    },
    /// The probe refused the command.
    Refused(Refusal),
    /// A verb was sent whose capability the probe never announced.
    ///
    /// Caught here rather than at the far end deliberately: the protocol says a driver must
    /// not send a command whose capability was not announced, and a client that relies on
    /// being refused has already sent it.
    NotNegotiated(String),
    /// Negotiation has not happened yet.
    NotNegotiatedYet,
    /// A line exceeded the protocol's length limit.
    TooLong(usize),
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::Malformed { line, detail } => write!(f, "{detail}: {line:?}"),
            Self::Refused(reason) => write!(f, "refused: {reason:?}"),
            Self::NotNegotiated(verb) => write!(
                f,
                "`{verb}` was not announced by this probe, so it was not sent"
            ),
            Self::NotNegotiatedYet => write!(f, "no session: `hello` has not been sent"),
            Self::TooLong(length) => {
                write!(
                    f,
                    "a line of {length} bytes exceeds the {LINE_LIMIT}-byte limit"
                )
            }
        }
    }
}

impl std::error::Error for ClientError {}

impl From<std::io::Error> for ClientError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// What one command produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    /// Sequence the command was sent under.
    pub seq: u64,
    /// How it ended.
    pub outcome: Outcome,
    /// Free text the probe supplied, or the client's reason for a non-answer.
    pub detail: String,
    /// Records that arrived between the acknowledgement and the answer.
    pub records: Vec<Record>,
}

/// A live session with a probe.
///
/// One connection, one command in flight. That is a protocol requirement rather than a
/// simplification: with two commands outstanding and a process that has just vanished,
/// nothing says which one killed it - and that attribution is the finding.
#[derive(Debug)]
pub struct Client<S: Read + Write> {
    stream: BufReader<S>,
    /// Sequence numbers are the client's to own, strictly increasing from one.
    next_seq: u64,
    /// Identifier the probe gave itself, once negotiated.
    session: Option<String>,
    /// What it said it could do.
    capabilities: BTreeSet<Capability>,
    /// How long to wait for an answer before calling it a timeout.
    budget: Duration,
    /// Every line seen, so a session can be written out and replayed.
    transcript: Vec<String>,
}

impl<S: Read + Write> Client<S> {
    /// Wraps a stream. Nothing is sent until [`Client::hello`].
    pub fn new(stream: S, budget: Duration) -> Self {
        Self {
            stream: BufReader::new(stream),
            next_seq: 1,
            session: None,
            capabilities: BTreeSet::new(),
            budget,
            transcript: Vec::new(),
        }
    }

    /// The session identifier, once negotiated.
    pub fn session(&self) -> Option<&str> {
        self.session.as_deref()
    }

    /// Whether the probe announced a capability.
    pub fn can(&self, capability: &Capability) -> bool {
        self.capabilities.contains(capability)
    }

    /// Everything seen on the wire, in order.
    ///
    /// The session is transient and the corpus is the product, so a client that could not
    /// hand back what it saw would be throwing away the only durable thing it produced.
    pub fn transcript(&self) -> &[String] {
        &self.transcript
    }

    /// Negotiates, and returns the session identifier.
    ///
    /// A **different** identifier from a previous negotiation means the probe restarted:
    /// a faulting command ends it, and everything since the last negotiation belongs to a
    /// different process. Nothing is resumed and nothing pretends otherwise.
    /// # The session secret
    ///
    /// A probe generates one per startup and shows it - on the console's own display, since
    /// there is no other channel - and it is passed as a fourth field appended after the
    /// version. Appended rather than inserted, which is what keeps an older driver working.
    ///
    /// `None` sends the three-field form. A probe that generated no secret accepts it; one
    /// that did answers [`Refusal::Unauthorised`], which is a clean refusal rather than a
    /// parse error, so the wrong-key case is legible instead of looking like a broken wire.
    pub fn hello(&mut self, version: u32, secret: Option<&str>) -> Result<String, ClientError> {
        let version = version.to_string();
        let mut arguments = vec![version.as_str()];
        if let Some(secret) = secret {
            arguments.push(secret);
        }
        let answer = self.command_unchecked("hello", &arguments)?;
        for record in &answer.records {
            if let Record::Hello {
                session,
                capabilities,
                ..
            } = record
            {
                self.session = Some(session.clone());
                self.capabilities = capabilities.iter().cloned().collect();
            }
        }
        self.session.clone().ok_or(ClientError::NotNegotiatedYet)
    }

    /// Runs the probe's compiled-in suite, streaming its records back.
    pub fn report(&mut self) -> Result<Answer, ClientError> {
        self.command("report", &[])
    }

    /// Invokes an address with up to six integer arguments.
    ///
    /// # What a fatal address does, and what it does not
    ///
    /// A malformed address or argument is **refused**. A well-formed one that happens to be
    /// fatal - zero, unmapped, whatever the guest left there - is **not**: it is called, and
    /// the probe dies executing it. That arrives as an acknowledgement with no result,
    /// which is [`Outcome::Died`].
    ///
    /// Null is called rather than rejected, deliberately. A probe that refused zero would be
    /// answering a question about its own argument checking instead of the question that
    /// was asked, and "what does this platform do when you call null" is a real question
    /// with a real answer.
    pub fn call(&mut self, address: u64, arguments: &[u64]) -> Result<Answer, ClientError> {
        let mut rendered = vec![format!("{address:#x}")];
        rendered.extend(arguments.iter().map(|argument| format!("{argument:#x}")));
        let borrowed: Vec<&str> = rendered.iter().map(String::as_str).collect();
        self.command("call", &borrowed)
    }

    /// Reads a run of guest memory.
    ///
    /// # Two legitimate answers for an address that cannot be read
    ///
    /// A platform that can test an address before touching it answers `refused|unmapped`.
    /// One that cannot **faults**, which arrives as [`Outcome::Died`]. Both are permitted
    /// and they are different facts: "this address is not readable" and "asking about this
    /// address killed the process" are not the same thing, and a consumer that collapsed
    /// them would lose the distinction the record went to the trouble of keeping.
    ///
    /// The serving build at the time of writing does not pre-validate, so a bad address
    /// dies. A caller must handle both regardless - which build is on the other end is not
    /// something this can know.
    pub fn read(&mut self, address: u64, length: u64) -> Result<Answer, ClientError> {
        self.command("read", &[&format!("{address:#x}"), &format!("{length:#x}")])
    }

    /// Closes the session cleanly.
    ///
    /// A session that ends without this is recorded as having ended without it, which is a
    /// fact about the run rather than an error.
    pub fn bye(&mut self) -> Result<Answer, ClientError> {
        self.command("bye", &[])
    }

    /// Sends a command, refusing to send one the probe never announced.
    ///
    /// The check is here rather than at the far end because the protocol asks for it here.
    /// A client that sends anyway and waits to be refused has already put a verb on the
    /// wire that this probe does not implement, and on a target that faults easily that is
    /// not a free thing to do.
    pub fn command(&mut self, verb: &str, arguments: &[&str]) -> Result<Answer, ClientError> {
        if self.session.is_none() {
            return Err(ClientError::NotNegotiatedYet);
        }
        if let Some(needed) = capability_for(verb) {
            if !self.capabilities.contains(&needed) {
                return Err(ClientError::NotNegotiated(verb.to_owned()));
            }
        }
        self.command_unchecked(verb, arguments)
    }

    fn command_unchecked(&mut self, verb: &str, arguments: &[&str]) -> Result<Answer, ClientError> {
        let seq = self.next_seq;
        self.next_seq += 1;

        let mut line = format!("CMD|{seq}|{verb}");
        for argument in arguments {
            line.push('|');
            line.push_str(argument);
        }
        line.push('\n');
        if line.len() > LINE_LIMIT {
            return Err(ClientError::TooLong(line.len()));
        }
        self.transcript.push(line.trim_end().to_owned());
        self.stream.get_mut().write_all(line.as_bytes())?;
        self.stream.get_mut().flush()?;

        self.collect(seq)
    }

    /// Reads until this command is answered, or until it is established that it was not.
    ///
    /// # The three non-answers
    ///
    /// An acknowledgement is written and flushed **before** the command runs, so its
    /// absence and its presence mean different things:
    ///
    /// - acknowledged, then the stream closed with no result: the command **died**. It
    ///   ended the process, and the process cannot report that itself.
    /// - acknowledged, then nothing within the budget: **timeout**. The probe may be alive,
    ///   blocked, or looping, and the honest record says which was observed rather than
    ///   which was guessed.
    /// - the stream closed without even an acknowledgement: **lost**, and recorded as the
    ///   ambiguity it is.
    ///
    /// None of them carries a value. There is no field on those variants for one to hide
    /// in, which is the point.
    fn collect(&mut self, seq: u64) -> Result<Answer, ClientError> {
        let deadline = Instant::now() + self.budget;
        let mut acknowledged = false;
        let mut records = Vec::new();

        loop {
            if Instant::now() >= deadline {
                return Ok(Answer {
                    seq,
                    outcome: if acknowledged {
                        Outcome::Timeout
                    } else {
                        Outcome::Lost
                    },
                    detail: format!("no answer within {:?}", self.budget),
                    records,
                });
            }

            let mut line = String::new();
            let read = self.stream.read_line(&mut line)?;
            if read == 0 {
                // The far end closed. What that means depends entirely on whether the
                // command was acknowledged first.
                return Ok(Answer {
                    seq,
                    outcome: if acknowledged {
                        Outcome::Died
                    } else {
                        Outcome::Lost
                    },
                    detail: if acknowledged {
                        "connection closed after ack with no result".to_owned()
                    } else {
                        "connection closed before the command was acknowledged".to_owned()
                    },
                    records,
                });
            }
            if line.len() > LINE_LIMIT {
                return Err(ClientError::TooLong(line.len()));
            }
            self.transcript.push(line.trim_end().to_owned());

            let parsed = crate::parse_line(&line).map_err(|detail| ClientError::Malformed {
                line: line.trim_end().to_owned(),
                detail,
            })?;
            match parsed {
                Line::Note(_) | Line::Request { .. } => {}
                Line::Record(record) => match &record {
                    Record::Ack { .. } => acknowledged = true,
                    Record::Done {
                        outcome, detail, ..
                    } => {
                        return Ok(Answer {
                            seq,
                            outcome: outcome.clone(),
                            detail: detail.clone(),
                            records,
                        });
                    }
                    Record::Refused { reason, .. } => {
                        return Err(ClientError::Refused(reason.clone()));
                    }
                    _ => records.push(record),
                },
            }
        }
    }
}

/// Which capability a verb needs, where it needs one.
///
/// `hello` and `bye` need none - they are how a session begins and ends, and requiring a
/// capability to negotiate would be circular.
fn capability_for(verb: &str) -> Option<Capability> {
    match verb {
        "call" => Some(Capability::Call),
        "resolve" => Some(Capability::Resolve),
        "read" => Some(Capability::Read),
        "write" => Some(Capability::Write),
        "blob" | "run" => Some(Capability::Blob),
        "reset" => Some(Capability::Reset),
        "report" => Some(Capability::Report),
        _ => None,
    }
}

/// Opens a session against a listening probe.
///
/// # Why this is the only place a socket appears
///
/// The probe listens and the driver connects - a console has no DNS, no configuration file
/// and no way to be told where to find a host, but it has an address a person can read off
/// a screen. So the operator supplies `host:port` and this dials it.
///
/// Everything above the connection is [`Client`], which does not know a socket exists. That
/// keeps the whole protocol testable from memory and keeps this function small enough to
/// have nothing worth testing in it - which is the right size for the one part that cannot
/// be exercised in CI.
///
/// # Timeouts are set on the socket as well as the budget
///
/// The read budget alone cannot rescue a blocked read: without a socket timeout the call
/// sits in the kernel and the deadline is never consulted. Both are needed, and the socket
/// one is set slightly under so the budget is what actually decides.
pub fn connect(
    address: &str,
    budget: Duration,
) -> Result<Client<std::net::TcpStream>, ClientError> {
    let stream = std::net::TcpStream::connect(address)?;
    stream.set_read_timeout(Some(budget))?;
    stream.set_write_timeout(Some(budget))?;
    Ok(Client::new(stream, budget))
}
