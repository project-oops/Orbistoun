//! Reading the records a hardware probe produces.
//!
//! # What this is
//!
//! obSCEne is a conformance probe that runs on real hardware and answers questions this
//! project can otherwise only infer. It speaks a line protocol, specified in that project's
//! `docs/PROTOCOL.md`, and what comes back is a stream of `OBS|` records.
//!
//! This crate reads those records. It does not drive a session, it does not open a socket,
//! and it does not know what a Steam Deck is. Records arrive as bytes - from a captured
//! transcript, from a committed corpus - and become values a test or an implementation can
//! use.
//!
//! # Why this exists before there is any hardware
//!
//! The protocol ships with captured transcripts, and their stated purpose is that a
//! consumer can be built and tested without hardware attached. That is what happens here:
//! every fixture under `tests/fixtures/protocol/` is a real exchange, and parsing all of
//! them is the conformance test.
//!
//! The transcripts are **copied in as data**, never referenced across repositories. A test
//! that reads a sibling checkout fails for everyone who does not have one, and a build
//! dependency between the two projects is exactly the coupling D207 exists to prevent.
//!
//! # The one thing this type system is for
//!
//! A command that did not answer must never be readable as one that answered.
//!
//! `died` is not `returned 0`. `timeout` is not `died`. The protocol says a corpus blurring
//! those is worse than no corpus, because the fiction is indistinguishable from evidence -
//! and evidence is what this project is short of.
//!
//! So [`Outcome`] carries a value **only** in the variant that observed one. There is no
//! field to read for a call that died, no default to fall through to, and no way to write
//! code that treats the two alike without saying so out loud. That is the whole design.

#![forbid(unsafe_code)]

pub mod client;
pub mod respond;

use std::collections::BTreeMap;
use std::fmt;

use orbistoun_hle::knowledge::{FunctionKnowledge, Oracle, Returns};

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
/// Read rather than assumed: a stand-in target with none of the platform's libraries
/// announces no [`Capability::Resolve`], and a consumer discovers that here instead of
/// asking a question the target cannot answer.
///
/// Unknown tokens are **kept**, not dropped. The protocol permits new capabilities within a
/// version, and a reader that silently discarded them would report a newer probe as less
/// capable than it is.
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
    /// An argument was malformed - including a sequence number that did not increase.
    BadArgument,
    /// Another session holds the probe.
    Busy,
    /// The capability was not announced during negotiation.
    NotNegotiated,
    /// The address is not mapped, established without faulting.
    Unmapped,
    /// The session secret was wrong or absent.
    ///
    /// The probe generates one per startup and displays it, because a console has no other
    /// channel. A restart replaces it, so this is what a stale key looks like as well as a
    /// wrong one.
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
/// A probe cannot report its own death - the process is gone - so the facts that matter
/// most are the ones it did *not* say. Keeping the distinction means a reader can tell
/// something the system reported from something inferred from its silence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObservedBy {
    /// The probe said so.
    Probe,
    /// Inferred by the driver from silence or a closed connection.
    Driver,
}

/// What a command did.
///
/// # The variants that carry no value
///
/// [`Outcome::Died`], [`Outcome::Timeout`] and [`Outcome::Lost`] have no result field, and
/// that is deliberate rather than an omission. A call that faulted did not return zero; it
/// did not return. Giving those variants a value - even an `Option` - creates a place for a
/// reader to find a number that was never observed, and a number found in a record is
/// eventually trusted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The command completed and had nothing to return.
    Ok,
    /// The command returned this value from the integer return register.
    ///
    /// The integer register and nothing else - a function returning a float leaves its
    /// answer somewhere this does not read, and the record says `returned` because that is
    /// what was observed.
    Returned(u64),
    /// The thing asked about does not exist. A fact, not a failure.
    Absent,
    /// The command ended the process. Established by the driver.
    Died,
    /// The command has not returned yet. The probe may be alive, blocked, or looping.
    ///
    /// Deliberately not resolved into [`Outcome::Died`]: a blocked call and a dead process
    /// look identical from one end of a socket, and the honest record says which was
    /// observed rather than which was guessed.
    Timeout,
    /// The connection closed and the probe never came back. Ambiguous, recorded as such.
    Lost,
    /// An outcome word this version does not know.
    ///
    /// # Two rules meeting, and neither yielding
    ///
    /// Report enum values are **open**: the probe may add one without bumping the format
    /// version, and a reader degrades rather than failing. Refusing the line would make this
    /// consumer break on a stream it was told to expect.
    ///
    /// A command that did not answer is **never** recorded as having answered. An outcome
    /// nobody here understands has not been understood, so it cannot be a result.
    ///
    /// Both hold at once: the line parses, and the outcome carries no value and reports
    /// [`Outcome::answered`] as false. Degrading is not the same as assuming the best.
    Unrecognised(String),
}

