//! The shim-to-worker protocol: messages as data.
//!
//! Guest code executes in a child process (D032), so the shims and the worker talk.
//! **This crate defines what they say, not how it travels.** Messages are plain serde
//! types with no handles, no references into loaded modules, and no lifetimes - the
//! constraint D035 places on the service layer, made structural here.
//!
//! [`codec`] holds one transport - newline-delimited JSON over a pipe - and is
//! deliberately separable. Changing the channel should not move the protocol.
//!
//! # Versioning
//!
//! Worker mode is self-reinvocation of the same executable (D033), so version skew is
//! close to impossible by construction. [`PROTOCOL_VERSION`] is cheap insurance
//! anyway: a mismatch produces a clear refusal rather than a subtle misparse, and it
//! documents that the wire format is a contract rather than an implementation detail.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub mod codec;

/// Wire format version. Bump on any incompatible change to [`Request`] or [`Event`].
pub const PROTOCOL_VERSION: u32 = 3;

/// How far a run got. Ordered, so "furthest point reached" is a comparison.
///
/// This is the coarse progress axis a run report leads with - a phase regression
/// between runs is the clearest signal that a change made things worse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// Nothing has happened yet.
    Start,
    /// The container was opened and its wrapper parsed.
    ContainerParsed,
    /// Imports were enumerated.
    ImportsResolved,
    /// Address space was reserved.
    Mapped,
    /// Relocations applied and TLS set up.
    Linked,
    /// Control was handed to the guest entry point.
    Entered,
    /// The guest presented at least one frame.
    Presented,
}

/// What a shim asks the worker to do.
///
/// `PartialEq` but **not `Eq`**, which [`Event`] already was: a stick position is a real
/// number and there is no total ordering on those. Comparing requests in a test is what the
/// derive is for, and that needs only the partial one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "request", rename_all = "snake_case")]
pub enum Request {
    /// Opening handshake. Always first.
    Hello {
        /// Version the shim speaks.
        protocol_version: u32,
    },
    /// Inspect a container without executing it.
    Survey {
        /// Path to the guest executable.
        path: PathBuf,
    },
    /// Load and execute.
    Run {
        /// Path to the guest executable.
        path: PathBuf,
        /// Symbol database to name imports with, if any.
        ///
        /// Passed explicitly rather than resolved by the worker from a convention.
        /// The worker is a separate process, so it inherits nothing from the shim that
        /// spawned it - and a call trace that reports hashes because a default path
        /// happened not to exist is the kind of quiet degradation that wastes an
        /// afternoon.
        symbols_db: Option<PathBuf>,
        /// Seconds of guest execution to allow, or `None` for no limit.
        ///
        /// A guest with every import unimplemented can settle into a loop waiting for
        /// something that will never happen - one commercial executable here ran for ten
        /// minutes without faulting. A limit turns that from a hang into a report.
        limit_seconds: Option<u64>,
        /// Imports the guest may call before it is stopped, or `None` for no budget.
        ///
        /// The deterministic counterpart to `limit_seconds`. That one fixes the duration
        /// and lets the call count vary by 13% between identical runs; this fixes the
        /// count. Both travel, because a guest that stops calling imports never reaches a
        /// budget and a guest in a tight import loop wastes most of a clock (D238).
        call_budget: Option<u64>,
    },
    /// Carry a shell action into a running session.
    ///
    /// **The one request that has to be honoured while a run is in flight.** Everything
    /// else here is answered between runs, because the worker is inside the guest for the
    /// whole of one - which is precisely the moment somebody presses the shell button.
    ///
    /// So it is answered on the reading thread rather than the main loop, and it produces
    /// no event in reply, which is what keeps a second writer off the output stream.
    ///
    /// The payload is `orbistoun-shell`'s own type rather than a copy of it: two enums
    /// meaning the same thing drift, and the drift shows up as a button doing the wrong
    /// thing.
    Shell {
        /// What was asked for.
        action: orbistoun_shell::Request,
    },
    /// What the pads are doing, as **a title is allowed to see them**.
    ///
    /// The window owns input, because the system's own button has to be seen by something
    /// that is not the title (D326). So the arbitration happens before this is sent: the
    /// shell's button is stripped and a neutral pad travels while the shell has focus. What
    /// arrives here is already what the guest may know about.
    ///
    /// Answered on the reading thread like [`Self::Shell`], and for the same reason - the
    /// handling loop is inside the guest for the whole of a run, which is exactly when
    /// somebody is pressing something.
    ///
    /// **Sent only when it changes.** Input is a level rather than a stream, so an unchanged
    /// pad needs no message and a backlog of stale ones would be worse than none (D345).
    Input {
        /// One state per configured port, in port order.
        pads: Vec<orbistoun_input::PadState>,
    },
    /// Stop cleanly.
    Shutdown,
}

