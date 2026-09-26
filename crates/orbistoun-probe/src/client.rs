//! Driving a live session against a probe.
//!
//! [`Client`] is generic over any byte stream: a `TcpStream` to the target, or in-memory
//! buffers replaying a captured transcript. CI never opens a socket, so every error path,
//! above all a far end that stops answering, is tested from memory. The client only obtains
//! records; grading and pairing are the caller's, against an [`Origin`](crate::Origin) the
//! operator asserted.

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
        detail: crate::LineError,
    },
    /// The probe refused the command.
    Refused(Refusal),
    /// A verb was sent whose capability the probe never announced.
    ///
    /// Caught here rather than at the far end: the protocol forbids a driver sending a
    /// command whose capability was not announced.
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
/// One connection, one command in flight: a protocol requirement, so a command that ends
/// the probe is unambiguously attributed.
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
    /// The session is transient and the transcript is what the corpus keeps.
    pub fn transcript(&self) -> &[String] {
        &self.transcript
    }

    /// Negotiates, and returns the session identifier.
    ///
    /// A different identifier from a previous negotiation means the probe restarted, and
    /// nothing is resumed.
    ///
    /// A probe generates a session secret per startup and shows it on the target's display;
    /// it is sent as a fourth field after the version, so an older driver still works.
    /// `None` sends the three-field form, which a probe with a secret answers with
    /// [`Refusal::Unauthorised`] rather than a parse error.
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
    /// A malformed address or argument is refused. A well-formed but fatal one (zero,
    /// unmapped) is called, and the probe dies executing it, which arrives as
    /// [`Outcome::Died`]. Null is called rather than rejected, because what the platform
    /// does on a null call is itself a measurement.
    pub fn call(&mut self, address: u64, arguments: &[u64]) -> Result<Answer, ClientError> {
        let mut rendered = vec![format!("{address:#x}")];
        rendered.extend(arguments.iter().map(|argument| format!("{argument:#x}")));
        let borrowed: Vec<&str> = rendered.iter().map(String::as_str).collect();
        self.command("call", &borrowed)
    }

    /// Reads a run of guest memory.
    ///
    /// An unreadable address has two permitted answers: `refused|unmapped` from a probe
    /// that tests the address first, or [`Outcome::Died`] from one that faults. They are
    /// different facts, and a caller must handle both since it cannot know which build
    /// serves.
    pub fn read(&mut self, address: u64, length: u64) -> Result<Answer, ClientError> {
        self.command("read", &[&format!("{address:#x}"), &format!("{length:#x}")])
    }

    /// Closes the session cleanly.
    ///
    /// A session that ends without this is recorded as such, as a fact rather than an error.
    pub fn bye(&mut self) -> Result<Answer, ClientError> {
        self.command("bye", &[])
    }

    /// Sends a command, refusing to send one the probe never announced.
    ///
    /// The protocol puts this check on the client, so an unimplemented verb never reaches a
    /// target that faults easily.
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
    /// An acknowledgement is flushed before the command runs, which gives three non-answers:
    /// acknowledged then closed with no result is `died`; acknowledged then silent past the
    /// budget is `timeout`; closed with no acknowledgement is `lost`. None carries a value.
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
                // The far end closed; whether the command was acknowledged decides the outcome.
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
/// `hello` and `bye` need none, since they begin and end a session.
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
/// The probe listens and the driver connects, because the target shows an address a person
/// can read but cannot be told where a host is. This is the only place a socket appears;
/// everything above it is [`Client`]. A socket timeout is set slightly under the budget,
/// because a blocked read never consults the budget.
pub fn connect(
    address: &str,
    budget: Duration,
) -> Result<Client<std::net::TcpStream>, ClientError> {
    let stream = std::net::TcpStream::connect(address)?;
    stream.set_read_timeout(Some(budget))?;
    stream.set_write_timeout(Some(budget))?;
    Ok(Client::new(stream, budget))
}