impl Outcome {
    /// The value observed, if one was.
    ///
    /// `None` for every non-answer, and there is no variant this could invent a number for.
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
            // An unrecognised word arrived *from* the probe, so the probe observed
            // something - this reader simply cannot say what. That is a different fact
            // from silence and is not filed with it.
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
/// # Why this is not simply mapped on arrival
///
/// The probe and this project both grade their facts, and the two vocabularies overlap
/// without matching. Translating on the way in would lose the original, and the original is
/// what a later reader needs when a mapping turns out to have been too generous.
///
/// So the probe's word is kept verbatim and [`Provenance::oracle`] performs the mapping at
/// the point of use, where the session is also in hand - which turns out to matter more
/// than the word itself.
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
    /// Observed on a console.
    Hardware,
    /// A grade this version does not know.
    ///
    /// **Not the same as no grade at all.** An absent field means the record predates
    /// grading and claims nothing; this means the record claims something and this reader
    /// cannot say what. Both end up ungraded, but only one of them says *the consumer is
    /// out of date* - and that is worth surfacing rather than quietly filing under
    /// "claims nothing".
    Unrecognised(String),
}

impl Provenance {
    /// Reads a provenance token, or `None` if there was none.
    ///
    /// **Absent is not a value.** The record format gained this field after some record
    /// kinds were already being written, and its own documentation records that the table
    /// drifted and a parser written against it would have been wrong about half the stream.
    /// A record without the field is one that predates it, not one claiming anything, and
    /// treating the gap as a default would invent a grade nobody assigned.
    pub fn parse(token: &str) -> Option<Self> {
        match token {
            "assumed" => Some(Self::Assumed),
            "derived" => Some(Self::Derived),
            "spec" => Some(Self::Spec),
            "documented" => Some(Self::Documented),
            "hardware" => Some(Self::Hardware),
            // An empty field is absent; anything else is a grade that was given and not
            // understood. Report enum values are open, so this is expected rather than
            // exceptional.
            "" => None,
            other => Some(Self::Unrecognised(other.to_owned())),
        }
    }