/// What the worker says back.
///
/// A stream, not a reply: one request produces zero or more events, ending in a
/// terminal one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    /// Handshake response.
    Hello {
        /// Version the worker speaks.
        protocol_version: u32,
        /// Build identity, so a report can record exactly what produced it.
        worker_version: String,
    },
    /// Progress along the phase axis.
    Reached {
        /// Phase now completed.
        phase: Phase,
    },
    /// A survey finished.
    SurveyComplete(SurveySummary),
    /// The run ended.
    Terminated {
        /// How it ended.
        outcome: Outcome,
        /// Furthest phase reached.
        reached: Phase,
    },
    /// Something went wrong that is not a guest failure.
    Failed {
        /// Human-readable cause.
        error: String,
    },
}

/// How a run ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Outcome {
    /// The guest returned from its entry point.
    Exited {
        /// Guest exit code.
        code: i32,
    },
    /// The guest faulted.
    Crashed {
        /// What kind of fault.
        signal: String,
    },
    /// orbistoun stopped it - a stub it could not answer, or a limit.
    Halted {
        /// Why.
        reason: String,
    },
    /// The shim asked it to stop.
    Cancelled,
}

/// Structural facts about a container, without executing or fully parsing it.
///
/// The first genuinely useful output of the container layer: enough to tell whether a
/// file is the shape we expect, and to say precisely how it differs when it is not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainerInfo {
    /// Whether a wrapper was found, and which generation.
    pub wrapper: WrapperInfo,
    /// Byte offset of the inner ELF image.
    pub elf_offset: usize,
    /// Guest entry point.
    pub entry: u64,
    /// ELF `e_type`. Vendor values sit in the OS-specific range.
    pub e_type: u16,
    /// ELF `e_machine`.
    pub machine: u16,
    /// ELF `EI_OSABI`. 9 is FreeBSD, which is what real material carries.
    pub osabi: u8,
    /// Program header entries.
    pub program_headers: usize,
    /// How many of those are vendor segments.
    pub vendor_segments: usize,
    /// Program-header indices whose bytes the wrapper's descriptor table locates.
    ///
    /// Headers absent from this list are not missing - several describe regions
    /// *inside* another header's data rather than having their own descriptor.
    pub mapped_segments: Vec<usize>,
    /// The process parameter block, if the container carries one.
    ///
    /// `None` for a module or a bare object with no `PT_SCE_PROCPARAM` segment.
    #[serde(default)]
    pub proc_param: Option<ProcParamInfo>,
}

/// The process parameter block a launching title carries, read as far as cited offsets allow.
///
/// A console loader reads this before the first guest instruction to learn the SDK version and,
/// through the memory-parameter block, the flexible-memory budget the title asked for. Only
/// fields at cited offsets are reported; the memory-parameter block's *contents* are surfaced
/// raw rather than interpreted, because the layout inside it is not established from a citable
/// source (D442).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcParamInfo {
    /// The size the block states, its one mandatory field.
    pub size: u64,
    /// Whether the magic reads `"ORBI"`; a loader ignores a block that fails this.
    pub magic_ok: bool,
    /// The entry count the block states. A real launching title states five.
    pub entry_count: u32,
    /// The SDK version field at `+0x10`.
    pub sdk_version: u32,
    /// The libc-parameter pointer at `+0x38`, as a guest virtual address (`0` = absent).
    ///
    /// Reported alongside the memory-parameter pointer because the two disambiguate each other:
    /// obSCEne's hardware fault (D219) says a launching title's loader writes through this
    /// pointer, so a non-null value here confirms the pointer offsets are read correctly even
    /// when the memory-parameter slot beside it is null.
    pub libc_param_vaddr: u64,
    /// The memory-parameter pointer at `+0x40`, as a guest virtual address (`0` = absent).
    pub mem_param_vaddr: u64,
    /// The third pointer at `+0x48`, as a guest virtual address (`0` = absent).
    pub third_param_vaddr: u64,
    /// The memory-parameter block's stated size (its first word), if the pointer resolves to
    /// bytes in the file.
    pub mem_param_size: Option<u64>,
    /// The non-zero 64-bit words the memory-parameter block carries past its size field, as
    /// `(offset, value)`. Reported, not interpreted: this is the oracle a future flexible-memory
    /// implementation confirms a cited layout against, not a layout in itself.
    pub mem_param_nonzero: Vec<(u64, u64)>,
}

