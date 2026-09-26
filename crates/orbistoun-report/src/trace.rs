//! What a guest asked for, and how one run compares with the last.
//!
//! The trace types and the pure comparison live here, below both shims, so the CLI and the
//! GUI share one implementation (D034). The producing side (the fault handler, the region
//! table, the allocation-free line writer) stays in `orbistoun-worker`.

/// What a guest asked for, in the order of how much it wanted it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CallTrace {
    /// Which module was run.
    pub module: String,
    /// How far it got.
    pub reached: String,
    /// Total calls through any stub.
    pub total_calls: u64,
    /// Distinct imports called.
    pub distinct: usize,
    /// Frames the guest handed to the output layer, counted by the port that accepted them.
    ///
    /// Not derived from `calls`: a submit a port refused is still a call to something
    /// implemented. The number comes from the video crate's port table (D558).
    #[serde(default)]
    pub frames: u64,
    /// Whether the buffer the guest last flipped held bytes it had written.
    ///
    /// Separates `presented` from `flipped`. The worker reads the last-flipped buffer back
    /// after the run and sets this when its head differs from the zero a fresh allocation
    /// holds. `status_of` awards `Reach::Presented` only when it is set; it defaults false,
    /// so an unwritten or unreadable buffer stays at `flipped`.
    #[serde(default)]
    pub frame_written: bool,
    /// Every import called, most-used first.
    pub calls: Vec<CalledImport>,
    /// Every system call the guest made directly, not through a stub.
    ///
    /// A guest that enters the kernel by number touches no import, so these are tracked
    /// separately (D401). They are not in `calls`, because `distinct` counts imports and a
    /// system call has no stub index.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub syscalls: Vec<AskedSyscall>,
    /// The last calls the guest made, in the order it made them.
    ///
    /// The ranked list says what a guest spends its time on; at a fault the question is
    /// what it called last, which only the ordered tail answers.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tail: Vec<TracedCall>,
    /// How the guest's calls measured against the calling convention.
    ///
    /// Always present, so "no misaligned calls" is a measurement rather than a silence
    /// (D159).
    #[serde(default)]
    pub abi: AbiReport,
    /// How file reads went.
    ///
    /// Always present, so "no short reads" is a measurement rather than a silence.
    #[serde(default)]
    pub reads: ReadReport,
    /// How formatted writes went.
    ///
    /// Always present once anything was formatted, so "nothing refused" is a measurement
    /// rather than a silence.
    #[serde(default)]
    pub formats: FormatReport,
    /// What the first command buffer the guest submitted to the graphics driver contained.
    /// `None` until a guest hands a buffer to `sceAgcDriverSubmitDcb`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub submission: Option<SubmissionSummary>,
    /// What the guest was pointing at, for calls nothing implements.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dumps: Vec<ArgumentDump>,
    /// What the run was subject to.
    ///
    /// Recorded so two runs under different conditions are not compared as one
    /// measurement; see [`Conditions`].
    #[serde(default)]
    pub conditions: Conditions,
    /// Why the guest stopped itself, if it did.
    ///
    /// A run ends by faulting, by being stopped at a limit, or by the guest deciding to
    /// stop; this records the third, which is its own outcome (D177).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stopped: Option<String>,
    /// Where it died, if it did.
    ///
    /// A progress measure: a faulting instruction pointer that moved forward means the
    /// guest executed code it could not reach before (D129).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fault: Option<FaultSite>,
    /// Imports a run named with `ORBISTOUN_DUMP`, by label.
    ///
    /// Empty in an ordinary run. A `Gap::Captured` finding fires only for imports named
    /// here, not for whatever the default dump condition captured, which also matches every
    /// floating-point-only implementation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub forced_dumps: Vec<String>,
    /// How long the guest went without asking the host for anything, when the clock ended it.
    ///
    /// Separates a guest still working at the time limit from one that stopped asking for
    /// anything, from a single run. `None` for a run that faulted, stopped itself or spent
    /// its call budget, since the guest was active when it ended.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quiet: Option<Quiet>,
    /// Which limit stopped the run, when one did.
    ///
    /// Running out of clock and spending the call budget must not read alike (D238). The
    /// worker's exit code is seen only by the parent after the trace is written, so the
    /// branch that stopped the run records it here. `None` when no limit fired.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_by: Option<String>,
    /// What the guest put into words, oldest first.
    ///
    /// The guest's own diagnostics, which often state a cause directly. Captured at the
    /// platform ABI (the C library's format family, the system log call, and writes to the
    /// standard descriptors), so it is independent of any one engine.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub said: Vec<String>,
    /// Every guest thread, and the last call each one made.
    ///
    /// Says which thread a run that goes quiet is waiting on. Empty for a run with no thread
    /// registry, such as a unit test.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub threads: Vec<ThreadNote>,
    /// Modules the title itself ships, by the library name its imports carry (D640).
    ///
    /// A symbol from one of these is the title's own, so no vendor vocabulary names it and
    /// the advice changes accordingly. Empty when the title ships none or the loader did not
    /// report; an empty list never implies a symbol is the title's.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub title_modules: Vec<String>,
}

/// The silence at the end of a run the clock stopped.
///
/// Activity is import calls plus system calls. A guest that stops moving that number has
/// stopped asking the host for anything, as a thread waiting forever does. It is not proof
/// of blocking, since a guest computing in its own code calls nothing either, so the wording
/// says only what was counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Quiet {
    /// Milliseconds between the last counted activity and the clock expiring.
    pub silent_ms: u64,
    /// Milliseconds from the start of the run to the last counted activity.
    pub last_activity_ms: u64,
    /// How long the run actually lasted, which is not always the limit it was given.
    pub run_ms: u64,
    /// How often the counters were read.
    ///
    /// Recorded because it bounds the precision of the silence.
    pub sample_ms: u64,
}

impl Quiet {
    /// Whether the silence is worth a reader's attention.
    ///
    /// Half the run, and at least a second, so a short run does not report a trivial
    /// silence.
    #[must_use]
    pub fn is_notable(&self) -> bool {
        self.silent_ms >= 1_000 && self.silent_ms.saturating_mul(2) >= self.run_ms
    }

    /// The silence as a sentence, in the terms it was measured in.
    #[must_use]
    pub fn describe(&self) -> String {
        format!(
            "made no call in its last {}.{}s of {}.{}s - the last import or system call was at {}.{}s, sampled every {}ms",
            self.silent_ms / 1000,
            (self.silent_ms % 1000) / 100,
            self.run_ms / 1000,
            (self.run_ms % 1000) / 100,
            self.last_activity_ms / 1000,
            (self.last_activity_ms % 1000) / 100,
            self.sample_ms,
        )
    }
}

/// How many calls of context to keep before the end.
///
/// Enough to see past a burst of one repeated function, such as `memset` while clearing
/// memory.
pub const TAIL_CALLS: usize = 48;

/// One guest thread, as it was when the run ended.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ThreadNote {
    /// The handle the guest holds, which is what it passes to calls naming a thread.
    pub handle: u64,
    /// The name the guest gave it.
    pub name: String,
    /// Whether it has ended.
    pub finished: bool,
    /// The last call this thread made, if one is still in the recorded window.
    ///
    /// [`None`] means not seen recently, not "made no calls": the window holds the last
    /// [`TAIL_CALLS`] calls of the whole run, so a thread that went quiet early falls out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_call: Option<String>,
    /// Where that call sits in the run's order, for reading against the tail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_sequence: Option<u64>,
}

/// What the guest's calls looked like against the System V convention.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AbiReport {
    /// Calls that arrived on a stack the convention forbids.
    pub misaligned_calls: u64,
    /// Sequence number of the first offender.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_misaligned_sequence: Option<u64>,
    /// The import that first arrived misaligned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_misaligned_import: Option<String>,
    /// The stack pointer it arrived with, whole, since the full value says which region
    /// the stack was in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_misaligned_rsp: Option<u64>,
}

/// One call, in sequence.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TracedCall {
    /// Position in the global call order.
    pub sequence: u64,
    /// Library and name, or library and hash when no name is known yet.
    pub label: String,
    /// The integer arguments, in register order: `rdi`, `rsi`, `rdx`, `rcx`, `r8`, `r9`.
    ///
    /// All six, because a value handed on from an earlier answer can appear in any slot.
    #[serde(default)]
    pub args: [u64; 6],
    /// Which host thread made it, matching `FaultSite::host_thread`.
    ///
    /// Only compared for equality, to say whether this call was on the thread that faulted
    /// (D621).
    #[serde(default)]
    pub thread: u64,
    /// The guest address this call returns to - one instruction past the call site.
    ///
    /// In the same address space a fault's frame walk reports, so a stack trace and a call
    /// trace can be read against each other.
    #[serde(default)]
    pub from: u64,
    /// What this call answered in `rax`, when it returned before the trace was read (D459).
    ///
    /// [`None`] when the answer is not known: a call still running, or one whose guest
    /// faulted as the call returned. Distinct from `Some(0)`, which is success.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub returned: Option<u64>,
}