    /// What this project would call the same fact, given where it was observed.
    ///
    /// # The mapping
    ///
    /// | probe | here | why |
    /// |---|---|---|
    /// | `hardware` | `measured` | observed on a console by a conformance probe, which is this project's definition of measured, word for word |
    /// | `spec` | `published` | ISO C or POSIX settles it |
    /// | `documented` | `published` | vendor interface documentation describes it specifically |
    /// | `derived` | `published` | this project's `published` explicitly covers the tree the target C library derives from |
    /// | `assumed` | `assumed` | |
    ///
    /// `derived` is the one worth pausing on, because the conservative instinct is to
    /// downgrade it. That would be wrong rather than careful: it is genuinely stronger than
    /// an assumption, and `published` here is *defined* to include the derivation case. A
    /// grade that under-reports is not free - it makes a fact indistinguishable from a
    /// guess, and this project's whole accounting exists so the two can be told apart.
    ///
    /// # The origin is not a detail, and it is not read off the wire
    ///
    /// **A `hardware` result is only `measured` if the operator asserted real hardware.**
    ///
    /// Not if the session *said* so. A probe cannot certify its own machine: inside an
    /// emulator it reports the emulator's version as the platform's, so `target|console`
    /// arriving on the wire is a claim and not evidence. The operator's assertion is the
    /// only thing that separates a measurement of a console from an emulator's answer
    /// wearing a console's badge.
    ///
    /// Everything else is `assumed` - which is the honest grade for a number that is
    /// probably right and has never been checked against the thing it describes.
    ///
    /// Silent promotion is the failure this guards. A number measured on one device and
    /// read later as authoritative for another is wrong with nothing in the record saying
    /// so - and that has already cost this project months, pointed at the wrong GPU
    /// generation with nothing complaining.
    // `Hardware` and `Assumed` reach the same grade and must not be merged into one arm.
    // They arrive there for opposite reasons: one is a measurement demoted because it was
    // taken on the wrong part, the other never claimed anything. Collapsing them would
    // delete the only place the demotion is visible in the code, and a reader would find a
    // mapping that looks like it never downgrades anything.
    #[allow(clippy::match_same_arms, reason = "the demotion must stay visible")]
    pub fn oracle(&self, origin: &Origin) -> Oracle {
        match self {
            Self::Hardware if origin.is_target => Oracle::Measured,
            // Real hardware, wrong hardware - a stand-in measures itself accurately and
            // says nothing about the target.
            Self::Hardware => Oracle::Assumed,
            Self::Spec | Self::Documented | Self::Derived => Oracle::Published,
            Self::Assumed => Oracle::Assumed,
            // A grade nobody here understands cannot be honoured, and the safe reading is
            // the weakest one. Never the strongest: an unknown word must not become a
            // measurement on the strength of being unfamiliar.
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
        /// `None` where the transcript carried something that was not a number - which is
        /// itself a case the protocol specifies, refused with sequence zero.
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
    /// A command was received, written *before* it was carried out.
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
    /// A report is not a session: it carries no negotiation, so there is no `hello` and no
    /// `part`. This is the nearest thing it has to an origin - and it names the *binary
    /// kind*, `module`, `payload` or `host`, rather than the device. **A report therefore
    /// cannot say which hardware produced it**, which is a limit worth reading off the
    /// record rather than working around.
    Build {
        /// Build identifier.
        build: String,
        /// Which kind of binary ran: `module`, `payload` or `host`.
        kind: String,
    },
    /// Whether a symbol exists, and how it is reached.
    ///
    /// The cheapest useful fact a probe produces, and one this project cannot establish any
    /// other way: a name recovered from a hash can only ever be a name something already
    /// imports, whereas a platform asked directly answers for symbols nothing imports at
    /// all.
    Sym {
        /// Library the symbol was looked for in.
        library: String,
        /// The symbol.
        symbol: String,
        /// `present` or `absent`.
        presence: String,
        /// How it is reached - `shared`, and whatever else a target reports.
        availability: String,
    },
    /// Whether a symbol exists, and where it resolved to.
    ///
    /// # Why this is separate from [`Self::Sym`]
    ///
    /// They carry the same first three fields and a different fourth: `sym` says *how* the
    /// symbol is reached, `resolve` says *where* it landed. Neither is a superset, so
    /// folding them would mean inventing whichever field the record did not have.
    ///
    /// The probe emits this from its symbol census. It was reaching this reader and being
    /// carried as [`Self::Other`] - kept, correctly, but contributing no existence fact -
    /// so a by-name census answered a question nothing here was asking (D245).
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
    /// Read the state, never only the value - see [`Confidence`].
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
    /// The same record a report uses, so a parser written for one reads the other without
    /// knowing this protocol exists.
    Bytes {
        /// What produced it - `read/0x<address>` for a memory read.
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
    /// The record that makes a result mean something. A `res` identifies its check by
    /// `section/name` and says nothing about which function was called; this says which,
    /// and the two are paired by check identifier.
    ///
    /// It is also emitted *before* the call, for the same reason `ack` is written before a
    /// command runs: a `try` with no matching `res` names the call that did not return.
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
    /// Any other record kind, carried without interpretation.
    ///
    /// The protocol permits new record kinds within a version and requires a consumer to
    /// ignore what it does not recognise. Ignoring is not the same as discarding: the
    /// fields are kept so a later reader can make sense of them.
    Other {
        /// Record kind.
        kind: String,
        /// Fields, verbatim.
        fields: Vec<String>,
    },
}

/// A transcript that could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// One-based line number.
    pub line: usize,
    /// What was wrong.
    pub detail: String,
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
/// A line that is neither a request nor a record is a note. Transcripts are commented
/// prose as much as data, and the comments carry the reasoning - dropping them would
/// throw away the part a person reads.
pub fn parse_line(text: &str) -> Result<Line, String> {
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
                return Err("a request carries no verb".to_owned());
            }
            Ok(Line::Request {
                // Deliberately not an error. A sequence that is not a number is a case the
                // protocol specifies - it is refused, and a transcript capturing that
                // refusal has to be readable or the case cannot be tested.
                seq: seq.parse().ok(),
                verb: verb.to_owned(),
                arguments: fields[3.min(fields.len())..]
                    .iter()
                    .map(|f| (*f).to_owned())
                    .collect(),
            })
        }
        Some(RECORD) => parse_record(&fields).map(Line::Record),
        _ => Err(format!(
            "line begins with neither {REQUEST} nor {RECORD}: {trimmed}"
        )),
    }
}