/// Which wrapper, if any, a container was found inside.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WrapperInfo {
    /// A bare ELF with no wrapper.
    None,
    /// A vendor container.
    ///
    /// **Both generations, told apart rather than flattened.** They parse identically -
    /// same header layout, same segment descriptors - but a title built for the previous
    /// console is a different emulation problem, and a report that cannot say which it
    /// read is hiding the most useful fact about it (D176).
    Wrapped {
        /// Whether this is the previous console's container.
        #[serde(default)]
        previous_generation: bool,
        /// Number of segment descriptors, which is what the ELF offset derives from.
        segment_count: u16,
        /// The size the header states. Not the file length - see the wrapper docs.
        stated_size: u64,
    },
}

/// One loadable segment and what happened when the address space was reserved for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SegmentPlacement {
    /// Program header index.
    pub index: usize,
    /// Guest virtual address the segment demands.
    pub vaddr: u64,
    /// Bytes it occupies in memory.
    pub memsz: u64,
    /// Readable.
    pub read: bool,
    /// Writable.
    pub write: bool,
    /// Executable.
    pub execute: bool,
    /// Reserved for per-segment placement detail once segments are populated
    /// individually. The span reservation is what currently succeeds or fails.
    pub failure: Option<String>,
}

/// The result of reserving a module's address space without executing it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoadLayout {
    /// Base the module was placed at.
    pub base: u64,
    /// Start of the contiguous span reserved for the whole module, page-aligned.
    pub span_base: u64,
    /// Length of that span.
    pub span_len: u64,
    /// Every loadable segment, in program-header order.
    pub segments: Vec<SegmentPlacement>,
    /// `None` if the span was reserved, otherwise why it could not be.
    ///
    /// This is the interesting failure: it means the address a module was linked for
    /// is unavailable in this process - exactly the class of problem that motivated
    /// executing in a child process (D032).
    pub reservation_failure: Option<String>,
}

impl LoadLayout {
    /// Whether the module's span was successfully placed.
    pub fn placed(&self) -> bool {
        self.reservation_failure.is_none()
    }

    /// Total bytes the module demands.
    pub fn total_bytes(&self) -> u64 {
        self.segments.iter().map(|s| s.memsz).sum()
    }
}

/// Whether an import names code or data.
///
/// # Why a report has to say
///
/// Interception writes an address into a relocation slot, and for code that address is a
/// thunk. **For data the same answer is wrong in a way that looks right**: a guest
/// importing `__stderrp` loads the slot and dereferences what it found, so a thunk becomes
/// x86 instruction bytes read as a pointer, and the guest carries on. Nothing reports a
/// problem until something unrelated breaks much later.
///
/// Declared here rather than borrowed from `orbistoun-elf` because this crate takes serde
/// and nothing else, deliberately, and a wire type reaching into a parser for its
/// vocabulary is how that stays true only until someone is in a hurry (D307).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ImportKind {
    /// Code. A thunk is the right answer.
    Function,
    /// Data. A thunk is not an answer at all.
    Object,
    /// The symbol table did not say, which is a fact rather than a default.
    #[default]
    Unspecified,
}

/// One import a guest module asks for, and whether orbistoun can answer it.
///
/// Lives here rather than in the service layer because it is wire data: the service
/// returns it, the worker sends it, and a run report embeds it. One shape, not three.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ImportRecord {
    /// The hash the module imports by.
    pub nid: u64,
    /// Library, where the import table named one.
    pub library: Option<String>,
    /// Symbol name, where the registry or a symbol database knows it.
    pub symbol: Option<String>,
    /// Whether orbistoun has this function declared at all.
    pub known: bool,
    /// Whether the guest wants code or data in this slot.
    #[serde(default)]
    pub kind: ImportKind,
}

