//! The shim-to-worker protocol: messages as data.
//!
//! Guest code executes in a child process, so the shims and the worker exchange messages.
//! This crate defines what they say, not how it travels: plain serde types with no handles, no
//! references into loaded modules and no lifetimes (D035). [`codec`] holds one separable transport.
//! Worker mode reinvokes the same executable, so version skew is unlikely;
//! [`PROTOCOL_VERSION`] still turns a mismatch into a clear refusal.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub mod codec;

/// Wire format version. Bump on any incompatible change to [`Request`] or [`Event`], such as a new
/// variant a peer would reject.
pub const PROTOCOL_VERSION: u32 = 4;

/// How far a run got. Ordered, so "furthest point reached" is a comparison, and a phase regression
/// between runs is visible.
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
/// `PartialEq` but not `Eq`: a stick position is a floating-point number.
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
        /// Passed explicitly because the worker is a separate process and inherits nothing from the
        /// shim, so a default path resolved there could silently differ.
        symbols_db: Option<PathBuf>,
        /// Seconds of guest execution to allow, or `None` for no limit. A guest waiting on
        /// unimplemented imports can loop without faulting, and a limit turns that into a report.
        limit_seconds: Option<u64>,
        /// Imports the guest may call before it is stopped, or `None` for no budget.
        ///
        /// The deterministic counterpart to `limit_seconds`, which fixes the duration and lets the
        /// call count vary. Both travel, because a guest that stops calling imports never reaches a
        /// budget (D238).
        call_budget: Option<u64>,
        /// A pad script this run plays, taking precedence over one named in `config.toml` (D721),
        /// so a test names its input in its own command. `None` plays what the configuration says.
        #[serde(default)]
        input_script: Option<PathBuf>,
        /// Capture what the title reads from its pad into this script file from its entry (D721).
        /// `None` captures nothing.
        #[serde(default)]
        capture_input: Option<PathBuf>,
        /// Run the module as a staged title, as if it lay under the library's `data/homebrew` tree:
        /// its `/app0` is writable through an overlay (D722). A module that lies there is staged
        /// regardless; this is for a loose developer build.
        #[serde(default)]
        staged: bool,
        /// Replace the title's stored link plan with this run's, whatever the stored one's key,
        /// and report what differed (D724).
        #[serde(default)]
        relink: bool,
    },
    /// Link a title and store its plan without entering it (D724).
    Link {
        /// Path to the guest executable.
        path: PathBuf,
        /// Symbol database to name imports with, as for [`Self::Run`].
        symbols_db: Option<PathBuf>,
        /// Replace the stored plan whatever its key, as `Run::relink` does.
        #[serde(default)]
        relink: bool,
    },
    /// Carry a shell action into a running session.
    ///
    /// The worker is inside the guest for the whole of a run, so this is answered on the reading
    /// thread rather than the main loop, and produces no reply, which keeps a second writer off the
    /// output stream. The payload is `orbistoun-shell`'s own type.
    Shell {
        /// What was asked for.
        action: orbistoun_shell::Request,
    },
    /// What the pads are doing, as a title is allowed to see them.
    ///
    /// The window owns input (D326), so the shell's button is already stripped, and a neutral pad
    /// travels while the shell has focus. Answered on the reading thread like [`Self::Shell`]. Sent
    /// only when it changes, because input is a level.
    Input {
        /// One state per configured port, in port order.
        pads: Vec<orbistoun_input::PadState>,
    },
    /// Starts capturing what the title reads from its pad into `to`, or stops with `None` (D721).
    /// Sent only when somebody asks.
    ///
    /// Answered on the reading thread like [`Self::Input`], with no reply.
    CaptureInput {
        /// The script file to write, or `None` to stop.
        to: Option<PathBuf>,
    },
    /// Starts playing the pad script `script` from now, or stops playing with `None` (D721).
    ///
    /// Answered on the reading thread like [`Self::Input`], with no reply.
    PlayInput {
        /// The script to play, or `None` to stop.
        script: Option<PathBuf>,
    },
    /// Stop cleanly.
    Shutdown,
}