/// Reads an outcome word and the value beside it.
///
/// Split out of [`parse_record`] because it carries the rule the whole crate is shaped
/// around, and a rule worth stating is worth being able to find.
fn parse_outcome(word: &str, value: &str) -> Result<Outcome, String> {
    let outcome = match word {
        "ok" => Outcome::Ok,
        "absent" => Outcome::Absent,
        "died" => Outcome::Died,
        "timeout" => Outcome::Timeout,
        "lost" => Outcome::Lost,
        "returned" => Outcome::Returned(hex(value).ok_or_else(|| {
            format!("a returned outcome carries no hexadecimal value: {value:?}")
        })?),
        // Not an error. The probe is permitted to add outcome words without a version
        // bump, so a reader that refused the line would break on a stream it was told to
        // expect. It degrades instead - and degrading means "no result", not "probably
        // fine".
        other => Outcome::Unrecognised(other.to_owned()),
    };
    // A non-answer carrying a value is the exact confusion this crate exists to prevent,
    // so it is refused at the door rather than parsed into a shape that cannot hold it.
    if !outcome.answered() && !value.is_empty() {
        return Err(concat!(
            "an outcome that did not answer carries a value - a command that did not ",
            "answer has no result, and recording one would make a fiction ",
            "indistinguishable from evidence"
        )
        .to_owned());
    }
    Ok(outcome)
}

fn parse_record(fields: &[&str]) -> Result<Record, String> {
    let kind = fields.get(1).copied().unwrap_or_default();
    let at = |index: usize| fields.get(index).copied().unwrap_or_default();
    let sequence = |index: usize| -> Result<u64, String> {
        at(index)
            .parse::<u64>()
            .map_err(|_| format!("{kind} record has no sequence number: {:?}", at(index)))
    };

    match kind {
        "ack" => Ok(Record::Ack {
            seq: sequence(2)?,
            verb: at(3).to_owned(),
        }),
        "hello" => Ok(Record::Hello {
            version: at(2)
                .parse()
                .map_err(|_| format!("hello carries no version: {:?}", at(2)))?,
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
            // Absent rather than defaulted. A record predating the field claims nothing,
            // and inventing a grade for it would be the one thing this crate exists to
            // stop.
            provenance: Provenance::parse(at(6)),
        }),
        "refused" => Ok(Record::Refused {
            seq: sequence(2)?,
            reason: Refusal::parse(at(3)),
        }),
        "" => Err("record carries no kind".to_owned()),
        other => Ok(Record::Other {
            kind: other.to_owned(),
            fields: fields[2..].iter().map(|f| (*f).to_owned()).collect(),
        }),
    }
}

/// The probe's by-name census record.
///
/// Its own function only because `parse_record` is at its line budget; the reason it exists
/// is in D245.
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
    /// The question to ask before sending a command, and the reason a consumer never has
    /// to assume: a target with no system libraries announces no `resolve`, and asking it
    /// to resolve a symbol is a question it cannot answer rather than one it answers badly.
    pub fn can(&self, capability: &Capability) -> bool {
        self.capabilities.contains(capability)
    }

    /// What the session *claimed* it was running on.
    ///
    /// **A claim, never evidence.** A probe cannot certify its own machine - inside an
    /// emulator it reports the emulator's version as the platform's - so this says what
    /// arrived on the wire and nothing about what is true. Grading uses [`Origin`], which
    /// the operator asserts.
    ///
    /// Kept because a claim that disagrees with the operator is worth seeing, and because
    /// a transcript is easier to read with it than without.
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
    /// More than one means the probe restarted - a faulting command ends it, and a fresh
    /// identifier is how that becomes visible rather than being silently continuous.
    pub sessions: Vec<Session>,
    /// Every command, in the order it was issued.
    pub exchanges: Vec<Exchange>,
    /// Records that arrived outside any command.
    ///
    /// A *corpus* has no commands in it. The session transcript is the interface; what gets
    /// committed is the report the run produced, and that is records all the way down. A
    /// reader that only looked inside exchanges would find nothing in the artefact that
    /// actually matters - which is precisely what happened before this field existed.
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
                        // Attached to the session it names rather than the most recent one.
                        // A transcript spanning a restart carries records for both, and
                        // binding metadata to whichever came last would attribute one
                        // process's answers to another.
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
    /// The shape of a probe that died mid-command, seen from the other end. A driver turns
    /// these into `died` records; a transcript that simply stops leaves them here, which is
    /// a more honest reading than inventing an outcome for them.
    pub fn unanswered(&self) -> impl Iterator<Item = &Exchange> {
        self.exchanges.iter().filter(|exchange| {
            exchange.acknowledged && exchange.outcome.is_none() && exchange.refusal.is_none()
        })
    }
}

