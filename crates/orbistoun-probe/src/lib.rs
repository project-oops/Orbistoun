//! Reading the records a conformance probe produces.
//!
//! obSCEne runs on real hardware and speaks the line protocol in its `docs/PROTOCOL.md`,
//! answering with `OBS|` records (D207). This crate turns those records, from a captured
//! transcript or a committed corpus, into values; the fixtures under
//! `tests/fixtures/protocol/` are real exchanges copied in as data.
//!
//! A command that did not answer is never readable as one that answered: `died` is not
//! `returned 0` and `timeout` is not `died`, so [`Outcome`] carries a value only in the
//! variant that observed one.

#![forbid(unsafe_code)]

pub mod client;
pub mod respond;

use std::collections::BTreeMap;
use std::fmt;

use orbistoun_hle::knowledge::{FunctionKnowledge, Oracle, Returns};
use orbistoun_nid::Nid;

/// The prefix every record carries.
const RECORD: &str = "OBS";
/// The prefix every request carries.
const REQUEST: &str = "CMD";
/// Field separator. A literal one cannot appear inside a field.
const SEPARATOR: char = '|';
/// The protocol version this crate reads.
pub const VERSION: u32 = 1;

/// Something a probe can do, announced during negotiation.
///
/// Read rather than assumed: a stand-in target without the platform's libraries announces
/// no [`Capability::Resolve`]. Unknown tokens are kept, since the protocol permits new
/// capabilities within a version.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Capability {
    /// Invoke a function by address.
    Call,
    /// Look a symbol up by name at run time.
    Resolve,
    /// Read guest memory.
    Read,
    /// Write guest memory. Off unless deliberately enabled.
    Write,
    /// Receive a code blob and execute it.
    Blob,
    /// Return to a known state without restarting.
    Reset,
    /// Run the compiled-in check suite.
    Report,
    /// Submit work to the graphics device.
    Gpu,
    /// A token this version does not know. Carried verbatim.
    Other(String),
}

impl Capability {
    /// Reads a capability token.
    pub fn parse(token: &str) -> Self {
        match token {
            "call" => Self::Call,
            "resolve" => Self::Resolve,
            "read" => Self::Read,
            "write" => Self::Write,
            "blob" => Self::Blob,
            "reset" => Self::Reset,
            "report" => Self::Report,
            "gpu" => Self::Gpu,
            other => Self::Other(other.to_owned()),
        }
    }
}

/// Why a command was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The verb is not implemented.
    UnknownVerb,
    /// The verb is known and this target cannot do it.
    Unsupported,
    /// An argument was malformed, including a sequence number that did not increase.
    BadArgument,
    /// Another session holds the probe.
    Busy,
    /// The capability was not announced during negotiation.
    NotNegotiated,
    /// The address is not mapped, established without faulting.
    Unmapped,
    /// The session secret was wrong or absent.
    ///
    /// The probe generates one per startup and displays it; a restart replaces it, so a
    /// stale key reads the same as a wrong one.
    Unauthorised,
    /// A reason this version does not know.
    Other(String),
}

impl Refusal {
    fn parse(token: &str) -> Self {
        match token {
            "unknown-verb" => Self::UnknownVerb,
            "unsupported" => Self::Unsupported,
            "bad-argument" => Self::BadArgument,
            "busy" => Self::Busy,
            "not-negotiated" => Self::NotNegotiated,
            "unmapped" => Self::Unmapped,
            "unauthorised" => Self::Unauthorised,
            other => Self::Other(other.to_owned()),
        }
    }
}

/// Who established an outcome.
///
/// A probe cannot report its own death, so a reader must tell what the probe said from
/// what was inferred from its silence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObservedBy {
    /// The probe said so.
    Probe,
    /// Inferred by the driver from silence or a closed connection.
    Driver,
}

/// What a command did.
///
/// [`Outcome::Died`], [`Outcome::Timeout`] and [`Outcome::Lost`] have no result field: a
/// call that faulted did not return, and no field means no unobserved number to read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The command completed and had nothing to return.
    Ok,
    /// The command returned this value from the integer return register.
    ///
    /// The integer register only; a floating-point answer is not read.
    Returned(u64),
    /// The thing asked about does not exist. A fact, not a failure.
    Absent,
    /// The command ended the process. Established by the driver.
    Died,
    /// The command has not returned yet. The probe may be alive, blocked, or looping.
    ///
    /// Not resolved into [`Outcome::Died`]: a blocked call and a dead process look
    /// identical from one end of a socket.
    Timeout,
    /// The connection closed and the probe never came back. Ambiguous, recorded as such.
    Lost,
    /// An outcome word this version does not know.
    ///
    /// Report enum values are open, so the line parses; an outcome not understood is not a
    /// result, so it carries no value and [`Outcome::answered`] is false.
    Unrecognised(String),
}

impl Outcome {
    /// The value observed, if one was.
    ///
    /// `None` for every non-answer.
    pub const fn value(&self) -> Option<u64> {
        match self {
            Self::Returned(value) => Some(*value),
            _ => None,
        }
    }

    /// Whether a result was actually observed.
    pub const fn answered(&self) -> bool {
        matches!(self, Self::Ok | Self::Returned(_) | Self::Absent)
    }

    /// Who could have established this outcome.
    pub const fn observed_by(&self) -> ObservedBy {
        match self {
            // Only the driver can report these: the probe was gone or silent.
            Self::Died | Self::Timeout | Self::Lost => ObservedBy::Driver,
            // An unrecognised word came from the probe, so the probe observed something;
            // that is not silence.
            _ => ObservedBy::Probe,
        }
    }
}

impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ok => write!(f, "ok"),
            Self::Returned(value) => write!(f, "returned {value:#x}"),
            Self::Absent => write!(f, "absent"),
            Self::Died => write!(f, "died"),
            Self::Timeout => write!(f, "timeout"),
            Self::Lost => write!(f, "lost"),
            Self::Unrecognised(word) => write!(f, "{word} (unrecognised)"),
        }
    }
}

/// How much a reader should trust one result, in the probe's own vocabulary.
///
/// The probe's grading vocabulary overlaps this project's without matching, so the word is
/// kept verbatim and [`Provenance::oracle`] maps it at the point of use, where the origin
/// is also known.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Provenance {
    /// The probe's own reasoning. Sensible, unconfirmed, could be wrong in any direction.
    Assumed,
    /// The target kernel derives from a documented system and that system's specification
    /// settles this specific case. Wrong only if the vendor changed a behaviour while
    /// keeping the name.
    Derived,
    /// ISO C or POSIX names the function and settles it.
    Spec,
    /// Vendor interface documentation describes this behaviour specifically.
    Documented,
    /// Observed on hardware.
    Hardware,
    /// A grade this version does not know.
    ///
    /// Not the same as no grade: an absent field claims nothing, while this claims
    /// something this reader cannot parse, which means the reader is out of date.
    Unrecognised(String),
}

impl Provenance {
    /// Reads a provenance token, or `None` if there was none.
    ///
    /// Absent is not a value: some record kinds carry no provenance field, and a default
    /// would invent a grade nobody assigned.
    pub fn parse(token: &str) -> Option<Self> {
        match token {
            "assumed" => Some(Self::Assumed),
            "derived" => Some(Self::Derived),
            "spec" => Some(Self::Spec),
            "documented" => Some(Self::Documented),
            "hardware" => Some(Self::Hardware),
            // An empty field is absent; anything else is a grade given and not understood,
            // which open enum values make expected.
            "" => None,
            other => Some(Self::Unrecognised(other.to_owned())),
        }
    }