/// What the worker says back.
///
/// A stream, not a reply: one request produces zero or more events, ending in a terminal one.
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
    /// A title was linked without being entered.
    Linked(LinkSummary),
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
    /// A rendered frame is ready, its bytes in a shared region this names.
    ///
    /// The descriptor, never the pixels (D695): the bytes cross in the region the worker wrote, and
    /// this carries dimensions, format, a sequence number and the region's name. `region` is a bare
    /// name the shim resolves against the frames directory, like a trace file's name.
    Frame {
        /// Width in pixels.
        width: u32,
        /// Height in pixels.
        height: u32,
        /// How the bytes are laid out.
        format: FrameFormat,
        /// Which frame this is, so the shim can order arrivals and drop a stale one.
        sequence: u64,
        /// The name of the region holding the bytes, resolved against the frames directory.
        region: String,
    },
    /// The guest asked the system to start another title, as a launcher does.
    ///
    /// The worker runs one guest, so the front end ends this run and launches the id from its
    /// library.
    LaunchApp {
        /// The title id asked for, as the guest passed it.
        title_id: String,
    },
    /// Where the last stretch of a running title's time went, streamed about once a second for a
    /// front end to show over the picture.
    Perf(PerfReport),
}

/// The [`PerfReport::phases`] entry that contains the others up to the write-back: a whole graphics
/// submit. Reported beside them, never summed with them.
pub const SUBMIT_TOTAL_PHASE: &str = "submit (total)";

/// The [`PerfReport::phases`] entry for the device's busy time by its own clock. It runs alongside
/// the host's phases, so it is reported as its own share and never summed.
pub const GPU_BUSY_PHASE: &str = "gpu busy";

/// One stretch of a running title's time: how long, what happened in it, and which parts of
/// presenting a frame took how much of it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PerfReport {
    /// How long the stretch was, in milliseconds.
    pub window_ms: f64,
    /// Flips the guest submitted in it.
    pub flips: u64,
    /// Submissions whose draws were carried out.
    pub submissions: u64,
    /// Draws carried out.
    pub draws: u64,
    /// Milliseconds spent in each measured phase, by name, in the order a frame passes through them.
    /// What is left of the window is the guest's own time and whatever nothing measures.
    pub phases: Vec<(String, f64)>,
}

/// How a frame's bytes are laid out in its region.
///
/// The detile path produces 32-bpp RGBA (`orbistoun-gpu`'s `tiling.rs`). An enum so a second layout
/// is a variant the shim must handle rather than a reinterpretation of the same bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrameFormat {
    /// Eight bits per channel, `R G B A` in byte order - the 32-bpp the detile produces.
    Rgba8,
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

/// Structural facts about a container, without executing or fully parsing it: enough to say whether
/// a file has the expected shape and how it differs when it does not.
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
    /// A header absent from this list is not missing: several describe regions inside another
    /// header's data.
    pub mapped_segments: Vec<usize>,
    /// The process parameter block, if the container carries one.
    ///
    /// `None` for a module or a bare object with no `PT_SCE_PROCPARAM` segment.
    #[serde(default)]
    pub proc_param: Option<ProcParamInfo>,
}

/// The process parameter block a launching title carries, read as far as cited offsets allow.
///
/// A platform loader reads it before the first guest instruction for the SDK version and, through
/// the memory-parameter block, the flexible-memory budget. Only fields at cited offsets are
/// reported; the memory-parameter block's contents are surfaced raw because their layout has no
/// citable source.
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
    /// Reported beside the memory-parameter pointer: a launching title's loader writes through this
    /// pointer, so a non-null value confirms the pointer offsets are read correctly even when the
    /// memory-parameter slot is null.
    pub libc_param_vaddr: u64,
    /// The memory-parameter pointer at `+0x40`, as a guest virtual address (`0` = absent).
    pub mem_param_vaddr: u64,
    /// The third pointer at `+0x48`, as a guest virtual address (`0` = absent).
    pub third_param_vaddr: u64,
    /// The memory-parameter block's stated size (its first word), if the pointer resolves to
    /// bytes in the file.
    pub mem_param_size: Option<u64>,
    /// The non-zero 64-bit words the memory-parameter block carries past its size field, as
    /// `(offset, value)`. Reported, not interpreted: an oracle for a cited layout, not a layout.
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
    /// Both generations parse identically, but a title built for the previous generation is a
    /// different emulation problem, so the report says which it read.
    Wrapped {
        /// Whether this is the previous generation's container.
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
    /// Reserved for per-segment placement detail. The span reservation is what succeeds or fails.
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
    /// `None` if the span was reserved, otherwise why it could not be: the address the module was
    /// linked for is unavailable in this process, the case a child process exists for (D032).
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

/// Whether an import names code or data (D307).
///
/// For code, interception writes a thunk address into the relocation slot. For data that answer is
/// wrong silently: a guest importing `__stderrp` dereferences the slot, reads instruction bytes as
/// a pointer and carries on. Declared here rather than borrowed from `orbistoun-elf` because this
/// crate depends on serde and nothing else.
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
/// Wire data: the service returns it, the worker sends it, and a run report embeds it.
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

/// One symbol a module provides.
///
/// Wire data, like [`ImportRecord`]. An export is a NID plus where in the module it lives.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ExportRecord {
    /// The hash an importer asks for it by.
    pub nid: u64,
    /// The name, where the module spelled one or a symbol database knows it.
    pub symbol: Option<String>,
    /// Where it lives, as an offset from the module's own base, not an address: nothing has been
    /// placed when this is read.
    pub offset: u64,
    /// Code or data. Binding data as a function hands the guest a thunk where it expects a value.
    #[serde(default)]
    pub kind: ImportKind,
}