/// Where a guest faulted.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FaultSite {
    /// What it was doing: a read, a write, an instruction fetch.
    ///
    /// Not every value here describes touching an address. See
    /// [`FaultSite::touched_an_address`], which is the question most callers actually
    /// have, and [`FaultSite::TOUCHED`] for the exhaustive list.
    pub kind: String,
    /// The address it touched.
    pub address: u64,
    /// The instruction that touched it. This is the number that measures progress.
    pub instruction_pointer: u64,
    /// Where the instruction was: a region orbistoun placed, or the host module holding it.
    ///
    /// Host modules are named because Windows bases system modules per boot, so a bare host
    /// address does not reproduce across reboots while an offset into a named module does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    /// Offset into that region or module.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<u64>,
    /// What each register that holds a readable address is pointing at.
    ///
    /// The register values alone do not say what they point at. Filled by the worker,
    /// because deciding whether an address is readable and naming its region need the
    /// running process.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pointees: Vec<String>,
    /// The import the guest was inside when it faulted, if it was inside one.
    ///
    /// For a fault in host code, this narrows the search from the whole emulator to one
    /// function.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inside_import: Option<String>,
    /// The guest's own call path, innermost first.
    ///
    /// Empty when the chain could not be walked, which is ordinary for optimised code that
    /// omits the frame pointer.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub frames: Vec<Frame>,
    /// The host thread the fault happened on, as the recorded calls carry it.
    ///
    /// Compared for equality against `TracedCall::thread`, to say which recent calls were on
    /// the thread that faulted (D621).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_thread: Option<u64>,
    /// Which guest thread faulted, as the guest's own handle for it.
    ///
    /// The call tail shows every thread, so this says which one the fault was on (D621).
    /// `None` when nothing claimed this host thread as a guest one, as for the main thread
    /// before the loader hands over.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread: Option<u64>,
    /// The registers as they were at the fault.
    ///
    /// Taken from the context record the vectored handler receives. The stack pointer alone
    /// distinguishes stack exhaustion from a bad pointer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registers: Option<Registers>,
    /// The bytes of the faulting instruction itself.
    ///
    /// Read by the worker at fault time, so [`classify_trap`] can tell a bad dereference from
    /// a kernel entry such as `int 0x41` in the ranked finding.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub instruction: Vec<u8>,
}

/// A faulting instruction that is a trap (the guest entering the kernel or stopping itself),
/// as opposed to an ordinary instruction that touched a bad address. A kernel entry is a gap
/// for orbistoun to implement; a guest trap is the guest aborting on its own check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrapKind {
    /// `int`, `syscall`, `sysenter` or `hlt`: the guest entered the kernel through an
    /// instruction with no handler. The vector is carried for `int`, since it names the entry.
    KernelEntry {
        /// The interrupt vector for `int n`; `None` for `syscall`/`sysenter`/`hlt`.
        vector: Option<u8>,
    },
    /// `ud2`: a trap the guest raised itself after a failed check. The cause is upstream, in
    /// whatever the guest was told just before.
    GuestTrap,
}

/// Classifies a faulting instruction as a trap, if it is one.
///
/// One classifier for the live crash print and the ranked finding, so they agree. Pure and
/// allocation-free, so the fault handler can call it.
#[must_use]
pub fn classify_trap(opcode: &[u8]) -> Option<TrapKind> {
    match opcode.first().copied()? {
        0xf4 => Some(TrapKind::KernelEntry { vector: None }), // hlt
        0xcd => Some(TrapKind::KernelEntry {
            vector: Some(opcode.get(1).copied().unwrap_or(0)),
        }),
        0x0f if matches!(opcode.get(1), Some(&0x05 | &0x34)) => {
            Some(TrapKind::KernelEntry { vector: None }) // syscall / sysenter
        }
        0x0f if opcode.get(1) == Some(&0x0b) => Some(TrapKind::GuestTrap), // ud2
        _ => None,
    }
}

impl FaultSite {
    /// Kinds whose [`address`](Self::address) is somewhere the guest actually touched.
    ///
    /// An access violation carries the address in its exception parameters, so the number
    /// means what it looks like it means.
    pub const TOUCHED: [&'static str; 3] = ["write to", "read of", "instruction fetch from"];

    /// The kind a breakpoint carries when the faulting address is inside the stub table.
    ///
    /// Only when the address has been checked against the stub table.
    pub const BREAKPOINT_IN_STUBS: &'static str = "breakpoint - stub padding - at";
    /// A breakpoint at an address the stub table demonstrably does not cover.
    ///
    /// A guest trap instruction, an assertion or a debugger attaching all land here, and a
    /// fault record cannot tell them apart, so this names none of them.
    pub const BREAKPOINT_OUTSIDE_STUBS: &'static str = "breakpoint - not stub padding - at";
    /// A breakpoint that could not be placed, because no stub table was registered.
    ///
    /// Distinct from [`Self::BREAKPOINT_OUTSIDE_STUBS`]: "checked, and not there" and
    /// "nothing to check against" are different states.
    pub const BREAKPOINT_UNPLACED: &'static str = "breakpoint - at";

    /// Kinds whose [`address`](Self::address) is the faulting instruction itself.
    ///
    /// These exceptions carry no address parameters, so the reporter fills the field with
    /// the instruction pointer, which is not an address the guest asked for. Every such kind
    /// must be listed, or a consumer misclassifies it by falling through.
    pub const AT_THE_INSTRUCTION: [&'static str; 5] = [
        "illegal instruction at",
        Self::BREAKPOINT_IN_STUBS,
        "stack overflow at",
        Self::BREAKPOINT_OUTSIDE_STUBS,
        Self::BREAKPOINT_UNPLACED,
    ];

    /// Whether [`address`](Self::address) is somewhere the guest asked for.
    ///
    /// A sweep planting sentinels compares fault addresses run to run; for a kind that
    /// reports the instruction pointer, both sentinels give the same address, which would
    /// read as a move unrelated to the plant.
    #[must_use]
    pub fn touched_an_address(&self) -> bool {
        Self::TOUCHED.contains(&self.kind.as_str())
    }
}

/// Whether the guest received every byte it asked for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ReadReport {
    /// Reads attempted.
    pub reads: u64,
    /// Reads cut short before the end of their file.
    pub short: u64,
    /// Bytes delivered.
    pub bytes: u64,
}

/// What the guest was pointing at when it called something nothing implements.
///
/// A trace gives an argument's value; for an out-parameter or a descriptor structure the
/// bytes it points at are the information.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ArgumentDump {
    /// The import that was called, named where a name is known.
    pub label: String,
    /// Which argument, counting from zero.
    pub slot: u8,
    /// Where the bytes came from, described against a known region where possible.
    pub at: String,
    /// The argument's raw value, whether or not it points anywhere.
    #[serde(default)]
    pub value: u64,
    /// The bytes at that address, hex-encoded, when it pointed into mapped memory.
    ///
    /// Empty for a scalar (a size, a flag, a count), whose raw value is the evidence.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub bytes: String,
    /// The same bytes as text, where they read as text.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
}

/// What formatted writes managed.
///
/// A formatted write that refuses hands the guest an empty string yet still counts as
/// reaching an implementation, so this separates "implemented" from "answered" (D183).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FormatReport {
    /// Formatted writes attempted.
    pub calls: u64,
    /// Writes that produced nothing because the format could not be honoured.
    pub refused: u64,
    /// Writes whose result did not fit the destination.
    pub truncated: u64,
    /// The first conversion that could not be honoured, described.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub first_fault: String,
}

/// What the first command buffer a guest submitted turned out to contain.
///
/// The packets a title built, the registers it set, the draws it asked for and the shader
/// addresses it named: the first graphics measurement a run produces. A summary of
/// `SubmissionReport`, plus the shader failures as text, since they explain a draw with
/// nothing bound.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SubmissionSummary {
    /// Packets the walk recognised in the submitted buffer.
    pub packets: usize,
    /// Register writes extracted from them.
    pub register_writes: usize,
    /// Draws the stream asked for (auto-indexed; an indexed draw's count is separate state).
    pub draws: usize,
    /// Shader addresses the register writes named.
    pub shaders_found: usize,
    /// Of the addresses a register named, how many fell in a region the guest was given.
    ///
    /// Evidence on whether GPU addresses in the stream equal guest addresses (D101). Counted
    /// apart from the shader outcome, because an address can resolve and its shader still
    /// fail to translate.
    #[serde(default)]
    pub addresses_resolved: usize,
    /// Addresses a register named that fell in no region the guest was given.
    #[serde(default)]
    pub addresses_unresolved: usize,
    /// Of the shader addresses named, how many produced a module a backend can bind.
    #[serde(default)]
    pub shaders_translated: usize,
    /// Every shader that did not translate, as `stage at address: reason`.
    ///
    /// Says why a draw has nothing bound, so a translation gap is not read as a backend gap.
    #[serde(default)]
    pub shader_failures: Vec<String>,
}