    /// What this project would call the same fact, given where it was observed.
    ///
    /// `hardware` maps to `measured`, and `spec`, `documented` and `derived` to `published`,
    /// whose definition here includes the tree the target C library derives from; `assumed`
    /// stays `assumed`.
    ///
    /// A `hardware` result is `measured` only if the operator asserted the target (D246).
    /// A probe cannot certify its own machine, since inside an emulator it reports the
    /// emulator's version, so a session's claim is not evidence. Otherwise it is `assumed`.
    // `Hardware` and `Assumed` reach the same grade for opposite reasons, and separate arms
    // keep the demotion visible.
    #[allow(clippy::match_same_arms, reason = "the demotion must stay visible")]
    pub fn oracle(&self, origin: &Origin) -> Oracle {
        match self {
            Self::Hardware if origin.is_target => Oracle::Measured,
            // A stand-in measures itself, not the target.
            Self::Hardware => Oracle::Assumed,
            Self::Spec | Self::Documented | Self::Derived => Oracle::Published,
            Self::Assumed => Oracle::Assumed,
            // A grade not understood takes the weakest reading.
            Self::Unrecognised(_) => Oracle::Assumed,
        }
    }
}

/// What a check concluded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// The behaviour matched what was expected.
    Pass,
    /// Some of it matched.
    Partial,
    /// It did not match.
    Fail,
    /// It was not run.
    Skip,
    /// A status this version does not know.
    Other(String),
}

impl Status {
    fn parse(token: &str) -> Self {
        match token {
            "pass" => Self::Pass,
            "partial" => Self::Partial,
            "fail" => Self::Fail,
            "skip" => Self::Skip,
            other => Self::Other(other.to_owned()),
        }
    }
}

/// One line of a transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    /// A comment or a blank line.
    Note(String),
    /// A command the driver sent.
    Request {
        /// Sequence number, strictly increasing within a session.
        ///
        /// `None` where the transcript carried something that was not a number, a case the
        /// protocol specifies and refuses with sequence zero.
        seq: Option<u64>,
        /// The verb.
        verb: String,
        /// Arguments, verbatim.
        arguments: Vec<String>,
    },
    /// A record the probe or the driver emitted.
    Record(Record),
}

/// A record from a probe, or written by a driver about a probe's silence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Record {
    /// A command was received, written before it was carried out.
    Ack {
        /// Sequence of the command being acknowledged.
        seq: u64,
        /// The verb being acknowledged.
        verb: String,
    },
    /// Negotiation succeeded.
    Hello {
        /// Protocol version the probe will speak.
        version: u32,
        /// Identifier for this process. A new one means the probe restarted.
        session: String,
        /// What the probe can do.
        capabilities: Vec<Capability>,
    },
    /// What produced the answers in this session.
    Part {
        /// Session the metadata belongs to.
        session: String,
        /// Key, open-ended by design.
        key: String,
        /// Value, verbatim.
        value: String,
    },
    /// A command finished.
    Done {
        /// Sequence of the command.
        seq: u64,
        /// What it did.
        outcome: Outcome,
        /// Free text, carried verbatim.
        detail: String,
    },
    /// A command was refused.
    Refused {
        /// Sequence as sent, so a caller can see which line was rejected.
        seq: u64,
        /// Why.
        reason: Refusal,
    },
    /// What produced a report.
    ///
    /// A report carries no negotiation, so this is its nearest thing to an origin. It
    /// names the binary kind, not the device, so a report cannot say which hardware
    /// produced it.
    Build {
        /// Build identifier.
        build: String,
        /// Which kind of binary ran: `module`, `payload` or `host`.
        kind: String,
    },
    /// Whether a symbol exists, and how it is reached.
    ///
    /// A platform asked directly answers for symbols nothing imports, which no name
    /// recovered from a hash can cover.
    Sym {
        /// Library the symbol was looked for in.
        library: String,
        /// The symbol.
        symbol: String,
        /// `present` or `absent`.
        presence: String,
        /// How it is reached: `shared`, or whatever else a target reports.
        availability: String,
    },
    /// Whether a symbol exists, and where it resolved to.
    ///
    /// Separate from [`Self::Sym`]: the same first three fields, but `sym` says how the
    /// symbol is reached and `resolve` where it landed, and neither is a superset. The
    /// probe emits this from its by-name symbol census.
    Resolve {
        /// Library the symbol was looked for in.
        library: String,
        /// The symbol.
        symbol: String,
        /// `present` or `absent`.
        presence: String,
        /// Where it resolved to, verbatim as the probe wrote it.
        address: String,
    },
    /// A group of checks, with what it is establishing.
    Section {
        /// Section identifier, prefixed so it sorts into running order.
        id: String,
        /// Short title.
        title: String,
        /// What the section is for.
        purpose: String,
    },
    /// One field of the target's account of itself.
    ///
    /// Read the state, never only the value; see [`Confidence`].
    SysInfo {
        /// What the field is called.
        field: String,
        /// How firmly it was established, verbatim.
        state: String,
        /// The value, verbatim.
        value: String,
    },
    /// Where the probe is also writing its records.
    ///
    /// A run is never lost to a dropped connection, because the probe keeps its own copy.
    Sink {
        /// The path, on the target.
        path: String,
    },
    /// A run of memory, as hexadecimal.
    ///
    /// The same record a report uses, so one parser reads both.
    Bytes {
        /// What produced it: `read/0x<address>` for a memory read.
        id: String,
        /// Where it came from, in the probe's words.
        source: String,
        /// What kind of run this is.
        kind: String,
        /// Offset of this run within the whole request.
        offset: u64,
        /// The bytes, as hexadecimal, verbatim.
        hex: String,
    },
    /// How one section's checks came out.
    SectionTally {
        /// Section identifier, matching a [`Record::Section`].
        id: String,
        /// Checks that passed.
        pass: u32,
        /// Checks that partly passed.
        partial: u32,
        /// Checks that failed.
        fail: u32,
        /// Checks that did not run.
        skip: u32,
    },
    /// A check is about to run, naming what it will exercise.
    ///
    /// A `res` identifies its check by `section/name` only; this names the function, and
    /// the two pair by check identifier. Emitted before the call, like `ack`, so a `try`
    /// with no matching `res` names the call that did not return.
    Try {
        /// Check identifier, matching the `res` that follows.
        check: String,
        /// Library the symbol lives in.
        library: String,
        /// Symbol being exercised.
        symbol: String,
    },
    /// One check's result.
    Res {
        /// Check identifier, `section/name`.
        check: String,
        /// What it concluded.
        status: Status,
        /// The value observed, verbatim and possibly empty.
        value: String,
        /// Free text.
        detail: String,
        /// How much to trust it, or `None` on a record written before the field existed.
        provenance: Option<Provenance>,
    },
    /// One number a probe measured, in the section that measured it.
    ///
    /// Seven fields. Unlike a `res`, a measurement carries no verdict: it is a number and
    /// the name of what was counted, and its meaning is the consumer's.
    Measure {
        /// Section that took the measurement, prefixed so it sorts into running order.
        section: String,
        /// What was measured: a symbol, a path, a sysctl name, an encoded hash.
        subject: String,
        /// Which quantity of the subject this is.
        field: String,
        /// The value, verbatim.
        ///
        /// Not parsed here, since not every value is hexadecimal; [`Measurement::number`]
        /// parses and says when it could not.
        value: String,
        /// What the value counts: `bytes`, `ticks`, `address`, `offset`, and so on.
        unit: String,
    },
    /// Any other record kind, carried without interpretation.
    ///
    /// The protocol permits new record kinds within a version; the fields are kept so a
    /// later reader can use them.
    Other {
        /// Record kind.
        kind: String,
        /// Fields, verbatim.
        fields: Vec<String>,
    },
}