/// What machine produced a run, **as asserted by the operator**.
///
/// # Why this is not read off the wire
///
/// A probe cannot certify its own machine. Running inside an emulator, obSCEne's call to
/// the platform's version query returns *the emulator's* chosen version - so a probe that
/// stamped that as `firmware=` would be putting an emulator's answer in a console's badge.
/// It would look exactly like a measurement of real hardware, and it is the one confusion
/// this project's whole grading vocabulary exists to catch.
///
/// The `part` records a session announces are therefore **claims**, useful for reading a
/// transcript and worthless as evidence of what ran. The operator - the person who knows
/// whether the thing on the desk is a console or a window on a laptop - asserts it, and
/// that assertion is what a grade rests on.
///
/// # The default is the safe one
///
/// [`Origin::unasserted`] says nothing was claimed, and under it **no result can be graded
/// as measured**. That is deliberate: a client that forgets to ask the operator produces a
/// corpus of assumptions, which is recoverable, rather than a corpus of measurements that
/// were never measured, which is not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    /// What the operator says this ran on - a console, a named emulator, a stand-in part.
    pub device: String,
    /// Firmware or version, where the operator knows it.
    pub firmware: String,
    /// Whether the operator asserts this **is the target platform**.
    ///
    /// # Not "is it real hardware", and the difference is a bug that was live
    ///
    /// This field was called `real_hardware` and it was wrong in the one direction that
    /// matters. A Steam Deck **is** real hardware. Somebody connecting one and reading that
    /// name honestly would assert it, and every measurement taken on a Deck would be graded
    /// as a fact about the console - which is precisely the silent promotion the whole
    /// mechanism exists to prevent, reachable by an accurate reading of the field's own
    /// name.
    ///
    /// The grading question was never whether the silicon was real. It is whether the
    /// silicon was **the thing being emulated**. A Deck is real and is not it; an emulator
    /// is neither.
    pub is_target: bool,
    /// Anything else the operator recorded, and anything the probe claimed.
    pub notes: BTreeMap<String, String>,
}

impl Origin {
    /// An origin nobody asserted.
    ///
    /// Nothing under this grades above an assumption, which is the correct answer when
    /// nothing is known about what ran.
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

    /// Adds what a session *claimed* about itself, kept as context and never as evidence.
    ///
    /// Carried so a transcript remains readable and so a claim that disagrees with the
    /// operator is visible rather than lost - an emulator announcing `target|console` next
    /// to an operator saying otherwise is worth seeing.
    #[must_use]
    pub fn with_claims(mut self, session: &Session) -> Self {
        for (key, value) in &session.parts {
            self.notes.insert(format!("claimed-{key}"), value.clone());
        }
        self
    }

    /// Whether a device name is one this project knows is **not** the target.
    ///
    /// # Why a list rather than a question
    ///
    /// Asking "is this the target?" separately from "what is this?" asks the operator the
    /// same thing twice, and the second question is the one they have to reason about
    /// rather than simply know. Somebody in a hurry answers it the way that gets their data
    /// graded.
    ///
    /// So the device name carries the answer wherever it can, and the list is of the
    /// **stand-ins** rather than the targets. That direction is deliberate: an unrecognised
    /// name defaults to *not the target*, so a new emulator nobody has listed is treated
    /// conservatively rather than promoted by default. Being wrong here costs a demotion,
    /// which is recoverable; the other direction is not.
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
            // Ourselves, and the one that would be easiest to forget. orbistoun answers
            // the same command protocol now, so a transcript can be *this emulator's own
            // account of itself* - and grading that as a fact about the platform would be
            // the project marking its own homework (D236).
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
/// # Why this is not simply a `res` record
///
/// A result identifies its check by `section/name` and never says which function it
/// exercised. The `try` emitted before it does. Pairing them is the whole step from "this
/// check passed" to "this function returns this, and here is how well we know it" - and the
/// second is the only form this project can act on.
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
    /// `None` where the record carried no grade - which is not the same as a weak grade,
    /// and is why this is an `Option` rather than defaulting to [`Oracle::Assumed`]. A
    /// caller that wants to treat the two alike has to say so.
    pub known_by: Option<Oracle>,
}

impl Finding {
    /// Whether this is strong enough to record as a fact rather than an assumption.
    pub fn is_fact(&self) -> bool {
        matches!(self.known_by, Some(Oracle::Measured | Oracle::Published))
    }