/// What a run was subject to, as opposed to what it found.
///
/// Attributing a difference between two runs to a change is valid only if everything else
/// was identical (D181). The time limit is wall-clock, so a faster machine reaches further;
/// the stub policy decides what unimplemented functions answer, and loosening it improves
/// every number without implementing anything. Both are recorded rather than forbidden, so
/// a comparison shows them.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Conditions {
    /// The wall-clock limit in seconds, or `None` for no limit.
    ///
    /// A backstop, not the measurement: it fixes the duration and lets the call count vary.
    /// It catches a guest that stops calling imports altogether (D238).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit_seconds: Option<u64>,
    /// Diagnostics that were asked for and did nothing at all.
    ///
    /// A diagnostic that never reached its target looks like one that changed nothing, so
    /// one that applied zero times is recorded and reported at the verdict (D227).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub did_nothing: Vec<String>,
    /// Imports the guest was allowed to call, or `None` for no budget.
    ///
    /// Deterministic: two runs of one build stop at the same call, so a verdict measures the
    /// change rather than the machine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_budget: Option<u64>,
    /// What a function with no implementation answered, spelled as the policy spells it.
    ///
    /// A string rather than the policy type, which is defined in a crate above this one.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub default_return: String,
    /// How many symbols the policy said something specific about (an answer, a region, or
    /// both), counted once each.
    #[serde(default)]
    pub overrides: usize,
    /// How many of those rest on nothing measured, and are therefore holding the run up.
    ///
    /// An answer measured on the target is the emulator being right; a guessed answer is a
    /// prop. Only props hold a compatibility entry back (D557).
    #[serde(default)]
    pub propping: usize,
    /// Every diagnostic the run was put under, or empty for an ordinary run.
    ///
    /// A run under a diagnostic answers a different question from "how far does it get?",
    /// so it is not compared as an ordinary run. One field for every diagnostic, so a new
    /// one cannot go unrecorded.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub experiments: String,
    /// The physical memory map the guest was shown, region by region.
    ///
    /// The offsets a guest queries mean something only against the boundaries it was
    /// given, so the run records them rather than leaving a reader to recompute them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub memory_map: Vec<(u64, u64, bool)>,
    /// Whether any of them changed the program rather than only observing it.
    ///
    /// Separate from the words above because it is acted on: a verdict earned under an
    /// intervention carries a warning (D227).
    #[serde(default)]
    pub intervened: bool,
    /// The build that produced the trace.
    ///
    /// Recorded but not compared, since it changes with every build; it identifies a
    /// result contributed from another tree.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub build: String,
}

impl Conditions {
    /// Whether unimplemented functions were reporting success.
    ///
    /// The state in which reaching further means less, not more.
    pub fn answers_blindly(&self) -> bool {
        !self.default_return.is_empty() && self.default_return != "unimplemented"
    }

    /// What differs from an earlier run, in words a report can print directly.
    ///
    /// Empty means the two are comparable. Sentences, because the only consumer prints them
    /// beside a verdict.
    pub fn differences_from(&self, before: &Self) -> Vec<String> {
        let mut changed = Vec::new();
        if self.call_budget != before.call_budget {
            changed.push(format!(
                "the call budget changed from {} to {}",
                describe_budget(before.call_budget),
                describe_budget(self.call_budget)
            ));
        }
        if self.limit_seconds != before.limit_seconds {
            changed.push(format!(
                "the time limit changed from {} to {}",
                describe_limit(before.limit_seconds),
                describe_limit(self.limit_seconds)
            ));
        }
        if self.default_return != before.default_return
            && !self.default_return.is_empty()
            && !before.default_return.is_empty()
        {
            changed.push(format!(
                "unimplemented functions now answer {} instead of {}",
                self.default_return, before.default_return
            ));
        }

        if self.overrides != before.overrides {
            changed.push(format!(
                "explicit stub answers went from {} to {}",
                before.overrides, self.overrides
            ));
        }
        if self.experiments != before.experiments {
            changed.push(format!(
                "this run was under {} and the last was under {}",
                describe_planted(&self.experiments),
                describe_planted(&before.experiments)
            ));
        }
        changed
    }
}

/// How to write the diagnostics a run was under, including none.
fn describe_planted(what: &str) -> String {
    if what.is_empty() {
        "no diagnostics".to_owned()
    } else {
        what.to_owned()
    }
}

/// How to write a time limit, including its absence.
fn describe_budget(calls: Option<u64>) -> String {
    calls.map_or_else(|| "no budget".to_owned(), |c| format!("{c} calls"))
}

/// The wall-clock limit, as a person reads it.
fn describe_limit(seconds: Option<u64>) -> String {
    seconds.map_or_else(|| "no limit".to_owned(), |s| format!("{s}s"))
}

/// One frame on the guest's stack.
///
/// A fault address says where the guest died, not who called it; at the top of a function,
/// where a null dereference usually lands, the instruction pointer alone says little.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Frame {
    /// Where this frame returns to: one instruction past the call site.
    pub return_address: u64,
    /// The frame pointer it was found through.
    pub frame_pointer: u64,
}

/// The integer registers at a fault.
///
/// The System V set, named as the guest's calling convention names them, because that is
/// how the arguments to whatever was running are read off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[expect(
    missing_docs,
    reason = "each field is one x86-64 register and names itself"
)]
pub struct Registers {
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rbp: u64,
    pub rsp: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
}