/// Why one line is outside the grammar.
///
/// The crate's one error enum: [`ParseError`] places it in a transcript and
/// [`client::ClientError::Malformed`] places it on a live stream.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LineError {
    /// A request line with an empty verb field.
    #[error("a request carries no verb")]
    NoVerb,
    /// A line that is not blank, not a comment, and neither a request nor a record.
    #[error("line begins with neither {REQUEST} nor {RECORD}: {0}")]
    Unrecognised(String),
    /// A `returned` outcome whose value is not hexadecimal.
    #[error("a returned outcome carries no hexadecimal value: {0:?}")]
    ReturnedWithoutValue(String),
    /// An outcome that did not answer, carrying a value anyway.
    #[error(
        "an outcome that did not answer carries a value - a command that did not answer has no result, and recording one would make a fiction indistinguishable from evidence"
    )]
    ValueWithoutAnswer,
    /// A record that needs a sequence number and has none that parses.
    #[error("{kind} record has no sequence number: {found:?}")]
    NoSequence {
        /// The record kind.
        kind: String,
        /// The field where the number should be.
        found: String,
    },
    /// A `hello` record whose version does not parse.
    #[error("hello carries no version: {0:?}")]
    NoVersion(String),
    /// A record line with an empty kind field.
    #[error("record carries no kind")]
    NoKind,
}

/// A transcript that could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// One-based line number.
    pub line: usize,
    /// What was wrong.
    pub detail: LineError,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.detail)
    }
}

impl std::error::Error for ParseError {}

/// Reads a number the protocol writes in hexadecimal with an `0x` prefix.
fn hex(text: &str) -> Option<u64> {
    let body = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))?;
    u64::from_str_radix(body, 16).ok()
}

/// Reads one line.
///
/// A line that is neither a request nor a record is a note, kept because transcript
/// comments carry the reasoning.
///
/// # Errors
///
/// When the line is outside the grammar.
pub fn parse_line(text: &str) -> Result<Line, LineError> {
    let trimmed = text.trim_end_matches(['\r', '\n']);
    if trimmed.trim().is_empty() || trimmed.trim_start().starts_with('#') {
        return Ok(Line::Note(trimmed.to_owned()));
    }

    let fields: Vec<&str> = trimmed.split(SEPARATOR).collect();
    match fields.first().copied() {
        Some(REQUEST) => {
            let seq = fields.get(1).copied().unwrap_or_default();
            let verb = fields.get(2).copied().unwrap_or_default();
            if verb.is_empty() {
                return Err(LineError::NoVerb);
            }
            Ok(Line::Request {
                // Not an error: the protocol specifies a non-numeric sequence as a refusal,
                // and a transcript capturing it must be readable.
                seq: seq.parse().ok(),
                verb: verb.to_owned(),
                arguments: fields[3.min(fields.len())..]
                    .iter()
                    .map(|f| (*f).to_owned())
                    .collect(),
            })
        }
        Some(RECORD) => parse_record(&fields).map(Line::Record),
        _ => Err(LineError::Unrecognised(trimmed.to_owned())),
    }
}

/// Reads an outcome word and the value beside it.
///
/// Separate from [`parse_record`] because it holds the rule that a non-answer carries no
/// value.
fn parse_outcome(word: &str, value: &str) -> Result<Outcome, LineError> {
    let outcome = match word {
        "ok" => Outcome::Ok,
        "absent" => Outcome::Absent,
        "died" => Outcome::Died,
        "timeout" => Outcome::Timeout,
        "lost" => Outcome::Lost,
        "returned" => Outcome::Returned(
            hex(value).ok_or_else(|| LineError::ReturnedWithoutValue(value.to_owned()))?,
        ),
        // Not an error: the probe may add outcome words without a version bump. An unknown
        // word means no result.
        other => Outcome::Unrecognised(other.to_owned()),
    };
    // A non-answer carrying a value is refused.
    if !outcome.answered() && !value.is_empty() {
        return Err(LineError::ValueWithoutAnswer);
    }
    Ok(outcome)
}

fn parse_record(fields: &[&str]) -> Result<Record, LineError> {
    let kind = fields.get(1).copied().unwrap_or_default();
    let at = |index: usize| fields.get(index).copied().unwrap_or_default();
    let sequence = |index: usize| -> Result<u64, LineError> {
        at(index).parse::<u64>().map_err(|_| LineError::NoSequence {
            kind: kind.to_owned(),
            found: at(index).to_owned(),
        })
    };

    match kind {
        "ack" => Ok(Record::Ack {
            seq: sequence(2)?,
            verb: at(3).to_owned(),
        }),
        "hello" => Ok(Record::Hello {
            version: at(2)
                .parse()
                .map_err(|_| LineError::NoVersion(at(2).to_owned()))?,
            session: at(3).to_owned(),
            capabilities: at(4)
                .split(',')
                .filter(|token| !token.is_empty())
                .map(Capability::parse)
                .collect(),
        }),
        "part" => Ok(Record::Part {
            session: at(2).to_owned(),
            key: at(3).to_owned(),
            value: at(4).to_owned(),
        }),
        "done" => Ok(Record::Done {
            seq: sequence(2)?,
            outcome: parse_outcome(at(3), at(4))?,
            detail: at(5).to_owned(),
        }),
        "sym" => Ok(Record::Sym {
            library: at(2).to_owned(),
            symbol: at(3).to_owned(),
            presence: at(4).to_owned(),
            availability: at(5).to_owned(),
        }),
        "resolve" => Ok(parse_resolve(at(2), at(3), at(4), at(5))),
        "section" => Ok(Record::Section {
            id: at(2).to_owned(),
            title: at(3).to_owned(),
            purpose: at(4).to_owned(),
        }),
        "sysinfo" => Ok(Record::SysInfo {
            field: at(2).to_owned(),
            state: at(3).to_owned(),
            value: at(4).to_owned(),
        }),
        "sink" => Ok(Record::Sink {
            path: at(2).to_owned(),
        }),
        "bytes" => Ok(Record::Bytes {
            id: at(2).to_owned(),
            source: at(3).to_owned(),
            kind: at(4).to_owned(),
            offset: at(5).parse().unwrap_or_default(),
            hex: at(6).to_owned(),
        }),
        "sectiontally" => {
            let count = |index: usize| at(index).parse().unwrap_or_default();
            Ok(Record::SectionTally {
                id: at(2).to_owned(),
                pass: count(3),
                partial: count(4),
                fail: count(5),
                skip: count(6),
            })
        }
        "build" => Ok(Record::Build {
            build: at(2).to_owned(),
            kind: at(3).to_owned(),
        }),
        "try" => Ok(Record::Try {
            check: at(2).to_owned(),
            library: at(3).to_owned(),
            symbol: at(4).to_owned(),
        }),
        "res" => Ok(Record::Res {
            check: at(2).to_owned(),
            status: Status::parse(at(3)),
            value: at(4).to_owned(),
            detail: at(5).to_owned(),
            // Absent rather than defaulted: a record without the field claims nothing.
            provenance: Provenance::parse(at(6)),
        }),
        "measure" => Ok(parse_measure(at(2), at(3), at(4), at(5), at(6))),
        "refused" => Ok(Record::Refused {
            seq: sequence(2)?,
            reason: Refusal::parse(at(3)),
        }),
        "" => Err(LineError::NoKind),
        other => Ok(Record::Other {
            kind: other.to_owned(),
            fields: fields[2..].iter().map(|f| (*f).to_owned()).collect(),
        }),
    }
}