/// What linking a title decided, and how that stood against the plan stored for it (D724).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkSummary {
    /// The plan's digest.
    pub digest: String,
    /// Modules linked, the executable included.
    pub modules: usize,
    /// Relocation writes across every module.
    pub writes: usize,
    /// `new`, `match` or `mismatch` against the stored plan, or empty when none could be kept.
    pub stored: String,
    /// What differed from the stored plan, on a mismatch or a relink, in words.
    pub differs: Vec<String>,
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

    /// How many orbistoun cannot answer: the headline for a compatibility report.
    pub fn unresolved(&self) -> usize {
        self.imports.iter().filter(|i| !i.known).count()
    }

    /// Unresolved imports only, in first-touch order. The first unmet need is usually the cause and
    /// the rest are cascade.
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
/// Refuses rather than attempting a best-effort parse of a stream it may misread.
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
    use super::{Event, FrameFormat, Outcome, PROTOCOL_VERSION, Phase, Request, check_version};
    use std::path::PathBuf;

    /// Every request survives a serialise and parse.
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

    /// Every event survives a serialise and parse.
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

    /// Messages are tagged, so the variant survives an unknown peer.
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

    /// A frame event names its region and carries no pointer.
    #[test]
    fn a_frame_event_names_its_region_and_carries_no_pointer() {
        let json = serde_json::to_string(&Event::Frame {
            width: 1920,
            height: 1080,
            format: FrameFormat::Rgba8,
            sequence: 7,
            region: "frame-7.bin".to_owned(),
        })
        .expect("serialise");
        assert!(
            json.contains("\"event\""),
            "tagged like every event: {json}"
        );
        assert!(json.contains("frame-7.bin"), "names its region: {json}");
        // A protocol message carries the bulk's name, never a pointer or a handle to it (D035).
        for forbidden in ["0x", "ptr", "handle", "addr"] {
            assert!(!json.contains(forbidden), "{forbidden} leaked into {json}");
        }
    }

    /// Phases are ordered, so furthest reached is a comparison.
    #[test]
    fn phases_are_ordered_so_furthest_reached_is_a_comparison() {
        // A phase regression between runs is only visible if this ordering holds.
        assert!(Phase::Start < Phase::ContainerParsed);
        assert!(Phase::ContainerParsed < Phase::ImportsResolved);
        assert!(Phase::ImportsResolved < Phase::Mapped);
        assert!(Phase::Mapped < Phase::Linked);
        assert!(Phase::Linked < Phase::Entered);
        assert!(Phase::Entered < Phase::Presented);
    }

    /// A version mismatch is refused.
    #[test]
    fn version_check_refuses_rather_than_guessing() {
        assert!(check_version(PROTOCOL_VERSION).is_ok());
        let err = check_version(PROTOCOL_VERSION + 1).expect_err("must refuse");
        assert_eq!(err.ours, PROTOCOL_VERSION);
        assert!(err.to_string().contains("mismatch"));
    }

    /// No message carries a borrowed value.
    #[test]
    fn no_message_carries_a_borrowed_value() {
        // Everything crossing the boundary owns its data, or it cannot cross a process boundary
        // (D035).
        const fn assert_owned<T: 'static>() {}
        assert_owned::<Request>();
        assert_owned::<Event>();
        assert_owned::<Outcome>();
    }
}