impl Registers {
    /// Every register, in dump order, with its name.
    ///
    /// One list, used by both the printed lines and the pointee description, so they agree.
    pub fn named(&self) -> [(&'static str, u64); 16] {
        [
            ("rax", self.rax),
            ("rbx", self.rbx),
            ("rcx", self.rcx),
            ("rdx", self.rdx),
            ("rsi", self.rsi),
            ("rdi", self.rdi),
            ("rbp", self.rbp),
            ("rsp", self.rsp),
            ("r8", self.r8),
            ("r9", self.r9),
            ("r10", self.r10),
            ("r11", self.r11),
            ("r12", self.r12),
            ("r13", self.r13),
            ("r14", self.r14),
            ("r15", self.r15),
        ]
    }

    /// Every register, as lines to print under a fault.
    ///
    /// All sixteen, since any register may hold the bad base. Grouped four to a line so the
    /// dump does not wrap in a terminal.
    pub fn lines(&self) -> Vec<String> {
        let all = self.named();
        all.chunks(4)
            .map(|row| {
                row.iter()
                    .map(|(name, value)| format!("{name}={value:#x}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect()
    }
}

/// One system call the guest asked the kernel for by number.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AskedSyscall {
    /// The number the guest passed.
    pub number: u64,
    /// What this run knows that number to be, where it knows anything.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Its first argument, which for an unnamed call narrows what it might be.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_argument: Option<u64>,
}

/// One import a guest called, and whether anything answered it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CalledImport {
    /// Dynamic symbol index: the stub it landed on.
    pub index: usize,
    /// Library and name, or library and hash when no name is known yet.
    pub label: String,
    /// How many times.
    pub calls: u64,
    /// Whether anything actually implements it, or whether it landed on a stub.
    ///
    /// Separates a call that was answered from one that got a placeholder (D179).
    #[serde(default)]
    pub implemented: bool,
    /// The signature inferred from how the guest called it, such as `(ptr, u32, ptr?)`, or
    /// empty when nothing was sampled. A characterisation from the guest's own calls, not a
    /// proof: a starting point for implementing the function or designing a probe for it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub shape: String,
}

impl CallTrace {
    /// Calls that landed on something with no implementation behind them.
    ///
    /// A call count is progress only to the extent the calls were answered by something
    /// real, so this is reported beside it.
    pub fn stubbed_calls(&self) -> u64 {
        self.calls
            .iter()
            .filter(|c| !c.implemented)
            .map(|c| c.calls)
            .sum()
    }

    /// Distinct imports the guest called that had nothing behind them.
    ///
    /// Counts functions rather than calls: `stubbed_share` is dominated by whatever the
    /// guest loops on, while this is stable against a hot loop and is the work list (D563).
    pub fn unanswered_imports(&self) -> usize {
        self.calls.iter().filter(|c| !c.implemented).count()
    }

    /// The distinct imports the guest called that landed on a placeholder, most-called first.
    ///
    /// The candidate causes of a fault: each is a place orbistoun answered with a placeholder
    /// (D708).
    pub fn stubbed_imports(&self) -> Vec<&CalledImport> {
        let mut stubbed: Vec<&CalledImport> =
            self.calls.iter().filter(|c| !c.implemented).collect();
        stubbed.sort_by(|a, b| b.calls.cmp(&a.calls).then_with(|| a.label.cmp(&b.label)));
        stubbed
    }

    /// What share of the run rested on stubs, as a percentage.
    ///
    /// Zero when nothing was called.
    pub fn stubbed_share(&self) -> u32 {
        if self.total_calls == 0 {
            return 0;
        }
        u32::try_from(self.stubbed_calls().saturating_mul(100) / self.total_calls).unwrap_or(100)
    }
}

/// What a run says about a title, for the compatibility record.
///
/// Derived from the trace, never written by hand, so an entry transcribes a measurement.
/// `measured_on` is passed in because this crate has no clock, which keeps it testable.
pub fn status_of(trace: &CallTrace, measured_on: String) -> orbistoun_overrides::Status {
    use orbistoun_overrides::Reach;

    // The ladder follows the phases in order. A guest that faulted still entered; how it
    // ended is the outcome, carried by `outcome`.
    let reach = match trace.reached.as_str() {
        // The frame count comes from the video crate's port table, so a flip rung needs a
        // flip a port accepted (D558). A flip whose buffer read back holding guest-written
        // bytes reaches `Presented`; checked before the plain flip arm as the stronger claim.
        "Entered" if trace.frames > 0 && trace.frame_written => Reach::Presented,
        "Entered" if trace.frames > 0 => Reach::Flipped,
        // After the flip arms: a guest that flipped and then exited is recorded as flipped,
        // with the deliberate stop carried in the outcome.
        "Entered" if trace.stopped.as_deref() == Some(orbistoun_overrides::DELIBERATE_EXIT) => {
            Reach::Exited
        }
        "Entered" => Reach::Entered,
        "Linked" => Reach::Linked,
        "ImportsResolved" | "ContainerParsed" => Reach::Parsed,
        _ => Reach::Rejected,
    };

    orbistoun_overrides::Status {
        reach,
        outcome: describe_end(trace),
        imports: trace.distinct,
        calls: trace.total_calls,
        standing: 100_u32.saturating_sub(trace.stubbed_share()),
        default_return: trace.conditions.default_return.clone(),
        overrides: trace.conditions.overrides,
        propping: trace.conditions.propping,
        frames: trace.frames,
        unanswered: Some(trace.unanswered_imports()),
        limit_seconds: trace.conditions.limit_seconds,
        build: trace.conditions.build.clone(),
        measured_on,
        notes: String::new(),
    }
}

/// The file name a module's trace is written to.
///
/// Declared once, because the worker writes the file and a shim reads it back, and the two
/// must agree. The last two path components, because `eboot.bin` alone is the same in
/// every title.
pub fn trace_file_name(module: &str) -> String {
    let stem: String = std::path::Path::new(module)
        .components()
        .rev()
        .take(2)
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("-")
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    format!("{stem}.json")
}

/// Reads the trace a previous run of `module` left behind.
///
/// `None` covers both "no previous run" and "the file is unreadable or stale-format": the
/// caller acts the same either way, and a first run is not an error.
pub fn load_previous(traces_dir: &std::path::Path, module: &std::path::Path) -> Option<CallTrace> {
    let name = trace_file_name(&module.to_string_lossy());
    let text = std::fs::read_to_string(traces_dir.join(name)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Identity of a trace file on disk, for telling one run's trace from an older one.
///
/// Modification time and length, because either alone can repeat: traces of one guest
/// have similar sizes, and a filesystem timestamp can be coarse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    /// When the file was last written.
    pub modified: std::time::SystemTime,
    /// How long it is, in bytes.
    pub len: u64,
}

/// The identity of `module`'s trace file, or `None` if there is not one.
pub fn stamp_of(traces_dir: &std::path::Path, module: &std::path::Path) -> Option<Stamp> {
    let name = trace_file_name(&module.to_string_lossy());
    let data = std::fs::metadata(traces_dir.join(name)).ok()?;
    Some(Stamp {
        modified: data.modified().ok()?,
        len: data.len(),
    })
}

/// Whether the run that just finished actually wrote a trace.
///
/// A worker that dies without writing leaves the previous run's file in place, which would
/// otherwise be reported as this run's and compare as "same" (D470). A pure decision, so it
/// is testable without a filesystem; the effectful half is [`stamp_of`].
#[must_use]
pub fn wrote_a_trace(before: Option<Stamp>, after: Option<Stamp>) -> bool {
    match (before, after) {
        // Nothing there afterwards means nothing was written, whatever was there before.
        (_, None) => false,
        // The first trace this module has ever had.
        (None, Some(_)) => true,
        // Rewritten only if it is a different file than the one read before the run.
        (Some(was), Some(now)) => was != now,
    }
}

/// How this run compares with the last one of the same module.
///
/// Two signals, interface reached and fault position, because either can move while the
/// other goes backwards when the code path changes (D129).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Nothing to compare against.
    FirstRun,
    /// Reached imports it could not reach before.
    Further,
    /// Reaching less of the interface than it did.
    Back,
    /// Nothing moved.
    Same,
    /// More of the interface, but along a different path whose positions do not compare.
    MoreInterfaceDifferentPath,
    /// Further along its path, but reaching less of the interface.
    FurtherButNarrower,
}

impl Verdict {
    /// The short label a shim prints beside the summary.
    ///
    /// Upper case for the verdicts that matter, so `FURTHER` stands out in terminal output.
    pub const fn label(self) -> &'static str {
        match self {
            Self::FirstRun => "",
            Self::Further => "FURTHER",
            Self::Back => "BACK",
            Self::Same => "same",
            Self::MoreInterfaceDifferentPath | Self::FurtherButNarrower => "MIXED",
        }
    }

    /// The one-line summary a shim prints.
    ///
    /// Here rather than in a shim, so the CLI and the GUI describe it identically (D034).
    pub const fn summary(self) -> &'static str {
        match self {
            Self::FirstRun => "first run of this module - nothing to compare against yet",
            // `Further` fires for more of the interface or for the same interface with the
            // fault further along; executing code it could not reach before is true of both.
            Self::Further => "executed code it could not reach before",
            Self::Back => "reaching less of the interface than it did",
            Self::Same => "nothing moved",
            Self::MoreInterfaceDifferentPath => {
                "more of the interface reached, but along a different path - the positions do not compare"
            }
            Self::FurtherButNarrower => {
                "further along its path, but reaching less of the interface"
            }
        }
    }

    /// Whether this is worth drawing attention to.
    pub const fn is_progress(self) -> bool {
        matches!(self, Self::Further | Self::FurtherButNarrower)
    }
}

/// The measured difference between two runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Progress {
    /// Change in distinct imports reached.
    pub distinct_delta: i64,
    /// Change in total calls made.
    pub calls_delta: i128,
    /// Where this run died, described.
    pub fault: String,
    /// Where the previous run died, if it differed.
    pub previous_fault: Option<String>,
    /// The verdict.
    pub verdict: Verdict,
    /// Whether this run got further while the program was being altered.
    ///
    /// A diagnostic that intervenes (a poked value, a poisoned region, an unrequested
    /// reservation) changes the program being measured, so progress under it may rest on a
    /// wrong answer and is not a diagnosis (D227).
    pub bought_under_intervention: bool,
    /// Whether this run ended without faulting at all.
    ///
    /// Then `Further` rests on the interface count alone and means "reached more imports",
    /// not "got further", and the report says so (D301).
    pub ended_without_a_fault: bool,
    /// What changed about the *run* rather than about the emulator, in words.
    ///
    /// When non-empty, the verdict measures a settings change; it still renders, labelled
    /// as such (D181).
    pub conditions_changed: Vec<String>,
}

/// Describes where a run ended, for comparison and for display.
///
/// A run that hit a limit has no fault position, which is not the same as address zero.
fn describe_end(trace: &CallTrace) -> String {
    if let Some(fault) = &trace.fault {
        return match (&fault.region, fault.offset) {
            (Some(region), Some(offset)) => format!("{region}+{offset:#x}"),
            _ => format!("{:#x}", fault.instruction_pointer),
        };
    }
    // The guest stopping itself is a decision, not a limit.
    if let Some(stopped) = &trace.stopped {
        return stopped.clone();
    }
    // The budget and the clock are different endings (D238). Only the branch that stopped
    // the run knows which fired, so this reads what it recorded rather than inferring it.
    // A stuck guest and a working one both end on the clock and get different outcomes.
    // The outcome is categorical: `compare` tests it between runs, so a duration here would
    // make every pair differ; the measurement stays in `quiet`.
    if trace.ended_by.as_deref() == Some(SPENT_THE_BUDGET) {
        return SPENT_THE_BUDGET.to_owned();
    }
    match trace.quiet {
        Some(quiet) if quiet.is_notable() => QUIET_TO_LIMIT.to_owned(),
        _ => RAN_TO_LIMIT.to_owned(),
    }
}