/// A measured number, with the section that measured it.
///
/// Split out of [`parse_record`] for length.
fn parse_measure(section: &str, subject: &str, field: &str, value: &str, unit: &str) -> Record {
    Record::Measure {
        section: section.to_owned(),
        subject: subject.to_owned(),
        field: field.to_owned(),
        value: value.to_owned(),
        unit: unit.to_owned(),
    }
}

/// The probe's by-name census record.
///
/// Its own function because `parse_record` is at its line budget.
fn parse_resolve(library: &str, symbol: &str, presence: &str, address: &str) -> Record {
    Record::Resolve {
        library: library.to_owned(),
        symbol: symbol.to_owned(),
        presence: presence.to_owned(),
        address: address.to_owned(),
    }
}

/// Every line of a transcript, in order.
pub fn parse(text: &str) -> Result<Vec<Line>, ParseError> {
    text.lines()
        .enumerate()
        .map(|(index, line)| {
            parse_line(line).map_err(|detail| ParseError {
                line: index + 1,
                detail,
            })
        })
        .collect()
}

/// What a transcript establishes about one probe process.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Session {
    /// Identifier the probe gave itself.
    pub session: String,
    /// Protocol version in use.
    pub version: u32,
    /// What it announced it could do.
    pub capabilities: Vec<Capability>,
    /// What produced its answers, denormalised onto every record derived from it.
    pub parts: BTreeMap<String, String>,
}

impl Session {
    /// Whether a capability was announced.
    ///
    /// Asked before sending a command: a target with no system libraries announces no
    /// `resolve`.
    pub fn can(&self, capability: &Capability) -> bool {
        self.capabilities.contains(capability)
    }

    /// What the session claimed it was running on: a claim, never evidence, since a probe
    /// cannot certify its own machine. Grading uses [`Origin`], which the operator asserts.
    pub fn claimed_target(&self) -> Option<&str> {
        self.parts.get("target").map(String::as_str)
    }
}

/// One command and everything it produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exchange {
    /// Sequence number, where the request carried a valid one.
    pub seq: Option<u64>,
    /// The verb.
    pub verb: String,
    /// Arguments as sent.
    pub arguments: Vec<String>,
    /// Whether the command was acknowledged before it ran.
    pub acknowledged: bool,
    /// What it did, or `None` if the transcript ends before an answer.
    pub outcome: Option<Outcome>,
    /// Why it was refused, if it was.
    pub refusal: Option<Refusal>,
    /// Records emitted between the acknowledgement and the answer.
    pub records: Vec<Record>,
}

impl Exchange {
    /// Whether this command produced a usable result.
    pub fn answered(&self) -> bool {
        self.outcome.as_ref().is_some_and(Outcome::answered)
    }
}

/// A transcript read as sessions and the commands within them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Transcript {
    /// Every session the transcript covers, in order.
    ///
    /// More than one means the probe restarted, which a faulting command causes.
    pub sessions: Vec<Session>,
    /// Every command, in the order it was issued.
    pub exchanges: Vec<Exchange>,
    /// Records that arrived outside any command.
    ///
    /// A committed report has no commands, so all of its records land here.
    pub records: Vec<Record>,
}

impl Transcript {
    /// Reads a whole transcript.
    pub fn read(text: &str) -> Result<Self, ParseError> {
        let lines = parse(text)?;
        let mut transcript = Self::default();
        let mut current: Option<Exchange> = None;

        for line in lines {
            match line {
                Line::Note(_) => {}
                Line::Request {
                    seq,
                    verb,
                    arguments,
                } => {
                    if let Some(exchange) = current.take() {
                        transcript.exchanges.push(exchange);
                    }
                    current = Some(Exchange {
                        seq,
                        verb,
                        arguments,
                        acknowledged: false,
                        outcome: None,
                        refusal: None,
                        records: Vec::new(),
                    });
                }
                Line::Record(record) => match &record {
                    Record::Ack { .. } => {
                        if let Some(exchange) = current.as_mut() {
                            exchange.acknowledged = true;
                        }
                    }
                    Record::Hello {
                        version,
                        session,
                        capabilities,
                    } => transcript.sessions.push(Session {
                        session: session.clone(),
                        version: *version,
                        capabilities: capabilities.clone(),
                        parts: BTreeMap::new(),
                    }),
                    Record::Part {
                        session,
                        key,
                        value,
                    } => {
                        // Attached to the session it names rather than the most recent one,
                        // since a transcript spanning a restart carries records for both.
                        if let Some(found) = transcript
                            .sessions
                            .iter_mut()
                            .find(|candidate| candidate.session == *session)
                        {
                            found.parts.insert(key.clone(), value.clone());
                        }
                    }
                    Record::Done { outcome, .. } => {
                        if let Some(exchange) = current.as_mut() {
                            exchange.outcome = Some(outcome.clone());
                        }
                    }
                    Record::Refused { reason, .. } => {
                        if let Some(exchange) = current.as_mut() {
                            exchange.refusal = Some(reason.clone());
                        }
                    }
                    Record::Other { .. }
                    | Record::Measure { .. }
                    | Record::Res { .. }
                    | Record::Try { .. }
                    | Record::Build { .. }
                    | Record::Sym { .. }
                    | Record::Resolve { .. }
                    | Record::Section { .. }
                    | Record::SectionTally { .. }
                    | Record::Bytes { .. }
                    | Record::SysInfo { .. }
                    | Record::Sink { .. } => {
                        if let Some(exchange) = current.as_mut() {
                            exchange.records.push(record.clone());
                        } else {
                            transcript.records.push(record.clone());
                        }
                    }
                },
            }
        }
        if let Some(exchange) = current.take() {
            transcript.exchanges.push(exchange);
        }
        Ok(transcript)
    }

    /// Commands that were acknowledged and never answered.
    ///
    /// A probe that died mid-command. A driver turns these into `died` records; a
    /// transcript that simply stops leaves them here rather than inventing an outcome.
    pub fn unanswered(&self) -> impl Iterator<Item = &Exchange> {
        self.exchanges.iter().filter(|exchange| {
            exchange.acknowledged && exchange.outcome.is_none() && exchange.refusal.is_none()
        })
    }
}

/// What machine produced a run, as asserted by the operator.
///
/// A probe cannot certify its own machine: inside an emulator, the platform's version
/// query returns the emulator's version. The `part` records a session announces are claims;
/// the operator's assertion is what a grade rests on. Under [`Origin::unasserted`] no
/// result grades as measured, so a forgotten assertion yields assumptions, not false
/// measurements.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    /// What the operator says this ran on: the target, a named emulator, a stand-in part.
    pub device: String,
    /// Firmware or version, where the operator knows it.
    pub firmware: String,
    /// Whether the operator asserts this is the target platform.
    ///
    /// Not whether it is real hardware: a real stand-in device is not the thing being
    /// emulated, and its measurements must not grade as facts about the target.
    pub is_target: bool,
    /// Anything else the operator recorded, and anything the probe claimed.
    pub notes: BTreeMap<String, String>,
}