    /// This finding as an entry the knowledge base would accept.
    ///
    /// # The rules it has to satisfy, and the one that bites
    ///
    /// An entry recording behaviour must say how it is known. A grade that claims an
    /// outside source must cite one. And a grade of `assumed` must cite **nothing** -
    /// because a citation beside a guess reads as evidence at a glance, which is the exact
    /// confusion the field exists to stop.
    ///
    /// That last rule bites here, and correctly. A measurement taken on a stand-in is
    /// demoted to `assumed`, and then it may not carry the citation naming the run it came
    /// from - even though that run is known precisely. The information does not vanish: it
    /// goes into the note and into an explicit assumption, where it reads as *the reason
    /// this is not settled* rather than as the authority for it.
    ///
    /// Being able to say where a guess came from is useful. Being able to say it in the
    /// field reserved for established facts is how a guess becomes one.
    ///
    /// # What is deliberately not filled in
    ///
    /// Arity, purpose, argument names, and what kind of value the function returns. A check
    /// observed one call with one set of arguments; it did not establish the shape of the
    /// function. Inferring arity from a single call would be reading one observation as a
    /// rule, and `Returns` in particular decides what an unimplemented stub hands back -
    /// getting it wrong hands the guest a wild pointer.
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
                // A demoted measurement, or something the probe was itself guessing at.
                // Where it came from belongs in the note, never in `cites`.
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
                // No grade at all. The record predates the field, so it claims nothing and
                // this entry must not claim anything either - not even that it is a guess,
                // which would be a grade nobody assigned.
                entry.note = format!("{observed}; {} - ungraded", self.cites(origin));
            }
        }
        entry
    }

    /// A citation naming where the fact came from, for the entry it would become.
    ///
    /// Names the check and the part, because "measured" without saying on what is the
    /// claim this project has already been burned by.
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
    /// Present on a report, absent on a live transcript, and **never a substitute for a
    /// session**: it identifies the binary, not the machine. Anything graded from a report
    /// alone is therefore ungraded, and saying so is better than picking a device.
    pub fn build(&self) -> Option<(&str, &str)> {
        self.every_record().find_map(|record| match record {
            Record::Build { build, kind } => Some((build.as_str(), kind.as_str())),
            _ => None,
        })
    }

    /// Every check that named a function, paired with what it concluded.
    ///
    /// A `try` with no matching `res` is **omitted rather than reported as failing**: the
    /// probe announced a call and never came back, so nothing was concluded. Recording that
    /// as a failure would be recording an outcome nobody observed, which is the same error
    /// as reading a death as a return value. [`Transcript::attempted_without_result`] lists
    /// them, because a call that killed the probe is a finding of its own kind.
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
                        // A result for a check that never announced what it was exercising.
                        // Nothing here can say which function it concerns, and guessing
                        // from the check identifier would be reading a name as evidence.
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
    /// The shape of a call that ended the probe, seen from the report rather than the wire.
    ///
    /// **Keyed by check, not by symbol.** Several checks exercise one function - a real
    /// report opens a missing path and then a null one - so a symbol can have concluded
    /// results and an unconcluded check at the same time. Reporting "this symbol did not
    /// conclude" would then contradict a finding sitting beside it, and the specific check
    /// is what a reader needs to repeat it anyway.
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
/// # Why this is worth its own type
///
/// It is a different kind of fact from a return value: a return value depends on arguments,
/// on state and on the part, where existence is a property of an interface.
///
/// **It is graded the same way regardless**, and the reasoning that said otherwise was
/// wrong in a way that mattered. It used to read: *a symbol that resolves, resolves - so a
/// `present` from a stand-in still says the name is spelled correctly*. That is true of what
/// the stand-in believes and says nothing about the platform, because **a stand-in's symbol
/// table is itself sourced from name lists mined out of other projects**.
///
/// So "confirmed present by an emulator" is not a measurement. It is a name from a mined
/// list, arriving through a side channel, and it would have been recorded as a probe
/// result - precisely the import D242 refuses, laundered into the strongest provenance this
/// project has (D246).
///
/// An existence fact therefore carries an [`Oracle`] like any other: `Measured` when the
/// operator asserts the run was on **the target**, `Assumed` otherwise. Only the first may
/// source a name.
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
    /// `None` for a `resolve` record, which does not have the field. **Not an empty
    /// string**: "reached in a way this record did not say" and "reached by a means named
    /// as nothing" are different facts, and a reader cannot tell them apart once they are
    /// spelled the same (D245).
    pub availability: Option<String>,
    /// Where it resolved to, when the record carried it. `None` for a `sym` record.
    pub address: Option<String>,
    /// How much this existence fact is worth, given what it ran on.
    ///
    /// `Oracle::Measured` only from the target. Anything else is `Oracle::Assumed` - not
    /// worthless, but never a source for a name (D246).
    pub known_by: Oracle,
}