/// A run the clock ended while the guest was still asking the host for things.
pub const RAN_TO_LIMIT: &str = "ran to the time limit";

/// A run the call budget ended, which is not the clock running out: the next step is to
/// raise the budget rather than find what the guest waits for (D238).
pub const SPENT_THE_BUDGET: &str = "spent its call budget";

/// A run the clock ended after the guest had stopped asking for anything.
///
/// "Went quiet" is the whole claim: no import or system call for at least a second and at
/// least half the run ([`Quiet::is_notable`]). A guest computing in its own code reaches
/// this too, so it does not claim the guest was waiting.
pub const QUIET_TO_LIMIT: &str = "went quiet, then ran to the time limit";

/// Compares a run against the one before it.
///
/// Pure, so the verdict is testable without running a guest (D129).
pub fn compare(before: Option<&CallTrace>, after: &CallTrace) -> Progress {
    use core::cmp::Ordering::{Equal, Greater, Less};

    let fault = describe_end(after);
    let Some(before) = before else {
        return Progress {
            distinct_delta: 0,
            calls_delta: 0,
            fault,
            previous_fault: None,
            verdict: Verdict::FirstRun,
            // Nothing to have changed against.
            bought_under_intervention: false,
            ended_without_a_fault: after.fault.is_none(),
            conditions_changed: Vec::new(),
        };
    };

    let previous = describe_end(before);
    let surface = after.distinct.cmp(&before.distinct);
    // Positions compare only when both runs died in the same kind of place; an instruction
    // pointer on a different code path is not comparable.
    let position = match (&after.fault, &before.fault) {
        (Some(a), Some(b)) if a.region == b.region => Some(a.offset.cmp(&b.offset)),
        // A missing fault is not a position in either direction; the surface count alone
        // decides the verdict (D301).
        _ => None,
    };

    // Disagreeing signals have their own verdicts and are matched first; then either signal
    // moving alone decides.
    let verdict = match (surface, position) {
        (Greater, Some(Less)) => Verdict::MoreInterfaceDifferentPath,
        (Less, Some(Greater)) => Verdict::FurtherButNarrower,
        (Greater, _) | (Equal, Some(Greater)) => Verdict::Further,
        (Less, _) | (Equal, Some(Less)) => Verdict::Back,
        (Equal, _) => Verdict::Same,
    };

    Progress {
        distinct_delta: after.distinct as i64 - before.distinct as i64,
        calls_delta: i128::from(after.total_calls) - i128::from(before.total_calls),
        fault,
        previous_fault: (previous != describe_end(after)).then_some(previous),
        verdict,
        // Only when the verdict is progress, so the caveat is not noise on every
        // instrumented run (D227).
        bought_under_intervention: matches!(
            verdict,
            Verdict::Further | Verdict::FurtherButNarrower | Verdict::MoreInterfaceDifferentPath
        ) && after.conditions.intervened,
        ended_without_a_fault: after.fault.is_none(),
        conditions_changed: after.conditions.differences_from(&before.conditions),
    }
}

#[cfg(test)]
mod syscall_record_tests {
    use super::{AskedSyscall, CallTrace};

    /// A trace without the newer fields still loads, since the work list reads every file in
    /// the traces directory.
    #[test]
    fn a_trace_without_the_field_still_loads() {
        let older = r#"{"module":"x","reached":"Entered","total_calls":0,"distinct":0,"calls":[]}"#;
        let trace: CallTrace = serde_json::from_str(older).expect("an older trace still parses");
        assert!(trace.syscalls.is_empty(), "and reads as none asked for");
    }

    /// What was recorded comes back, including the argument that says what a call might be.
    #[test]
    fn an_asked_syscall_survives_the_round_trip() {
        let mut trace: CallTrace = serde_json::from_str(
            r#"{"module":"x","reached":"Entered","total_calls":0,"distinct":0,"calls":[]}"#,
        )
        .expect("parses");
        trace.syscalls = vec![AskedSyscall {
            number: 649,
            name: None,
            first_argument: Some(2),
        }];
        let text = serde_json::to_string(&trace).expect("serialises");
        let back: CallTrace = serde_json::from_str(&text).expect("parses back");
        assert_eq!(back.syscalls.len(), 1);
        assert_eq!(back.syscalls[0].number, 649);
        assert_eq!(back.syscalls[0].first_argument, Some(2));
    }

    /// A submission's resolved and unresolved address counts survive serialisation (D101).
    #[test]
    fn a_submissions_resolved_counts_survive_the_round_trip() {
        let mut trace: CallTrace = serde_json::from_str(
            r#"{"module":"x","reached":"Entered","total_calls":0,"distinct":0,"calls":[]}"#,
        )
        .expect("parses");
        trace.submission = Some(super::SubmissionSummary {
            packets: 40,
            register_writes: 31,
            draws: 2,
            shaders_found: 3,
            addresses_resolved: 2,
            addresses_unresolved: 1,
            shaders_translated: 0,
            shader_failures: vec!["vertex at 0x2000: why".to_owned()],
        });
        let text = serde_json::to_string(&trace).expect("serialises");
        let back: CallTrace = serde_json::from_str(&text).expect("parses back");
        let summary = back.submission.expect("the submission survives");
        assert_eq!(summary.addresses_resolved, 2);
        assert_eq!(summary.addresses_unresolved, 1);
        assert_eq!(summary.shaders_found, 3);
    }
}

#[cfg(test)]
mod tail_return_tests {
    use super::{CallTrace, TracedCall};

    fn base() -> CallTrace {
        serde_json::from_str(
            r#"{"module":"x","reached":"Entered","total_calls":0,"distinct":0,"calls":[]}"#,
        )
        .expect("parses")
    }

    /// A recorded answer of zero survives and is not confused with absence (D459).
    #[test]
    fn a_zero_answer_survives_and_is_not_absence() {
        let mut trace = base();
        trace.tail = vec![TracedCall {
            thread: 0,
            sequence: 3,
            label: "libkernel::sceKernelMapDirectMemory".to_owned(),
            args: [0x6000_0080_0d28, 0, 0, 0, 0, 0],
            from: 0x4000_0159_6189,
            returned: Some(0),
        }];
        let text = serde_json::to_string(&trace).expect("serialises");
        let back: CallTrace = serde_json::from_str(&text).expect("parses back");
        assert_eq!(back.tail[0].returned, Some(0));
    }

    /// An unknown answer writes nothing, so it never reads back as zero.
    #[test]
    fn an_unknown_answer_is_absent_rather_than_zero() {
        let mut trace = base();
        trace.tail = vec![TracedCall {
            thread: 0,
            sequence: 1,
            label: "libc::strlen".to_owned(),
            args: [0x10, 0, 0, 0, 0, 0],
            from: 0x20,
            returned: None,
        }];
        let text = serde_json::to_string(&trace).expect("serialises");
        assert!(
            !text.contains("returned"),
            "an unknown answer must leave no field to be mistaken for zero"
        );
        let back: CallTrace = serde_json::from_str(&text).expect("parses back");
        assert_eq!(back.tail[0].returned, None);
    }