impl Origin {
    /// An origin nobody asserted.
    ///
    /// Nothing under this grades above an assumption.
    pub fn unasserted() -> Self {
        Self {
            device: "unasserted".to_owned(),
            firmware: String::new(),
            is_target: false,
            notes: BTreeMap::new(),
        }
    }

    /// An origin the operator has asserted.
    pub fn asserted(
        device: impl Into<String>,
        firmware: impl Into<String>,
        is_target: bool,
    ) -> Self {
        Self {
            device: device.into(),
            firmware: firmware.into(),
            is_target,
            notes: BTreeMap::new(),
        }
    }

    /// Adds what a session claimed about itself, as context and never as evidence, so a
    /// claim that disagrees with the operator stays visible.
    #[must_use]
    pub fn with_claims(mut self, session: &Session) -> Self {
        for (key, value) in &session.parts {
            self.notes.insert(format!("claimed-{key}"), value.clone());
        }
        self
    }

    /// Whether a device name is one this project knows is not the target.
    ///
    /// The device name carries the answer where it can, rather than asking the operator a
    /// second question. The list names stand-ins, not targets, so the caller treats an
    /// unrecognised name as not the target.
    pub fn is_known_stand_in(device: &str) -> bool {
        let device = device.to_ascii_lowercase();
        [
            "deck",
            "steamdeck",
            "steam deck",
            "host",
            "linux",
            "windows",
            "emulator",
            "shadps4",
            "rpcs3",
            "gpcs4",
            "kyty",
            "obliteration",
            // orbistoun answers the same protocol (D207), so its own transcripts must never
            // grade as facts about the platform.
            "orbistoun",
            "unasserted",
        ]
        .iter()
        .any(|known| device.contains(known))
    }

    /// How to describe this run in a citation.
    pub fn describe(&self) -> String {
        let firmware = if self.firmware.is_empty() {
            String::new()
        } else {
            format!(" firmware {}", self.firmware)
        };
        format!("{}{firmware}", self.device)
    }
}

/// One function, one thing established about it, and how firmly.
///
/// A `res` names only its check; pairing it with the preceding `try` says which function
/// it exercised and how firmly the result is known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// Library the symbol lives in.
    pub library: String,
    /// The symbol.
    pub symbol: String,
    /// The check that established it.
    pub check: String,
    /// What the check concluded.
    pub status: Status,
    /// The value observed, verbatim. Empty where the check observed no value.
    pub value: String,
    /// Free text from the record.
    pub detail: String,
    /// How firmly it is known, already adjusted for what produced it.
    ///
    /// `None` where the record carried no grade, which is not the same as
    /// [`Oracle::Assumed`].
    pub known_by: Option<Oracle>,
}

impl Finding {
    /// Whether this is strong enough to record as a fact rather than an assumption.
    pub fn is_fact(&self) -> bool {
        matches!(self.known_by, Some(Oracle::Measured | Oracle::Published))
    }

    /// This finding as an entry the knowledge base would accept.
    ///
    /// An entry must say how it is known, an outside source must be cited, and an
    /// `assumed` grade cites nothing (D180). A measurement demoted to `assumed` records
    /// its run in the note and an explicit assumption instead.
    ///
    /// Arity, purpose, argument names and return kind are not filled in: one observed call
    /// does not establish the function's shape, and a wrong `Returns` would hand the guest
    /// a wild pointer from a stub.
    pub fn knowledge(&self, origin: &Origin) -> FunctionKnowledge {
        let mut entry = FunctionKnowledge {
            name: self.symbol.clone(),
            known_by: self.known_by,
            ..FunctionKnowledge::default()
        };

        let observed = if self.value.is_empty() {
            format!("{:?} with no value observed", self.status).to_lowercase()
        } else {
            format!(
                "returns {} ({})",
                self.value,
                format!("{:?}", self.status).to_lowercase()
            )
        };

        match self.known_by {
            Some(oracle) if oracle.needs_citation() => {
                entry.cites = self.cites(origin);
                entry.edge_cases.push(observed);
            }
            Some(_) => {
                // A demoted measurement or a probe guess: its source goes in the note, never
                // in `cites`.
                entry.note = format!("{observed}; {}", self.cites(origin));
                entry.assumptions.push(format!(
                    concat!(
                        "{} - the operator did not assert real target hardware for {}, ",
                        "so this is an approximation of the target rather than a ",
                        "measurement of it"
                    ),
                    observed, origin.device
                ));
            }
            None => {
                // No grade: the record claims nothing, so the entry claims nothing either.
                entry.note = format!("{observed}; {} - ungraded", self.cites(origin));
            }
        }
        entry
    }

    /// A citation naming where the fact came from, for the entry it would become.
    ///
    /// Names the check and the part, so a measurement says what it was taken on.
    pub fn cites(&self, origin: &Origin) -> String {
        format!(
            "conformance probe, check {} on {}",
            self.check,
            origin.describe()
        )
    }
}

impl Transcript {
    /// The build a report came from, where it says.
    ///
    /// Present on a report, absent on a live transcript. It identifies the binary, not the
    /// machine, so it never substitutes for a session.
    pub fn build(&self) -> Option<(&str, &str)> {
        self.every_record().find_map(|record| match record {
            Record::Build { build, kind } => Some((build.as_str(), kind.as_str())),
            _ => None,
        })
    }

    /// Every check that named a function, paired with what it concluded.
    ///
    /// A `try` with no matching `res` is omitted rather than reported as failing, since
    /// nothing was concluded; [`Transcript::attempted_without_result`] lists those.
    pub fn findings(&self, origin: &Origin) -> Vec<Finding> {
        let mut attempted: BTreeMap<&str, (&str, &str)> = BTreeMap::new();
        let mut findings = Vec::new();

        for record in self.every_record() {
            match record {
                Record::Try {
                    check,
                    library,
                    symbol,
                } => {
                    attempted.insert(check.as_str(), (library.as_str(), symbol.as_str()));
                }
                Record::Res {
                    check,
                    status,
                    value,
                    detail,
                    provenance,
                } => {
                    let Some((library, symbol)) = attempted.get(check.as_str()) else {
                        // A result whose check never named its function; the check
                        // identifier is not evidence of one.
                        continue;
                    };
                    findings.push(Finding {
                        library: (*library).to_owned(),
                        symbol: (*symbol).to_owned(),
                        check: check.clone(),
                        status: status.clone(),
                        value: value.clone(),
                        detail: detail.clone(),
                        known_by: provenance.as_ref().map(|grade| grade.oracle(origin)),
                    });
                }
                _ => {}
            }
        }
        findings
    }

    /// Checks that announced a call and never reported a result.
    ///
    /// A call that ended the probe, seen from the report. Keyed by check, not symbol,
    /// because several checks exercise one function and a symbol can have concluded
    /// results beside an unconcluded check.
    pub fn attempted_without_result(&self) -> Vec<(String, String, String)> {
        let mut attempted: BTreeMap<&str, (&str, &str)> = BTreeMap::new();
        let mut concluded: Vec<&str> = Vec::new();
        for record in self.every_record() {
            match record {
                Record::Try {
                    check,
                    library,
                    symbol,
                } => {
                    attempted.insert(check.as_str(), (library.as_str(), symbol.as_str()));
                }
                Record::Res { check, .. } => concluded.push(check.as_str()),
                _ => {}
            }
        }
        attempted
            .into_iter()
            .filter(|(check, _)| !concluded.contains(check))
            .map(|(check, (library, symbol))| {
                (check.to_owned(), library.to_owned(), symbol.to_owned())
            })
            .collect()
    }