impl SymbolFact {
    /// Whether this fact may be used as the source of a name in the symbol database.
    ///
    /// The one question the naming rule turns on, answered in one place so no caller has to
    /// re-derive it - which is how two counters came to disagree (D239, D242, D246).
    pub fn may_source_a_name(&self) -> bool {
        self.present && self.known_by == Oracle::Measured
    }
}

impl Transcript {
    /// Every symbol the run established the existence of, graded by what it ran on.
    ///
    /// **Takes the origin rather than grading nothing.** This used to return facts with no
    /// grade at all, on the reasoning that existence is a property of an interface and so
    /// survives a stand-in. What that missed is where a stand-in's symbol table comes from:
    /// name lists mined out of other emulators. A `present` from one is that list speaking,
    /// and returning it ungraded made it indistinguishable from a console answering (D246).
    pub fn symbols(&self, origin: &Origin) -> Vec<SymbolFact> {
        // The same demotion the behaviour grading applies, for the same reason: the
        // question is not whether the silicon was real but whether it was the thing being
        // emulated. A Steam Deck is real and is not it; an emulator is neither.
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
                // The probe's by-name census. Same existence fact, carrying where it
                // landed instead of how it is reached - and worth more to this project
                // than any other record it emits, because it answers for symbols no title
                // imports, which a collision search can never reach (D245).
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
/// # Why a per-section view rather than one total
///
/// A single tally says how much was checked and nothing about *what is understood*. Ninety
/// passes spread thinly across every area and ninety concentrated in one are the same
/// number and completely different situations - and the second is the one that means a
/// subsystem can be relied on.
///
/// The sections carry a `purpose` line saying what each is establishing, which is what
/// turns a count into an answer. It is carried here rather than summarised, because the
/// probe's own words about what a section proves are worth more than anything this side
/// would write.
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
    /// A skip is **not** green. It is a check that did not run, so the section did not
    /// establish what it claims to establish, and rounding a skip up is how a subsystem
    /// gets relied on for something nobody tested.
    pub const fn is_wholly_green(&self) -> bool {
        self.total() > 0 && self.pass == self.total()
    }
}

impl Transcript {
    /// Each section with its tally, in running order.
    ///
    /// A section with no tally, or a tally naming no section, still appears. Both are
    /// incomplete reports rather than absent ones, and dropping either would quietly
    /// shrink the denominator - which flatters the result in exactly the direction nobody
    /// should be flattered.
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
/// # Why a partial read is still returned
///
/// A read that dies part way through has still established what it read before it died.
/// Those bytes are evidence; discarding them because the command did not complete would
/// throw away the only thing the run produced - and on a target where a fatal address is
/// the normal case, that is most runs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Memory {
    /// Address the read started at, where the records said.
    pub address: Option<u64>,
    /// The bytes, in offset order.
    pub bytes: Vec<u8>,
    /// Runs whose hexadecimal could not be decoded.
    ///
    /// Kept as a count rather than dropped silently. A run this cannot read is a gap in
    /// the middle of a buffer, and a caller reassembling one needs to know it is there
    /// rather than receiving a shorter buffer that looks complete.
    pub undecodable: usize,
}

/// Reads a hexadecimal run into bytes.
///
/// An odd number of digits is **refused** rather than rounded. Half a byte is not a byte,
/// and guessing which half was meant would put a value in a buffer that nothing observed.
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
                // `read/0x8003f510` - one `0x`, and the address after it.
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
/// # Why this is not a boolean, and not folded into the value
///
/// All three states can carry the value `unknown`, and they mean entirely different things.
/// A consumer that collapses them keeps the least useful part of the record:
///
/// - **`known`** - read through a confirmed signature. A real reading.
/// - **`unconfirmed`** - the query resolves, but the probe has no confirmed signature to
///   call it through yet. The probe's unfinished wiring, not a platform gap.
/// - **`absent`** - no such query here. The platform gap.
///
/// "This emulator does not implement it", "obSCEne has not wired it up", and "here is the
/// number" are three different findings, and only one of them is anybody's bug.
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
    /// **Deliberately not resolved to any of the others.** Reading an unrecognised state as
    /// `absent` would blame the platform for something it may well do; reading it as `known`
    /// would treat an unknown confidence as a reading. Neither is available, so it stays
    /// unrecognised and a consumer has to decide in the open.
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
/// # Never machine identity
///
/// These are **observations, not provenance**. Inside an emulator every one of them answers
/// as the emulator chooses - `memory|known|441M` is that emulator's number wearing the
/// target's badge, which is the same trap as a self-reported firmware version.
///
/// So nothing here reaches [`Origin`]: the machine a grade rests on is asserted by the
/// operator and by nothing else. These are for display, and the type keeps them separate so
/// that staying separate is structural rather than a habit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelfReport {
    /// What the field is called - `memory`, `vram`, `generation`, and so on.
    pub field: String,
    /// How firmly the target established it.
    pub confidence: Confidence,
    /// The value, verbatim. Frequently `unknown`, which is why the confidence matters.
    pub value: String,
}