    /// A trace without the answer field still loads, reading as unknown.
    #[test]
    fn a_tail_without_the_field_still_loads() {
        let older = r#"{"module":"x","reached":"Entered","total_calls":0,"distinct":0,"calls":[],"tail":[{"sequence":1,"label":"libc::strlen","arg0":16,"from":32}]}"#;
        let trace: CallTrace = serde_json::from_str(older).expect("an older trace still parses");
        assert_eq!(
            trace.tail[0].returned, None,
            "an older record reads as unknown, never as a zero answer"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{CallTrace, CalledImport, Conditions, FaultSite, Verdict, compare};

    /// A guest still calling when the clock stopped it is not quiet.
    ///
    /// A false positive would put a warning on every report that hits the limit.
    #[test]
    fn a_run_that_called_something_at_the_end_is_not_notable() {
        let busy = super::Quiet {
            silent_ms: 240,
            last_activity_ms: 19_760,
            run_ms: 20_000,
            sample_ms: 250,
        };
        assert!(!busy.is_notable());
    }

    /// A silence under a second is not notable however large a share of the run it is.
    ///
    /// The floor exists for short runs, where half the run is sampling and startup.
    #[test]
    fn a_brief_silence_in_a_brief_run_is_not_notable() {
        let brief = super::Quiet {
            silent_ms: 900,
            last_activity_ms: 900,
            run_ms: 1_800,
            sample_ms: 250,
        };
        assert!(brief.silent_ms.saturating_mul(2) >= brief.run_ms);
        assert!(!brief.is_notable());
    }

    /// A long silence at the end of a long run is notable and described in tenths of a second.
    #[test]
    fn a_guest_that_stopped_calling_is_notable_and_says_when() {
        let stuck = super::Quiet {
            silent_ms: 17_250,
            last_activity_ms: 2_750,
            run_ms: 20_000,
            sample_ms: 250,
        };
        assert!(stuck.is_notable());
        // Tenths, truncated rather than rounded, since the sample interval is a quarter second.
        assert_eq!(
            stuck.describe(),
            "made no call in its last 17.2s of 20.0s - the last import or system call was at 2.7s, sampled every 250ms"
        );
    }

    /// The unanswered count is of functions, not of calls, so a hot loop does not swing it
    /// (D563).
    #[test]
    fn the_record_counts_functions_not_calls() {
        let mut t = trace(3, 10_000, None, None);
        t.calls = vec![
            CalledImport {
                index: 0,
                label: "libc::memcpy".to_owned(),
                calls: 9_000,
                implemented: true,
                shape: String::new(),
            },
            // One function, thousands of calls: a hot loop on something unimplemented.
            CalledImport {
                index: 1,
                label: "libkernel::sceKernelWaitEqueue".to_owned(),
                calls: 990,
                implemented: false,
                shape: String::new(),
            },
            CalledImport {
                index: 2,
                label: "libc::setlocale".to_owned(),
                calls: 1,
                implemented: false,
                shape: String::new(),
            },
        ];

        assert_eq!(
            t.unanswered_imports(),
            2,
            "two functions are unanswered - the 990 calls are one of them, not 990 of them"
        );
        assert_eq!(
            t.stubbed_calls(),
            991,
            "and the call count still says what it says"
        );
        let status = super::status_of(&t, "2026-09-04".to_owned());
        assert_eq!(status.unanswered, Some(2), "and it reaches the record");
        assert_eq!(status.answered(), 1, "of three imports, one was answered");
    }

    /// A run reaches the flip rung by a frame count from the port table, not by a word
    /// (D558). The count's own correctness is tested where the port table lives.
    #[test]
    fn the_presenting_rung_comes_from_the_frame_count() {
        use orbistoun_overrides::Reach;

        let mut entered = trace(47, 933, None, None);
        entered.frames = 0;
        assert_eq!(
            super::status_of(&entered, "2026-09-04".to_owned()).reach,
            Reach::Entered,
            "a guest that presented nothing is not at the presenting rung"
        );

        let mut presented = trace(47, 933, None, None);
        presented.frames = 1;
        let status = super::status_of(&presented, "2026-09-04".to_owned());
        assert_eq!(status.reach, Reach::Flipped);
        assert_eq!(status.frames, 1, "and the count travels with the rung");
    }

    /// A guest that went quiet and one still working get different outcomes, and a silence
    /// too short or too small a fraction of the run does not change the outcome.
    #[test]
    fn a_run_that_went_quiet_is_not_filed_as_one_still_working() {
        let working = trace(47, 933, None, None);
        assert_eq!(
            super::describe_end(&working),
            super::RAN_TO_LIMIT,
            "nothing measured a silence, so nothing may be claimed about one"
        );

        // Seventeen silent seconds of twenty: notable by both halves of the rule.
        let mut stuck = trace(47, 933, None, None);
        stuck.quiet = Some(super::Quiet {
            silent_ms: 17_000,
            last_activity_ms: 3_000,
            run_ms: 20_000,
            sample_ms: 250,
        });
        assert_eq!(super::describe_end(&stuck), super::QUIET_TO_LIMIT);

        // Half the run, but under the one-second floor.
        let mut brief = trace(47, 933, None, None);
        brief.quiet = Some(super::Quiet {
            silent_ms: 900,
            last_activity_ms: 600,
            run_ms: 1_500,
            sample_ms: 250,
        });
        assert_eq!(
            super::describe_end(&brief),
            super::RAN_TO_LIMIT,
            "below the floor is not a silence worth a category"
        );

        // Over the floor, but a small fraction of a long run - still working, with a pause.
        let mut paused = trace(47, 933, None, None);
        paused.quiet = Some(super::Quiet {
            silent_ms: 2_000,
            last_activity_ms: 88_000,
            run_ms: 90_000,
            sample_ms: 250,
        });
        assert_eq!(
            super::describe_end(&paused),
            super::RAN_TO_LIMIT,
            "two seconds of ninety is a pause, not a stop"
        );

        // A guest that stopped itself is neither: its own decision outranks the clock.
        let mut stopped = trace(47, 933, None, None);
        stopped.quiet = Some(super::Quiet {
            silent_ms: 17_000,
            last_activity_ms: 3_000,
            run_ms: 20_000,
            sample_ms: 250,
        });
        stopped.stopped = Some(orbistoun_overrides::DELIBERATE_EXIT.to_owned());
        assert_eq!(
            super::describe_end(&stopped),
            orbistoun_overrides::DELIBERATE_EXIT,
            "a deliberate exit is not a silence, however quiet the run was"
        );
    }

    /// A spent call budget reads differently from the clock running out (D238), and comes
    /// from the branch that stopped the run, never from a call count.
    #[test]
    fn a_spent_budget_and_a_spent_clock_are_different_endings() {
        let mut budget = trace(47, 20_000_000, None, None);
        budget.ended_by = Some(super::SPENT_THE_BUDGET.to_owned());
        assert_eq!(super::describe_end(&budget), super::SPENT_THE_BUDGET);

        // The clock, with the same call count: the number is not what decides it.
        let mut clock = trace(47, 20_000_000, None, None);
        clock.ended_by = Some(super::RAN_TO_LIMIT.to_owned());
        assert_eq!(super::describe_end(&clock), super::RAN_TO_LIMIT);

        // A budget-ended run that also went quiet is still a budget ending.
        let mut both = trace(47, 20_000_000, None, None);
        both.ended_by = Some(super::SPENT_THE_BUDGET.to_owned());
        both.quiet = Some(super::Quiet {
            silent_ms: 17_000,
            last_activity_ms: 3_000,
            run_ms: 20_000,
            sample_ms: 250,
        });
        assert_eq!(super::describe_end(&both), super::SPENT_THE_BUDGET);

        // A guest that stopped itself outranks either: its own decision is not a limit.
        let mut exited = trace(47, 20_000_000, None, None);
        exited.ended_by = Some(super::SPENT_THE_BUDGET.to_owned());
        exited.stopped = Some(orbistoun_overrides::DELIBERATE_EXIT.to_owned());
        assert_eq!(
            super::describe_end(&exited),
            orbistoun_overrides::DELIBERATE_EXIT
        );

        // And a fault outranks everything, because the run did not reach a limit at all.
        let mut faulted = trace(47, 20_000_000, Some("image"), Some(0x1234));
        faulted.ended_by = Some(super::SPENT_THE_BUDGET.to_owned());
        assert_eq!(super::describe_end(&faulted), "image+0x1234");
    }

    /// A written frame reaches `Presented`; an unwritten flip stops at `Flipped`; and
    /// `frame_written` without an accepted flip, or below `Entered`, does not reach it.
    #[test]
    fn a_written_frame_reaches_presented_and_an_unwritten_flip_stops_at_flipped() {
        use orbistoun_overrides::Reach;

        let reach = |reached: &str, frames: u64, frame_written: bool| {
            let mut t = trace(47, 933, None, None);
            t.reached = reached.to_owned();
            t.frames = frames;
            t.frame_written = frame_written;
            super::status_of(&t, "2026-09-15".to_owned()).reach
        };

        // A flip whose buffer the guest wrote into is the one thing that reaches the top rung.
        assert_eq!(reach("Entered", 1, true), Reach::Presented);
        // The same run with nothing written stops one below, at flipped.
        assert_eq!(reach("Entered", 1, false), Reach::Flipped);
        // A written buffer that no port accepted a flip for is not a presented frame.
        assert_eq!(reach("Entered", 0, true), Reach::Entered);

        // The floor still decides: a frame written and flipped cannot lift a guest past a rung
        // it skipped, so nothing below `Entered` presents.
        for reached in [
            "Linked",
            "ImportsResolved",
            "ContainerParsed",
            "Rejected",
            "something nobody has written yet",
        ] {
            assert!(
                reach(reached, 53, true) < Reach::Presented,
                "{reached} with a written frame must not reach presented - it never entered",
            );
        }
    }

    /// A run that never entered is not promoted by a frame count: `reached` decides the floor.
    #[test]
    fn a_frame_count_cannot_lift_a_run_that_never_entered() {
        use orbistoun_overrides::Reach;

        let mut never_ran = trace(0, 0, None, None);
        never_ran.reached = "Linked".to_owned();
        never_ran.frames = 5;
        assert_eq!(
            super::status_of(&never_ran, "2026-09-04".to_owned()).reach,
            Reach::Linked,
            "five frames from a guest that never entered is a bug in the count, not a rung"
        );
    }

    fn trace(distinct: usize, calls: u64, region: Option<&str>, offset: Option<u64>) -> CallTrace {
        CallTrace {
            forced_dumps: Vec::new(),
            ended_by: None,
            threads: Vec::new(),
            said: Vec::new(),
            quiet: None,
            title_modules: Vec::new(),
            module: "m".to_owned(),
            reached: "Entered".to_owned(),
            total_calls: calls,
            distinct,
            frames: 0,
            frame_written: false,
            submission: None,
            calls: Vec::new(),
            syscalls: Vec::new(),
            tail: Vec::new(),
            abi: super::AbiReport::default(),
            reads: super::ReadReport::default(),
            dumps: Vec::new(),
            conditions: Conditions::default(),
            formats: super::FormatReport::default(),
            stopped: None,
            fault: region.map(|region| FaultSite {
                instruction: Vec::new(),
                thread: None,
                host_thread: None,
                pointees: Vec::new(),
                kind: "read of".to_owned(),
                address: 0,
                instruction_pointer: offset.unwrap_or(0),
                region: Some(region.to_owned()),
                offset,
                inside_import: None,
                registers: None,
                frames: Vec::new(),
            }),
        }
    }

    #[test]
    fn further_says_what_moved_without_claiming_the_other_signal() {
        // The same imports reached and the fault further along the same function: the
        // summary must not claim new imports.
        let before = trace(23, 222, Some("image"), Some(0x00af_c959));
        let after = trace(23, 222, Some("image"), Some(0x00af_ca2e));
        let progress = compare(Some(&before), &after);
        assert_eq!(progress.verdict, Verdict::Further);
        assert!(
            !progress.verdict.summary().contains("imports"),
            "it reached the same imports: {}",
            progress.verdict.summary()
        );
    }

    #[test]
    fn a_first_run_says_so_rather_than_claiming_no_change() {
        // With nothing to compare against, the verdict is a first run, not "same".
        let now = trace(3, 10, Some("image"), Some(0x100));
        assert_eq!(compare(None, &now).verdict, Verdict::FirstRun);
    }

    #[test]
    fn reaching_more_imports_is_progress() {
        let before = trace(3, 10, Some("image"), Some(0x100));
        let after = trace(5, 20, Some("image"), Some(0x200));
        let seen = compare(Some(&before), &after);
        assert_eq!(seen.verdict, Verdict::Further);
        assert_eq!(seen.distinct_delta, 2);
        assert_eq!(seen.calls_delta, 10);
    }

    #[test]
    fn the_same_imports_but_a_later_fault_is_still_progress() {
        // The guest executed code it could not reach before (D129).
        let before = trace(3, 10, Some("image"), Some(0x100));
        let after = trace(3, 10, Some("image"), Some(0x900));
        assert_eq!(compare(Some(&before), &after).verdict, Verdict::Further);
    }

    #[test]
    fn more_interface_along_a_different_path_is_not_reported_as_a_regression() {
        // More interface reached behind an instruction pointer that went backwards on a
        // changed path is not BACK (D129).
        let before = trace(3, 10, Some("image"), Some(0x900));
        let after = trace(9, 40, Some("image"), Some(0x100));
        let seen = compare(Some(&before), &after);
        assert_eq!(seen.verdict, Verdict::MoreInterfaceDifferentPath);
        assert!(!seen.verdict.is_progress() || seen.distinct_delta > 0);
    }

    #[test]
    fn positions_in_different_regions_are_not_compared() {
        // Instruction pointers on different code paths are not ordered.
        let before = trace(3, 10, Some("image"), Some(0x900));
        let after = trace(3, 10, Some("stubs"), Some(0x100));
        assert_eq!(compare(Some(&before), &after).verdict, Verdict::Same);
    }

    #[test]
    fn a_run_that_hit_the_time_limit_is_described_as_such() {
        // No fault position at all is not the same as dying at address zero.
        let after = trace(3, 10, None, None);
        assert_eq!(compare(None, &after).fault, "ran to the time limit");
    }

    #[test]
    fn every_verdict_has_a_summary_a_person_can_read() {
        // Every verdict has a summary, shared by both shims (D034).
        for verdict in [
            Verdict::FirstRun,
            Verdict::Further,
            Verdict::Back,
            Verdict::Same,
            Verdict::MoreInterfaceDifferentPath,
            Verdict::FurtherButNarrower,
        ] {
            assert!(!verdict.summary().is_empty());
        }
    }

    /// A trace with a policy and a limit attached.
    fn under(conditions: Conditions) -> CallTrace {
        CallTrace {
            conditions,
            ..trace(10, 100, Some("image"), Some(0x100))
        }
    }

    #[test]
    fn loosening_the_stub_policy_is_reported_as_a_settings_change() {
        // Answering `ok` for unimplemented functions improves every number without
        // implementing anything, so the verdict renders qualified by the policy change.
        let before = under(Conditions {
            default_return: "unimplemented".to_owned(),
            ..Conditions::default()
        });
        let after = CallTrace {
            distinct: 12,
            conditions: Conditions {
                default_return: "ok".to_owned(),
                ..Conditions::default()
            },
            ..under(Conditions::default())
        };

        let progress = compare(Some(&before), &after);
        assert_eq!(
            progress.verdict,
            Verdict::Further,
            "the numbers really did move"
        );
        assert!(
            progress
                .conditions_changed
                .iter()
                .any(|c| c.contains("answer ok")),
            "and the report must say why: {:?}",
            progress.conditions_changed
        );
    }

    #[test]
    fn the_wall_clock_limit_is_a_condition_because_it_measures_the_host() {
        // A faster host reaches further in the same wall-clock limit, so a limit change is
        // a difference.
        let before = under(Conditions {
            limit_seconds: Some(10),
            call_budget: None,
            did_nothing: Vec::new(),
            ..Conditions::default()
        });
        let after = under(Conditions {
            limit_seconds: Some(30),
            call_budget: None,
            did_nothing: Vec::new(),
            ..Conditions::default()
        });

        let changed = compare(Some(&before), &after).conditions_changed;
        assert_eq!(changed.len(), 1);
        assert!(changed[0].contains("10s") && changed[0].contains("30s"));
    }

    #[test]
    fn the_build_is_recorded_but_never_compared() {
        // The build changes with every release, so it is recorded but not compared.
        let before = under(Conditions {
            build: "0.1.0".to_owned(),
            ..Conditions::default()
        });
        let after = under(Conditions {
            build: "0.2.0".to_owned(),
            ..Conditions::default()
        });

        assert!(compare(Some(&before), &after).conditions_changed.is_empty());
    }

    #[test]
    fn an_unchanged_setup_leaves_the_verdict_unqualified() {
        // Identical conditions produce no caveat.
        let conditions = Conditions {
            experiments: String::new(),
            intervened: false,
            memory_map: Vec::new(),
            limit_seconds: Some(20),
            call_budget: None,
            did_nothing: Vec::new(),
            default_return: "unimplemented".to_owned(),
            overrides: 3,
            propping: 3,
            build: "0.1.0".to_owned(),
        };
        let before = under(conditions.clone());
        let after = under(conditions);

        assert!(compare(Some(&before), &after).conditions_changed.is_empty());
    }

    #[test]
    fn a_first_run_has_nothing_to_have_changed_against() {
        assert!(
            compare(None, &under(Conditions::default()))
                .conditions_changed
                .is_empty()
        );
    }

    /// An unchanged trace file reads as "no trace from this run", never as a measurement
    /// (D470).
    #[test]
    fn a_trace_that_was_not_rewritten_is_not_this_run_s() {
        let stamp = super::Stamp {
            modified: std::time::UNIX_EPOCH,
            len: 16_164,
        };
        assert!(
            !super::wrote_a_trace(Some(stamp), Some(stamp)),
            concat!(
                "an untouched file is the previous run's, and reporting it as this run's ",
                "is the failure this exists to stop"
            )
        );
    }

    /// The same length at a different time, and the same time at a different length, are
    /// both rewrites.
    #[test]
    fn either_half_of_the_stamp_changing_is_a_rewrite() {
        let was = super::Stamp {
            modified: std::time::UNIX_EPOCH,
            len: 16_164,
        };
        let later = super::Stamp {
            modified: std::time::UNIX_EPOCH + std::time::Duration::from_secs(1),
            ..was
        };
        let longer = super::Stamp { len: 17_000, ..was };
        assert!(super::wrote_a_trace(Some(was), Some(later)));
        assert!(super::wrote_a_trace(Some(was), Some(longer)));
    }

    /// A first run has nothing before it and is still a real measurement.
    #[test]
    fn the_first_trace_a_module_ever_writes_counts() {
        let now = super::Stamp {
            modified: std::time::UNIX_EPOCH,
            len: 1,
        };
        assert!(super::wrote_a_trace(None, Some(now)));
    }

    /// No file afterwards is no measurement, whatever came before.
    #[test]
    fn no_trace_afterwards_is_never_a_measurement() {
        let was = super::Stamp {
            modified: std::time::UNIX_EPOCH,
            len: 1,
        };
        assert!(!super::wrote_a_trace(None, None));
        assert!(!super::wrote_a_trace(Some(was), None));
    }

    #[test]
    fn only_calls_nothing_implements_count_against_the_total() {
        // 60 of 100 calls reached real code, so the run stands on 40% placeholder.
        let trace = CallTrace {
            total_calls: 100,
            calls: vec![
                CalledImport {
                    index: 0,
                    label: "libc::memset".to_owned(),
                    calls: 60,
                    implemented: true,
                    shape: String::new(),
                },
                CalledImport {
                    index: 1,
                    label: "libc::snprintf_s".to_owned(),
                    calls: 40,
                    implemented: false,
                    shape: String::new(),
                },
            ],
            ..under(Conditions::default())
        };

        assert_eq!(trace.stubbed_calls(), 40);
        assert_eq!(trace.stubbed_share(), 40);
    }

    #[test]
    fn a_run_that_called_nothing_stands_on_nothing_rather_than_dividing_by_zero() {
        let trace = CallTrace {
            total_calls: 0,
            calls: Vec::new(),
            ..under(Conditions::default())
        };
        assert_eq!(trace.stubbed_share(), 0);
    }

    #[test]
    fn answering_blindly_is_anything_other_than_reporting_unimplemented() {
        // A raw code is a specific answer rather than the default, but a run under one still
        // stands on something unproved.
        assert!(
            !Conditions::default().answers_blindly(),
            "unset says nothing"
        );
        for spelling in ["ok", "0x8002000e"] {
            assert!(
                Conditions {
                    default_return: spelling.to_owned(),
                    ..Conditions::default()
                }
                .answers_blindly()
            );
        }
        assert!(
            !Conditions {
                default_return: "unimplemented".to_owned(),
                ..Conditions::default()
            }
            .answers_blindly()
        );
    }

    /// A verdict taken under a different call budget is labelled as one.
    ///
    /// A different budget changes how far a guest reaches regardless of the build (D238).
    #[test]
    fn a_changed_call_budget_is_reported_as_a_difference() {
        let before = Conditions {
            call_budget: Some(10_000_000),
            ..Conditions::default()
        };
        let after = Conditions {
            call_budget: Some(20_000_000),
            ..Conditions::default()
        };
        let said = after.differences_from(&before);
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(said[0].contains("10000000"), "{said:?}");
        assert!(said[0].contains("20000000"), "{said:?}");

        // Identical budgets are not a difference.
        assert!(after.differences_from(&after).is_empty());
    }

    /// Having no budget and having one are different conditions, not the same one.
    #[test]
    fn an_absent_budget_differs_from_a_present_one() {
        let none = Conditions::default();
        let some = Conditions {
            call_budget: Some(20_000_000),
            ..Conditions::default()
        };
        assert_eq!(some.differences_from(&none).len(), 1);
    }

    /// A run that stopped faulting is not a run that went backwards (D301).
    #[test]
    fn a_run_that_stopped_faulting_is_not_reported_as_going_backwards() {
        let before = trace(23, 222, Some("image"), Some(0x00af_c959));
        let after = trace(23, 222, None, None);
        let progress = compare(Some(&before), &after);

        assert_ne!(
            progress.verdict,
            Verdict::Back,
            "the wall it hit last time is gone: {}",
            progress.verdict.summary()
        );
        assert!(
            progress.ended_without_a_fault,
            "and the report has to say the position measured nothing"
        );
    }

    /// A run that started faulting did not thereby get further (D301).
    #[test]
    fn a_run_that_began_faulting_is_not_reported_as_further() {
        let before = trace(23, 222, None, None);
        let after = trace(23, 222, Some("image"), Some(0x00af_c959));
        let progress = compare(Some(&before), &after);

        assert_ne!(
            progress.verdict,
            Verdict::Further,
            "it reached the same imports and acquired a fault: {}",
            progress.verdict.summary()
        );
        assert!(
            !progress.ended_without_a_fault,
            "this one did fault, and the flag is about this run"
        );
    }

    /// The surface signal still decides when there is no position to compare.
    ///
    /// More imports is progress whether or not anything faulted.
    #[test]
    fn reaching_more_imports_is_still_progress_with_no_fault_either_side() {
        let before = trace(3, 10, None, None);
        let after = trace(5, 20, None, None);
        let progress = compare(Some(&before), &after);

        assert_eq!(progress.verdict, Verdict::Further);
        assert_eq!(progress.distinct_delta, 2);
        assert!(progress.ended_without_a_fault);
    }
}

#[cfg(test)]
mod ladder {
    use super::{CallTrace, status_of};
    use orbistoun_overrides::DELIBERATE_EXIT;
    use orbistoun_overrides::Reach;

    /// A trace with the fields this rung turns on, and defaults for the rest.
    fn ran(frames: u64, stopped: Option<&str>) -> CallTrace {
        let mut trace: CallTrace = serde_json::from_str(
            r#"{"module":"x","reached":"Entered","total_calls":9,"distinct":3,"calls":[]}"#,
        )
        .expect("parses");
        trace.frames = frames;
        trace.stopped = stopped.map(ToOwned::to_owned);
        trace
    }

    fn reach_of(frames: u64, stopped: Option<&str>) -> Reach {
        status_of(&ran(frames, stopped), "2026-09-14".to_owned()).reach
    }

    /// Leaving by `exit` is further than merely having run.
    #[test]
    fn a_deliberate_exit_outranks_having_only_entered() {
        assert_eq!(reach_of(0, Some(DELIBERATE_EXIT)), Reach::Exited);
        assert_eq!(reach_of(0, None), Reach::Entered);
        assert!(Reach::Exited > Reach::Entered);
    }

    /// A guest that flipped and then exited is recorded as flipped; the deliberate stop is
    /// carried in the outcome.
    #[test]
    fn a_frame_outranks_a_deliberate_exit() {
        assert!(Reach::Flipped > Reach::Exited);
        assert_eq!(reach_of(8, Some(DELIBERATE_EXIT)), Reach::Flipped);
    }

    /// Only exiting earns the rung; an abort or an unhandled signal does not.
    #[test]
    fn giving_up_does_not_earn_the_rung() {
        assert_eq!(reach_of(0, Some("the guest called abort")), Reach::Entered);
        assert_eq!(
            reach_of(
                0,
                Some("the guest raised a signal nothing was installed to handle")
            ),
            Reach::Entered
        );
        assert_eq!(reach_of(0, Some("ran to the time limit")), Reach::Entered);
    }

    /// A deliberate exit beats a fault at equal reach, even on fewer imports, so the
    /// tiebreaker sits above `imports` (D686).
    #[test]
    fn a_deliberate_exit_beats_a_fault_at_equal_reach() {
        let exited = status_of(&ran(8, Some(DELIBERATE_EXIT)), "2026-09-14".to_owned());

        let mut faulted_trace = ran(8, None);
        faulted_trace.distinct += 1;
        faulted_trace.total_calls += 6;
        // Built from JSON so the test states only the fields it needs: a fault at a small
        // address.
        faulted_trace.fault = Some(
            serde_json::from_str(r#"{"kind":"read","address":24109,"instruction_pointer":24109}"#)
                .expect("a fault site"),
        );
        let faulted = status_of(&faulted_trace, "2026-09-14".to_owned());

        assert!(faulted.outcome != DELIBERATE_EXIT, "the other run faulted");
        assert!(
            exited.beats(&faulted),
            "stopping correctly must not read as a regression"
        );
        assert!(!faulted.beats(&exited), "and it is not symmetric");
    }

    /// The exit rung sits below `Flipped`: `ranking_key` compares the rung first, so a
    /// program that only calls `exit(0)` must not rank above a title presenting frames.
    #[test]
    fn a_trivial_exit_does_not_outrank_a_title_that_rendered() {
        let trivial = status_of(&ran(0, Some(DELIBERATE_EXIT)), "2026-09-14".to_owned());

        let mut rendered_trace = ran(9, None);
        rendered_trace.distinct = 223;
        rendered_trace.total_calls = 432_211;
        let rendered = status_of(&rendered_trace, "2026-09-14".to_owned());

        assert!(
            !trivial.beats(&rendered),
            "a guest that exited having done nothing must not outrank one that rendered"
        );
        assert!(rendered.beats(&trivial), "and the richer run wins");
    }
}