    /// Every record, whether it arrived inside a command or on its own.
    fn every_record(&self) -> impl Iterator<Item = &Record> {
        self.records.iter().chain(
            self.exchanges
                .iter()
                .flat_map(|exchange| exchange.records.iter()),
        )
    }
}

/// Whether a symbol exists on the target.
///
/// Existence is a property of an interface, unlike a return value, which depends on
/// arguments, state and the part. It is still graded by origin: a stand-in's symbol table
/// is sourced from name lists mined elsewhere, so its `present` is not a measurement. The
/// [`Oracle`] is `Measured` only when the operator asserts the target, and only then may
/// the fact source a name (D246).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolFact {
    /// Library the symbol was looked for in.
    pub library: String,
    /// The symbol.
    pub symbol: String,
    /// Whether it resolved.
    pub present: bool,
    /// How it is reached, verbatim, when the record carried it.
    ///
    /// `None` for a `resolve` record, which does not have the field; not an empty string,
    /// which would name a means as nothing.
    pub availability: Option<String>,
    /// Where it resolved to, when the record carried it. `None` for a `sym` record.
    pub address: Option<String>,
    /// How much this existence fact is worth, given what it ran on.
    ///
    /// `Oracle::Measured` only from the target; anything else is `Oracle::Assumed`, never a
    /// source for a name (D246).
    pub known_by: Oracle,
}

impl SymbolFact {
    /// Whether this fact may be used as the source of a name in the symbol database.
    ///
    /// The question the naming rule turns on, answered in one place so callers agree (D246).
    pub fn may_source_a_name(&self) -> bool {
        self.present && self.known_by == Oracle::Measured
    }
}

impl Transcript {
    /// Every symbol the run established the existence of, graded by what it ran on.
    ///
    /// Takes the origin because a stand-in's symbol table comes from name lists mined
    /// elsewhere, so its `present` must not read as the target answering (D246).
    pub fn symbols(&self, origin: &Origin) -> Vec<SymbolFact> {
        // The same demotion as behaviour grading: the question is whether the silicon was
        // the thing being emulated.
        let known_by = if origin.is_target {
            Oracle::Measured
        } else {
            Oracle::Assumed
        };
        self.every_record()
            .filter_map(|record| match record {
                Record::Sym {
                    library,
                    symbol,
                    presence,
                    availability,
                } => Some(SymbolFact {
                    library: library.clone(),
                    symbol: symbol.clone(),
                    // Anything that is not the word `present` is not a claim that it is.
                    present: presence == "present",
                    availability: Some(availability.clone()),
                    address: None,
                    known_by,
                }),
                // The probe's by-name census: the same existence fact, carrying where it
                // landed; it covers symbols no title imports.
                Record::Resolve {
                    library,
                    symbol,
                    presence,
                    address,
                } => Some(SymbolFact {
                    library: library.clone(),
                    symbol: symbol.clone(),
                    present: presence == "present",
                    availability: None,
                    address: Some(address.clone()),
                    known_by,
                }),
                _ => None,
            })
            .collect()
    }
}

/// One area of the platform, and how much of it came out green.
///
/// Per section rather than one total, because the same number of passes spread thinly or
/// concentrated in one area means different things. The probe's `purpose` line is carried
/// verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionCoverage {
    /// Section identifier, prefixed so it sorts into running order.
    pub id: String,
    /// Short title, empty if the tally arrived without its section.
    pub title: String,
    /// What the section establishes, in the probe's words.
    pub purpose: String,
    /// Checks that passed.
    pub pass: u32,
    /// Checks that partly passed.
    pub partial: u32,
    /// Checks that failed.
    pub fail: u32,
    /// Checks that did not run.
    pub skip: u32,
}

impl SectionCoverage {
    /// Every check the section accounts for.
    pub const fn total(&self) -> u32 {
        self.pass + self.partial + self.fail + self.skip
    }

    /// Whether everything in this section passed outright.
    ///
    /// A skip is not green: a check that did not run established nothing.
    pub const fn is_wholly_green(&self) -> bool {
        self.total() > 0 && self.pass == self.total()
    }
}

impl Transcript {
    /// Each section with its tally, in running order.
    ///
    /// A section with no tally, or a tally naming no section, still appears: dropping
    /// either would shrink the denominator.
    pub fn sections(&self) -> Vec<SectionCoverage> {
        let mut found: BTreeMap<String, SectionCoverage> = BTreeMap::new();
        for record in self.every_record() {
            match record {
                Record::Section { id, title, purpose } => {
                    let entry = found.entry(id.clone()).or_insert_with(|| SectionCoverage {
                        id: id.clone(),
                        title: String::new(),
                        purpose: String::new(),
                        pass: 0,
                        partial: 0,
                        fail: 0,
                        skip: 0,
                    });
                    entry.title.clone_from(title);
                    entry.purpose.clone_from(purpose);
                }
                Record::SectionTally {
                    id,
                    pass,
                    partial,
                    fail,
                    skip,
                } => {
                    let entry = found.entry(id.clone()).or_insert_with(|| SectionCoverage {
                        id: id.clone(),
                        title: String::new(),
                        purpose: String::new(),
                        pass: 0,
                        partial: 0,
                        fail: 0,
                        skip: 0,
                    });
                    entry.pass = *pass;
                    entry.partial = *partial;
                    entry.fail = *fail;
                    entry.skip = *skip;
                }
                _ => {}
            }
        }
        found.into_values().collect()
    }
}

/// Memory a read returned, reassembled.
///
/// A read that dies part way through still established the bytes before the fault, so a
/// partial read is returned.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Memory {
    /// Address the read started at, where the records said.
    pub address: Option<u64>,
    /// The bytes, in offset order.
    pub bytes: Vec<u8>,
    /// Runs whose hexadecimal could not be decoded.
    ///
    /// Counted rather than dropped, so a gap in the buffer is visible to the caller.
    pub undecodable: usize,
}

/// Reads a hexadecimal run into bytes.
///
/// An odd number of digits is refused rather than guessed at.
fn decode_hex(hex: &str) -> Option<Vec<u8>> {
    if hex.len() % 2 != 0 {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&hex[index..index + 2], 16).ok())
        .collect()
}

impl Transcript {
    /// Every run of memory in this transcript, reassembled in offset order.
    pub fn memory(&self) -> Memory {
        let mut runs: Vec<(u64, Vec<u8>)> = Vec::new();
        let mut memory = Memory::default();
        for record in self.every_record() {
            let Record::Bytes {
                id, offset, hex, ..
            } = record
            else {
                continue;
            };
            if memory.address.is_none() {
                // `read/0x8003f510`: one `0x`, and the address after it.
                memory.address = id
                    .rsplit_once("/0x")
                    .and_then(|(_, address)| u64::from_str_radix(address, 16).ok());
            }
            match decode_hex(hex) {
                Some(bytes) => runs.push((*offset, bytes)),
                None => memory.undecodable += 1,
            }
        }
        runs.sort_by_key(|(offset, _)| *offset);
        for (_, bytes) in runs {
            memory.bytes.extend(bytes);
        }
        memory
    }
}