/// What a module needs, determined without executing it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurveySummary {
    /// Guest entry point address.
    pub entry: u64,
    /// Every import, in table order.
    pub imports: Vec<ImportRecord>,
}

impl SurveySummary {
    /// Total imports seen.
    pub fn total(&self) -> usize {
        self.imports.len()
    }

    /// How many orbistoun cannot answer - the number to drive down, and the honest
    /// headline for a compatibility report.
    pub fn unresolved(&self) -> usize {
        self.imports.iter().filter(|i| !i.known).count()
    }

    /// Unresolved imports only, in first-touch order.
    ///
    /// First-touch matters as much as frequency: the *first* unmet need is usually the
    /// cause, and everything after it is cascade.
    pub fn unresolved_imports(&self) -> impl Iterator<Item = &ImportRecord> {
        self.imports.iter().filter(|i| !i.known)
    }
}

/// Why a handshake was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VersionMismatch {
    /// Version the peer offered.
    pub theirs: u32,
    /// Version this build speaks.
    pub ours: u32,
}

impl std::fmt::Display for VersionMismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "protocol version mismatch: peer speaks {}, this build speaks {}",
            self.theirs, self.ours
        )
    }
}

impl std::error::Error for VersionMismatch {}

/// Checks a peer's version against this build's.
///
/// Refuses loudly rather than attempting a best-effort parse: a subtly misread
/// message stream is far harder to diagnose than an outright refusal.
pub const fn check_version(theirs: u32) -> Result<(), VersionMismatch> {
    if theirs == PROTOCOL_VERSION {
        Ok(())
    } else {
        Err(VersionMismatch {
            theirs,
            ours: PROTOCOL_VERSION,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Event, Outcome, PROTOCOL_VERSION, Phase, Request, check_version};
    use std::path::PathBuf;

    #[test]
    fn requests_round_trip() {
        for r in [
            Request::Hello {
                protocol_version: PROTOCOL_VERSION,
            },
            Request::Survey {
                path: PathBuf::from("/titles/x/eboot.bin"),
            },
            Request::Shutdown,
        ] {
            let json = serde_json::to_string(&r).expect("serialise");
            let back: Request = serde_json::from_str(&json).expect("deserialise");
            assert_eq!(back, r);
        }
    }

    #[test]
    fn events_round_trip() {
        let e = Event::Terminated {
            outcome: Outcome::Halted {
                reason: "unimplemented import".to_owned(),
            },
            reached: Phase::Entered,
        };
        let json = serde_json::to_string(&e).expect("serialise");
        assert_eq!(
            serde_json::from_str::<Event>(&json).expect("deserialise"),
            e
        );
    }

    #[test]
    fn messages_are_tagged_so_the_variant_survives_an_unknown_peer() {
        let json = serde_json::to_string(&Request::Shutdown).expect("serialise");
        assert!(json.contains("\"request\""), "got {json}");
        let json = serde_json::to_string(&Event::Reached {
            phase: Phase::Mapped,
        })
        .expect("serialise");
        assert!(json.contains("\"event\""), "got {json}");
    }

    #[test]
    fn phases_are_ordered_so_furthest_reached_is_a_comparison() {
        // A phase regression between runs is the clearest "that change made it worse"
        // signal the report can carry, and it only works if this ordering holds.
        assert!(Phase::Start < Phase::ContainerParsed);
        assert!(Phase::ContainerParsed < Phase::ImportsResolved);
        assert!(Phase::ImportsResolved < Phase::Mapped);
        assert!(Phase::Mapped < Phase::Linked);
        assert!(Phase::Linked < Phase::Entered);
        assert!(Phase::Entered < Phase::Presented);
    }

    #[test]
    fn version_check_refuses_rather_than_guessing() {
        assert!(check_version(PROTOCOL_VERSION).is_ok());
        let err = check_version(PROTOCOL_VERSION + 1).expect_err("must refuse");
        assert_eq!(err.ours, PROTOCOL_VERSION);
        assert!(err.to_string().contains("mismatch"));
    }

    #[test]
    fn no_message_carries_a_borrowed_value() {
        // Compile-time assertion of D035: everything crossing the boundary must own
        // its data, or it cannot cross a process boundary at all.
        const fn assert_owned<T: 'static>() {}
        assert_owned::<Request>();
        assert_owned::<Event>();
        assert_owned::<Outcome>();
    }
}