impl Transcript {
    /// What the target said about itself.
    ///
    /// Ordered as it arrived, because the block is a status readout and its order is the
    /// probe's editorial choice about what matters most.
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
/// # The rule, and why it keys on the return kind
///
/// A value asked of the console under the probe's state rather than the guest's is very
/// likely right and is not certainly right. That is still better than a stub, which is
/// certainly wrong - so it is returned, recorded, and labelled.
///
/// **Except for a handle.** A function returning a handle or a pointer hands back a value
/// from the *console's* address space, meaningless in this one. The guest dereferences it
/// and dies somewhere unrelated hours later: certainly wrong, and it looks right, which is
/// the one failure this project has no cheap detector for.
///
/// The first version of this rule asked whether a function was *pure*. That is a
/// per-function judgement nobody makes reliably in advance and it fails silently. The
/// return kind is a property already recorded, already load-bearing - it is why an error
/// code is correct for a status function and a wild pointer for a handle one - and it is
/// checkable. (D125, D225)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Use {
    /// Hand it to the guest, and record it.
    Return,
    /// Record it, and do not hand it over.
    ///
    /// Not a failure. The answer is evidence about the function and is kept as such; what
    /// is withheld is only its use *as this guest's return value*.
    RecordOnly,
}

/// Whether a live answer may be handed to the guest.
///
/// `None` for an unknown return kind, which is treated as [`Use::RecordOnly`] by any caller
/// that follows the rule: not knowing what kind of value a function returns is precisely
/// when passing one through is most dangerous.
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
/// # What this carries that a bare value does not
///
/// The arguments it was asked with, and the fact that the state was the probe's rather than
/// the guest's. Both belong with the number: a return value without its arguments is not a
/// fact about a function, and `measured` without the divergence reads as fully trustworthy
/// to whoever finds it in six months.
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
    /// # Why the caveat is an assumption rather than a weaker grade
    ///
    /// The measurement is real: a console executed that function with those arguments and
    /// returned that value. What is *not* established is that the guest would have got the
    /// same answer, because the probe's process has not done what the guest did.
    ///
    /// A weaker grade would lose the first half; a bare `measured` would lose the second.
    /// So the grade stays `measured` and the divergence travels beside it as a stated
    /// assumption - which is also what makes it a **worklist item** rather than a footnote,
    /// since the assumption count is a thing this project ranks and retires.
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
            // A non-answer is still a finding, and it is emphatically not a value. What is
            // recorded is that asking killed the probe, which is a fact about the function
            // worth having and is the shape of most first attempts.
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
/// # Why a count by grade is the useful summary
///
/// The question a reader has before trusting any of this is not "how many checks ran" but
/// "how many of these are facts". A run of four hundred results where every one is
/// `assumed` has established nothing about the platform, and a summary reporting four
/// hundred results would be describing effort rather than evidence.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Established {
    /// Results whose grade is a measurement of the target.
    pub measured: usize,
    /// Results settled by a published standard or documentation.
    pub published: usize,
    /// Results that are somebody's reasoning, including measurements demoted because they
    /// were taken on a stand-in.
    pub assumed: usize,
    /// Results carrying no grade at all - written before the field existed.
    pub ungraded: usize,
    /// Commands acknowledged and never answered.
    pub unanswered: usize,
    /// Commands that ended the process, timed out, or vanished.
    ///
    /// Counted separately from [`Established::unanswered`] because they are a different
    /// observation: these were *reported* as not answering, where an unanswered command is
    /// one the transcript simply stops after. Both mean no result; only one was written
    /// down by something that saw it happen.
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
    /// The session matters: the same `hardware` result is a measurement of the target or an
    /// approximation of it depending on what produced it, and the count has to reflect that
    /// or it would report a stand-in run as having settled things it cannot settle.
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

        // Records from inside a command and records standing on their own are the same
        // evidence. A live session produces the first; a committed corpus is entirely the
        // second.
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
                    Oracle::Published => summary.published += 1,
                    Oracle::GuestObserved | Oracle::Assumed => summary.assumed += 1,
                }
            }
        }
        summary
    }
}