/// How well the target established one fact about itself.
///
/// All three states can carry the value `unknown` with different meanings: `known` is a
/// reading through a confirmed signature, `unconfirmed` means the query resolves but the
/// probe has no confirmed signature for it, and `absent` means the platform has no such
/// query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Confidence {
    /// Read through a confirmed signature.
    Known,
    /// The query resolves; the probe cannot yet call it confidently.
    Unconfirmed,
    /// No such query on this platform.
    Absent,
    /// A state this version does not know.
    ///
    /// Not resolved to any of the others, so a consumer decides explicitly.
    Unrecognised(String),
}

impl Confidence {
    fn parse(token: &str) -> Self {
        match token {
            "known" => Self::Known,
            "unconfirmed" => Self::Unconfirmed,
            "absent" => Self::Absent,
            other => Self::Unrecognised(other.to_owned()),
        }
    }

    /// Whether this carries a reading rather than an explanation for its absence.
    pub const fn is_reading(&self) -> bool {
        matches!(self, Self::Known)
    }
}

/// One thing the target says about itself.
///
/// Observations, not provenance: inside an emulator every field answers as the emulator
/// chooses. Nothing here reaches [`Origin`], which only the operator asserts; these are for
/// display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelfReport {
    /// What the field is called: `memory`, `vram`, `generation`, and so on.
    pub field: String,
    /// How firmly the target established it.
    pub confidence: Confidence,
    /// The value, verbatim. Frequently `unknown`, which is why the confidence matters.
    pub value: String,
}

impl Transcript {
    /// What the target said about itself.
    ///
    /// Ordered as it arrived, which is the probe's chosen order.
    pub fn self_report(&self) -> Vec<SelfReport> {
        self.every_record()
            .filter_map(|record| match record {
                Record::SysInfo {
                    field,
                    state,
                    value,
                } => Some(SelfReport {
                    field: field.clone(),
                    confidence: Confidence::parse(state),
                    value: value.clone(),
                }),
                _ => None,
            })
            .collect()
    }
}

/// What a live answer is allowed to do for the guest.
///
/// A value asked of the target under the probe's state is likely right, which beats a stub,
/// so it is returned, recorded and labelled (D225). A handle or pointer is the exception:
/// it comes from the target's address space and is meaningless in the guest's. The rule
/// keys on the recorded return kind, which is checkable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Use {
    /// Hand it to the guest, and record it.
    Return,
    /// Record it, and do not hand it over.
    ///
    /// Not a failure: the answer is kept as evidence and withheld only as the guest's
    /// return value.
    RecordOnly,
}

/// Whether a live answer may be handed to the guest.
///
/// An unknown return kind (`None`) is [`Use::RecordOnly`].
pub fn usable(returns: Option<Returns>) -> Use {
    match returns {
        // A status is a number that means the same thing in any address space.
        Some(Returns::Status) => Use::Return,
        // Everything else, and everything unknown.
        _ => Use::RecordOnly,
    }
}

/// A live answer, ready to be written down.
///
/// Carries the arguments it was asked with and the fact that the state was the probe's
/// rather than the guest's, since both qualify the number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asked {
    /// The symbol asked about.
    pub symbol: String,
    /// The arguments it was asked with, as sent.
    pub arguments: Vec<u64>,
    /// What came back, whatever it was.
    pub outcome: Outcome,
    /// Whether the guest may use it.
    pub usable: Use,
}

impl Asked {
    /// Records the answer as a knowledge entry.
    ///
    /// The measurement is real, but the guest's process state differs from the probe's.
    /// The grade stays `measured` and the divergence is a stated assumption, which the
    /// assumption count then tracks.
    pub fn knowledge(&self, origin: &Origin) -> FunctionKnowledge {
        let arguments = self
            .arguments
            .iter()
            .map(|argument| format!("{argument:#x}"))
            .collect::<Vec<_>>()
            .join(", ");

        let mut entry = FunctionKnowledge {
            name: self.symbol.clone(),
            ..FunctionKnowledge::default()
        };

        match &self.outcome {
            Outcome::Returned(value) => {
                entry
                    .edge_cases
                    .push(format!("({arguments}) returned {value:#x}"));
                entry.known_by = Some(if origin.is_target {
                    Oracle::Measured
                } else {
                    Oracle::Assumed
                });
                if origin.is_target {
                    entry.cites = format!(
                        "asked live of {} - probe session, not the guest's process",
                        origin.describe()
                    );
                } else {
                    entry.note = format!("asked live of {}", origin.describe());
                }
                entry.assumptions.push(format!(
                    concat!(
                        "asked under the probe's state rather than the guest's, so the ",
                        "guest may not observe the same answer for ({})"
                    ),
                    arguments
                ));
                if self.usable == Use::RecordOnly {
                    entry.assumptions.push(
                        concat!(
                            "the return kind is not a plain status, so this value was ",
                            "recorded and not handed to the guest - a handle or pointer ",
                            "from the console's address space means nothing in this one"
                        )
                        .to_owned(),
                    );
                }
            }
            // A non-answer is a finding, not a value: asking killed the probe.
            outcome => {
                entry.note = format!(
                    "asked live of {} with ({arguments}): {outcome}",
                    origin.describe()
                );
                entry.assumptions.push(format!(
                    concat!(
                        "({}) did not answer - {}. No value was observed and none is ",
                        "recorded"
                    ),
                    arguments, outcome
                ));
            }
        }
        entry
    }
}

/// What a corpus establishes, and how firmly.
///
/// Counted by grade, because how many results are facts matters more than how many checks
/// ran.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Established {
    /// Results whose grade is a measurement of the target.
    pub measured: usize,
    /// Results settled by a published standard or documentation.
    pub published: usize,
    /// Results that are somebody's reasoning, including measurements demoted because they
    /// were taken on a stand-in.
    pub assumed: usize,
    /// Results carrying no grade at all.
    pub ungraded: usize,
    /// Commands acknowledged and never answered.
    pub unanswered: usize,
    /// Commands that ended the process, timed out, or vanished.
    ///
    /// Separate from [`Established::unanswered`]: these were reported as not answering,
    /// where an unanswered command is one the transcript simply stops after.
    pub did_not_return: usize,
}

impl Established {
    /// How many results carry a grade this project would treat as a fact.
    pub const fn facts(&self) -> usize {
        self.measured + self.published
    }

    /// Every result seen, whatever its grade.
    pub const fn total(&self) -> usize {
        self.measured + self.published + self.assumed + self.ungraded
    }
}

impl fmt::Display for Established {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "results   {} of {} are facts",
            self.facts(),
            self.total()
        )?;
        writeln!(f, "  measured  {}", self.measured)?;
        writeln!(f, "  published {}", self.published)?;
        writeln!(f, "  assumed   {}", self.assumed)?;
        if self.ungraded > 0 {
            writeln!(
                f,
                "  ungraded  {} - written before the grade existed, claiming nothing",
                self.ungraded
            )?;
        }
        if self.did_not_return > 0 {
            writeln!(
                f,
                "
{} command(s) did not return - reported as such, not recorded as returning anything",
                self.did_not_return
            )?;
        }
        if self.unanswered > 0 {
            writeln!(
                f,
                concat!(
                    "\n{} command(s) acknowledged and never answered - each one is a call ",
                    "that did not return, not a call that returned nothing"
                ),
                self.unanswered
            )?;
        }
        Ok(())
    }
}

impl Transcript {
    /// Counts what this transcript establishes, grading each result by where it was seen.
    ///
    /// The same `hardware` result is a measurement or an assumption depending on the
    /// origin.
    pub fn established(&self, origin: &Origin) -> Established {
        let mut summary = Established {
            unanswered: self.unanswered().count(),
            did_not_return: self
                .exchanges
                .iter()
                .filter(|exchange| exchange.outcome.as_ref().is_some_and(|o| !o.answered()))
                .count(),
            ..Established::default()
        };

        // Records inside a command (a live session) and standing alone (a committed corpus)
        // are the same evidence.
        {
            for record in self.every_record() {
                let Record::Res { provenance, .. } = record else {
                    continue;
                };
                let Some(provenance) = provenance else {
                    summary.ungraded += 1;
                    continue;
                };
                match provenance.oracle(origin) {
                    Oracle::Measured => summary.measured += 1,
                    // `Provenance::oracle` never yields `Differential`; the arm exists for
                    // exhaustiveness and counts it as published, a verified claim (D478).
                    Oracle::Published | Oracle::Differential => summary.published += 1,
                    Oracle::GuestObserved | Oracle::Assumed => summary.assumed += 1,
                }
            }
        }
        summary
    }
}

/// The section the target's kernel export table arrives under.
///
/// Named once, because a misspelling would read as a report without the table.
pub const KEXPORT_SECTION: &str = "140-oracle/kexport-table";

/// The field a kexport record puts its address in.
const KEXPORT_FIELD: &str = "vaddr";

/// One number a probe measured, with the section that took it and what it is worth.
///
/// Not a [`Finding`]: a finding carries a verdict, while a measurement is a quantity and
/// the name of what was counted, interpreted by whoever asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Measurement {
    /// Section that took it.
    pub section: String,
    /// What was measured.
    pub subject: String,
    /// Which quantity of the subject this is.
    pub field: String,
    /// The value, verbatim and unparsed.
    pub value: String,
    /// What the value counts.
    pub unit: String,
    /// `Oracle::Measured` only from the target, exactly as every other fact is graded.
    pub known_by: Oracle,
}

impl Measurement {
    /// The value as a number, or `None` when it is not one.
    ///
    /// Hexadecimal only with an `0x` prefix: a bare `10` is ten.
    #[must_use]
    pub fn number(&self) -> Option<u64> {
        hex(&self.value).or_else(|| self.value.parse().ok())
    }
}

/// One symbol the target's kernel exports, as its own export table spells it.
///
/// The table carries a hash and an address but no name; a name comes from this project's
/// own vocabulary by hashing a candidate (D242). It says which hashes the platform exports,
/// and two hashes at one address are one function under two names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KernelExport {
    /// The hash, decoded.
    pub nid: Nid,
    /// The eleven characters it arrived as, kept so a record can be found again by eye.
    pub encoded: String,
    /// Where it resolves in the kernel's own address space.
    pub vaddr: u64,
    /// `Oracle::Measured` only from the target.
    pub known_by: Oracle,
}

/// Two or more exports the target places at one address: one function, several names.
/// Given a name for any member, every other member is a variant of it that a search can
/// test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportAlias {
    /// The address they share.
    pub vaddr: u64,
    /// The exports there, in the order the table listed them.
    pub exports: Vec<KernelExport>,
}

impl Transcript {
    /// Every measurement the run recorded, graded by what it ran on.
    pub fn measurements(&self, origin: &Origin) -> Vec<Measurement> {
        let known_by = if origin.is_target {
            Oracle::Measured
        } else {
            Oracle::Assumed
        };
        self.every_record()
            .filter_map(|record| match record {
                Record::Measure {
                    section,
                    subject,
                    field,
                    value,
                    unit,
                } => Some(Measurement {
                    section: section.clone(),
                    subject: subject.clone(),
                    field: field.clone(),
                    value: value.clone(),
                    unit: unit.clone(),
                    known_by,
                }),
                _ => None,
            })
            .collect()
    }

    /// How many measurements each section took.
    ///
    /// So a consumer can say which parts of a report it did not use.
    pub fn measured_sections(&self) -> BTreeMap<String, usize> {
        let mut counts = BTreeMap::new();
        for record in self.every_record() {
            if let Record::Measure { section, .. } = record {
                *counts.entry(section.clone()).or_insert(0) += 1;
            }
        }
        counts
    }

    /// The kernel export table, decoded.
    ///
    /// A record whose subject is not eleven characters of the hash alphabet is skipped, not
    /// guessed at, since anything under the section would otherwise decode to a plausible
    /// hash.
    pub fn kernel_exports(&self, origin: &Origin) -> Vec<KernelExport> {
        let known_by = if origin.is_target {
            Oracle::Measured
        } else {
            Oracle::Assumed
        };
        self.every_record()
            .filter_map(|record| match record {
                Record::Measure {
                    section,
                    subject,
                    field,
                    value,
                    ..
                } if section == KEXPORT_SECTION && field == KEXPORT_FIELD => Some(KernelExport {
                    nid: orbistoun_nid::decode_nid(subject)?,
                    encoded: subject.clone(),
                    vaddr: hex(value)?,
                    known_by,
                }),
                _ => None,
            })
            .collect()
    }
}

/// Groups exports by the address they share, keeping only the addresses with more than one.
///
/// Ordered by address, so two reports compare line by line.
#[must_use]
pub fn export_aliases(exports: &[KernelExport]) -> Vec<ExportAlias> {
    let mut by_address: BTreeMap<u64, Vec<KernelExport>> = BTreeMap::new();
    for export in exports {
        by_address
            .entry(export.vaddr)
            .or_default()
            .push(export.clone());
    }
    by_address
        .into_iter()
        .filter(|(_, exports)| exports.len() > 1)
        .map(|(vaddr, exports)| ExportAlias { vaddr, exports })
        .collect()
}

/// One check two transcripts disagree about.
///
/// The conformance probe runs both on the hardware and under orbistoun, so its verdicts
/// compare directly: a check that passes there and fails here is a named defect, with the
/// probe's own sentence saying what it expected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Divergence {
    /// The check, `section/name`.
    pub check: String,
    /// What it concluded in the transcript taken as the reference.
    pub reference: Status,
    /// What it concluded in the one being judged.
    pub subject: Status,
    /// The subject's own words about it, empty when it said nothing.
    pub detail: String,
}

impl Transcript {
    /// Every check this transcript concluded, by name.
    ///
    /// The last verdict wins where a check ran twice, which is what a re-run means.
    #[must_use]
    pub fn verdicts(&self) -> BTreeMap<String, (Status, String)> {
        let mut out = BTreeMap::new();
        for record in self.every_record() {
            if let Record::Res {
                check,
                status,
                detail,
                ..
            } = record
            {
                out.insert(check.clone(), (status.clone(), detail.clone()));
            }
        }
        out
    }

    /// Checks that concluded differently here than in `reference`.
    ///
    /// Only where both ran it: a check one side skipped differs in reachability, not
    /// behaviour. Ordered worst-first, pass-there-fail-here before partial passes.
    #[must_use]
    pub fn diverges_from(&self, reference: &Self) -> Vec<Divergence> {
        let theirs = reference.verdicts();
        let mut out: Vec<Divergence> = self
            .verdicts()
            .into_iter()
            .filter_map(|(check, (subject, detail))| {
                let (their_status, _) = theirs.get(&check)?;
                (their_status != &subject).then(|| Divergence {
                    check,
                    reference: their_status.clone(),
                    subject,
                    detail,
                })
            })
            .collect();
        out.sort_by_key(|d| {
            (
                // Passed there and failed here, first.
                u8::from(!(d.reference == Status::Pass && d.subject == Status::Fail)),
                u8::from(d.reference != Status::Pass),
                d.check.clone(),
            )
        });
        out
    }
}
