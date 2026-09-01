//! What a guest asked for, and how one run compares with the last.
//!
//! # Why these live here rather than with the fault handler that fills them
//!
//! They were in `orbistoun-worker`, next to the code that produces them, which reads as
//! the obvious place until a second shim needs them. `orbistoun-worker` sits *above*
//! `orbistoun-service` in the spine, so nothing in the service layer could see these
//! types - and the comparison logic that turns two of them into a verdict ended up in
//! `orbistoun-cli`, where the GUI cannot reach it either.
//!
//! That is principle 13's exact warning arriving on schedule: a shim had started holding
//! logic, and it was invisible while there was only one shim to notice. Moving the data
//! down the spine is what lets the orchestration live in one place (D160).
//!
//! The *producing* side - the fault handler, the region table, the allocation-free line
//! writer - stays in the worker. Only the shapes and the pure comparison move.

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
    /// Every import called, most-used first.
    pub calls: Vec<CalledImport>,
    /// Every system call the guest asked the kernel for **directly**, not through a stub.
    ///
    /// # Why this is a field of its own and not part of `calls`
    ///
    /// A guest that reaches the kernel by number never touches an import, so it leaves no mark
    /// on the ranked list at all. The open-toolchain payloads work exactly that way: they
    /// resolve one function to build a gadget and then go straight to the kernel, so a run that
    /// stopped dead on an unimplemented call could report *no imports of interest* and be
    /// telling the truth (D401).
    ///
    /// Folding them into `calls` would have been shorter and wrong twice: `distinct` means
    /// distinct **imports** and every report that prints it says so, and a syscall has no stub
    /// index to be indexed by.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub syscalls: Vec<AskedSyscall>,
    /// The last calls the guest made, in the order it made them.
    ///
    /// **The neighbourhood of the wall.** A ranked list says what a guest spends its time
    /// on, which is the right question for deciding what to implement - and the wrong one
    /// entirely when something has just handed it a null and it died. For that, the only
    /// useful question is what it called *last*, and the ranked list cannot answer it at
    /// any length (D154).
    ///
    /// The ordering was always recorded; it just never left the process.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tail: Vec<TracedCall>,
    /// How the guest's calls measured against the calling convention.
    ///
    /// Always present, so "no misaligned calls" is a *measurement* rather than a silence.
    /// A field that only appears when something is wrong is indistinguishable from a
    /// field nobody wired up (D159).
    #[serde(default)]
    pub abi: AbiReport,
    /// How file reads went.
    ///
    /// Always present, so "no short reads" is a measurement rather than a silence - the
    /// same rule the stack-conformance line follows (D175).
    #[serde(default)]
    pub reads: ReadReport,
    /// How formatted writes went.
    ///
    /// Always present once anything was formatted, so "nothing refused" is a measurement
    /// rather than a silence - the same rule the read and stack lines follow (D175).
    #[serde(default)]
    pub formats: FormatReport,
    /// What the guest was pointing at, for calls nothing implements.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dumps: Vec<ArgumentDump>,
    /// What the run was subject to.
    ///
    /// Recorded so that two traces can be told apart from two *measurements* - see
    /// [`Conditions`].
    #[serde(default)]
    pub conditions: Conditions,
    /// Why the guest stopped itself, if it did.
    ///
    /// **A third outcome, and it was being reported as the second.** A run ends by
    /// faulting, by being stopped from outside when it runs out of time, or by the guest
    /// deciding to stop. With no field for the third, a guest that called `abort` was
    /// described as having "run to the time limit" - which is not merely imprecise, it is
    /// the opposite of what happened (D177).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stopped: Option<String>,
    /// Where it died, if it did.
    ///
    /// **The progress measure.** How far a guest got before faulting is the one number
    /// that says whether a change helped: an instruction pointer that moved forward
    /// means the guest executed code it could not reach before, and nothing else in a
    /// run reports that as directly (D080).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fault: Option<FaultSite>,
}

/// How many calls of context to keep before the end.
///
/// Enough to see past a burst of one repeated function - a guest clearing memory calls
/// `memset` hundreds of times in a row, and a shorter tail would show nothing but that.
pub const TAIL_CALLS: usize = 48;

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
    /// The stack pointer it arrived with, whole - the full value says which region the
    /// stack was in, which a remainder alone cannot.
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
    /// The first integer argument, as it arrived.
    ///
    /// Kept because at a wall it is often the whole answer: a guest passing an address it
    /// was handed a moment earlier makes the chain visible without any other tooling.
    pub arg0: u64,
    /// The guest address this call returns to - one instruction past the call site.
    ///
    /// **The same address space a fault's frame walk reports**, which is the point: a
    /// stack trace and a call trace can be read against each other, and a frame stops
    /// being a bare number the moment an import was called from it (D173).
    #[serde(default)]
    pub from: u64,
    /// What this call **answered** in `rax`, when it had returned before the trace was read.
    ///
    /// **The half of a call the tail never carried.** `arg0` is what the guest passed *in*;
    /// this is what our implementation handed *back* - and at the walls this project hits,
    /// the wrong value is almost always the one we answered, not the one we were given (the
    /// D125 class). A tail that showed the call and not its result could not see that at all
    /// (D459).
    ///
    /// [`None`] when the answer was not known: a call still running, or - the common case
    /// at a fault - one whose guest crashed in its own code the instant the call returned.
    /// Kept distinct from `Some(0)`, because zero is `OK` and saying "unknown" is not the
    /// same as saying "it succeeded".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub returned: Option<u64>,
}

/// Where a guest faulted.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FaultSite {
    /// What it was doing - a read, a write, an instruction fetch.
    ///
    /// Not every value here describes touching an address. See
    /// [`FaultSite::touched_an_address`], which is the question most callers actually
    /// have, and [`FaultSite::TOUCHED`] for the exhaustive list.
    pub kind: String,
    /// The address it touched.
    pub address: u64,
    /// The instruction that touched it. This is the number that measures progress.
    pub instruction_pointer: u64,
    /// Which region the instruction was in, when it is one orbistoun placed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    /// Offset into that region.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<u64>,
    /// The import the guest was inside when it faulted, if it was inside one.
    ///
    /// **The question a fault in host code cannot otherwise answer.** An instruction
    /// pointer outside every placed region says only "this is ours, not the guest's",
    /// which narrows the search to the whole emulator. Naming the import narrows it to one
    /// function (D158).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inside_import: Option<String>,
    /// The guest's own call path, innermost first.
    ///
    /// Empty when the chain could not be walked, which is the ordinary case for optimised
    /// code that omits the frame pointer - not a failure (D172).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub frames: Vec<Frame>,
    /// The registers as they were at the fault.
    ///
    /// Captured because the handler has always had them - a vectored handler is passed the
    /// full context record - and threw them away. A stack pointer alone distinguishes
    /// "the guest ran out of stack" from "a pointer was wrong", which was guesswork
    /// before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registers: Option<Registers>,
}

impl FaultSite {
    /// Kinds whose [`address`](Self::address) is somewhere the guest actually touched.
    ///
    /// An access violation carries the address in its exception parameters, so the number
    /// means what it looks like it means.
    pub const TOUCHED: [&'static str; 3] = ["write to", "read of", "instruction fetch from"];

    /// Kinds whose [`address`](Self::address) is **the faulting instruction itself**.
    ///
    /// These exceptions carry no address parameters, so the reporter fills the field with
    /// the instruction pointer. The number is real, and it is not somewhere the guest
    /// asked for - so comparing it across runs answers a question nobody asked.
    pub const AT_THE_INSTRUCTION: [&'static str; 3] = [
        "illegal instruction at",
        "breakpoint - stub padding - at",
        "stack overflow at",
    ];

    /// Whether [`address`](Self::address) is somewhere the guest asked for.
    ///
    /// **Worth its own method because getting it wrong reads as a result.** A sweep
    /// planting sentinels at an argument compares where the guest faulted, run to run. Do
    /// that against an illegal instruction and both sentinels produce the *same* address -
    /// the instruction pointer - so the fault appears to have moved somewhere unrelated to
    /// what was planted, and gets reported as an inconsistent move rather than as the
    /// plant having broken control flow. Measured on a live title: planting at `arg1` of
    /// one import derailed the guest into non-code at a fixed address, and the sweep
    /// called it `Moved`.
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
/// **The question a call trace cannot otherwise answer.** A trace says an unimplemented
/// function was called and with what first argument; it does not say what that argument
/// *was*, and for an out-parameter or a descriptor struct that is the whole of the
/// information. `sceKernelDirectMemoryQuery` was understood because the guest passed a
/// structure size and somebody read it by hand (D083); this is that, automatically.
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
    /// Empty for a scalar - a size, a flag, a count - which is evidence in itself and was
    /// invisible while only pointers were recorded (D198).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub bytes: String,
    /// The same bytes as text, where they read as one - a name is worth more than its hex.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
}

/// What formatted writes managed.
///
/// **Reported because "implemented" and "answered correctly" are different claims**, and
/// nothing else in a run can tell them apart. A formatted write that refuses hands the
/// guest an empty string; the call still counts as reaching an implementation, so the
/// standing figure rises and the guest is no better off. Only this says so (D183).
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

/// What a run was subject to, as opposed to what it found.
///
/// # Why a verdict without this is not evidence
///
/// The whole loop rests on one inference: run, change one thing, run again, attribute the
/// difference to the change. That is valid only if everything *else* was identical, and
/// nothing recorded whether it was. Two settings break it in opposite directions and both
/// are one line of TOML away.
///
/// **The time limit is wall-clock**, so the same build on the same title reaches further on
/// a faster machine. Two contributors comparing runs are comparing their hardware. That
/// matters more the moment results are shared rather than kept.
///
/// **The stub policy decides what unimplemented functions answer.** Loosening it to `ok`
/// makes every number improve at once - the guest stops checking, runs on, and dies much
/// later - while nothing whatever has been implemented. It is the highest-scoring one-line
/// change available to anything optimising a call count, which is precisely why it has to
/// be visible in the comparison rather than inferred from a changelog.
///
/// Recorded rather than forbidden. Answering `ok` everywhere is a legitimate bisection
/// technique and the loop depends on being able to try it (principle 5); what makes it a
/// hack is doing it *unlabelled*.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Conditions {
    /// The wall-clock limit in seconds, or `None` for no limit.
    ///
    /// **A backstop, not the measurement.** It fixes the duration and lets the call count
    /// vary, and the call count is what a verdict is read off - three identical runs of one
    /// title returned 77.5M, 75.8M and 87.6M calls. What it catches that a budget cannot is
    /// a guest that stops calling imports altogether (D238).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit_seconds: Option<u64>,
    /// Diagnostics that were asked for and did nothing at all.
    ///
    /// **The failure this whole family keeps having.** A diagnostic that never reached the
    /// thing under test and one that reached it and changed nothing produce identical
    /// output - an ordinary-looking run - and only the second is a measurement. Two
    /// recorded eliminations turned out to be the first kind (D229, D230), and each was
    /// believed for weeks.
    ///
    /// So a diagnostic that applied zero times is recorded as having applied zero times,
    /// and the report says so where the verdict is read rather than in a line above it
    /// (D241).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub did_nothing: Vec<String>,
    /// Imports the guest was allowed to call, or `None` for no budget.
    ///
    /// The deterministic half: two runs of one build stop at the same call, so a verdict
    /// between them measures the change rather than the machine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_budget: Option<u64>,
    /// What a function with no implementation answered, spelled as the policy spells it.
    ///
    /// A string rather than the policy type: this crate sits below the one that defines it,
    /// and the value is for a person and a diff to read, not to act on.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub default_return: String,
    /// How many functions were given an explicit answer instead of the default.
    #[serde(default)]
    pub overrides: usize,
    /// Every diagnostic the run was put under, or empty for an ordinary run.
    ///
    /// **A run under a diagnostic is answering a different question** - "does this depend on
    /// memory nobody wrote?", "is this argument an out-parameter?" - rather than "how far
    /// does it get?". Comparing the two as though they measured the same thing is
    /// meaningless, and this is what stops it (D185, D218).
    ///
    /// One field rather than one per diagnostic. There were two, a third was never recorded
    /// at all, and five more were wanted - which is the shape that drifts (D220).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub experiments: String,
    /// The physical memory map the guest was shown, region by region.
    ///
    /// **A run that cannot say what map it presented cannot answer a question about maps.**
    /// The offsets a guest queries only mean something against the boundaries it was given,
    /// and those were computed inside the emulator and thrown away - so a reader comparing
    /// them had to know which shape was configured and recompute it, which is a second copy
    /// of the thing being measured (D357).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub memory_map: Vec<(u64, u64, bool)>,
    /// Whether any of them **changed the program** rather than only observing it.
    ///
    /// Recorded separately from the words above because it is acted on rather than read:
    /// a verdict earned under an intervention carries a warning, and parsing that back out
    /// of prose would be a second place for the rule to live (D227).
    #[serde(default)]
    pub intervened: bool,
    /// The build that produced the trace.
    ///
    /// Recorded but deliberately **not** compared: it changes on every release and would
    /// fire constantly, drowning the two conditions that actually change what a run does.
    /// It is here for a result contributed by somebody whose tree you cannot see.
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
    /// Empty means the two are comparable. Sentences rather than a struct because the
    /// only consumer is a line of output whose whole job is to stop somebody trusting a
    /// verdict that measures a settings change.
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
/// A fault address says *where* the guest died; it does not say who called it. At the top
/// of a function - which is where a null dereference usually lands - the instruction
/// pointer alone is nearly content-free.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Frame {
    /// Where this frame returns to - the call site, one instruction past the call.
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
    /// Every register, as lines to print under a fault.
    ///
    /// **All sixteen, not the four that usually matter.** The short form named `rax`,
    /// `rcx`, `rdx` and `rdi` because those carry an address or a size in most faults - and
    /// at the `image+0xafc959` wall the question was *which register held the base that
    /// should not have been zero*, which the four cannot answer. The values were captured
    /// and recorded all along; only the last step threw them away, so a run had to be
    /// repeated to learn something already sitting in its own trace (D230).
    ///
    /// Grouped four to a line because sixteen on one line wraps in a terminal, and a
    /// wrapped register dump is read wrongly.
    pub fn lines(&self) -> Vec<String> {
        let all = [
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
        ];
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
    /// Its first argument, kept because for a call nobody can name it is most of what
    /// there is to go on - `649(2, ...)` narrows what a thing might be in a way `649` does not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_argument: Option<u64>,
}

/// One import a guest called, and whether anything answered it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CalledImport {
    /// Dynamic symbol index - the stub it landed on.
    pub index: usize,
    /// Library and name, or library and hash when no name is known yet.
    pub label: String,
    /// How many times.
    pub calls: u64,
    /// Whether anything actually implements it, or whether it landed on a stub.
    ///
    /// **The most directly actionable fact a run reports.** Without it a trace cannot tell
    /// "the guest used this and it worked" from "the guest used this and got a
    /// placeholder", which are opposite conclusions drawn from the same line (D179).
    #[serde(default)]
    pub implemented: bool,
}

impl CallTrace {
    /// Calls that landed on something with no implementation behind them.
    ///
    /// **The discount on the headline number.** A call count is progress only to the extent
    /// the calls were answered by something real; the rest is the guest proceeding on the
    /// strength of a placeholder. Both are worth knowing and they are not the same thing,
    /// so neither is reported without the other (D181).
    pub fn stubbed_calls(&self) -> u64 {
        self.calls
            .iter()
            .filter(|c| !c.implemented)
            .map(|c| c.calls)
            .sum()
    }

    /// What share of the run rested on stubs, as a percentage.
    ///
    /// Zero when nothing was called, rather than a division by zero - a run that made no
    /// calls borrowed nothing, which is the honest reading.
    pub fn stubbed_share(&self) -> u32 {
        if self.total_calls == 0 {
            return 0;
        }
        u32::try_from(self.stubbed_calls().saturating_mul(100) / self.total_calls).unwrap_or(100)
    }
}

/// What a run says about a title, for the compatibility record.
///
/// **Derived, never typed.** A compatibility database whose grades are written by hand
/// drifts from what the tool observed the moment somebody is optimistic, and the drift is
/// invisible because there is nothing to check a hand-written grade against. Every field
/// here comes off the trace, so an entry is a transcription of a measurement rather than
/// an opinion about one.
///
/// `measured_on` is passed in because this crate has no clock and should not grow one -
/// a date fetched here would also make the function untestable.
pub fn status_of(trace: &CallTrace, measured_on: String) -> orbistoun_overrides::Status {
    use orbistoun_overrides::Reach;

    // The ladder is read off what actually happened, in the order the phases occur. A
    // guest that faulted still *entered*; that it then died - or survived to the limit -
    // is the outcome, not the distance, and `outcome` carries it.
    let reach = match trace.reached.as_str() {
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
        limit_seconds: trace.conditions.limit_seconds,
        build: trace.conditions.build.clone(),
        measured_on,
        notes: String::new(),
    }
}

/// The file name a module's trace is written to.
///
/// **Declared once because two places need it and they must agree.** The worker writes
/// the file and a shim reads it back to compare runs; if the two ever computed the name
/// differently the comparison would find nothing, report "first run of this module"
/// forever, and never once look wrong (D084).
///
/// The last two path components, because a bare `eboot.bin` is the same name in every
/// title and would have them all overwriting one file.
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
/// `None` covers both "no previous run" and "the file is unreadable or stale-format", and
/// deliberately does not distinguish them: the caller's next move is the same either way,
/// and a first run is not an error.
pub fn load_previous(traces_dir: &std::path::Path, module: &std::path::Path) -> Option<CallTrace> {
    let name = trace_file_name(&module.to_string_lossy());
    let text = std::fs::read_to_string(traces_dir.join(name)).ok()?;
    serde_json::from_str(&text).ok()
}

/// How this run compares with the last one of the same module.
///
/// **Two signals, deliberately.** Reporting one hid a run that reached eight more
/// subsystems behind an instruction pointer that had gone backwards, because the code
/// path had changed underneath it (D129).
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
    /// More of the interface, but along a different path - the positions do not compare.
    MoreInterfaceDifferentPath,
    /// Further along its path, but reaching less of the interface.
    FurtherButNarrower,
}

impl Verdict {
    /// The short label a shim prints beside the summary.
    ///
    /// Loud for the ones that matter and quiet for the ones that do not: `FURTHER` is the
    /// only thing this project is trying to produce, so it should be findable by eye in a
    /// wall of terminal output.
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
    /// Here rather than in a shim so the CLI and the GUI cannot describe the same
    /// measurement differently - which is the whole reason these moved (D160).
    pub const fn summary(self) -> &'static str {
        match self {
            Self::FirstRun => "first run of this module - nothing to compare against yet",
            // **Both causes, because there are two.** `Further` fires either for more of
            // the interface *or* for the same interface with the fault further along, and
            // this said "reached imports it could not reach before" for both - which reads
            // as a falsehood next to a `(+0)` distinct count, in the one line this project
            // steers by. Executing code it could not reach before is true of either, and is
            // how D080 states the measure in the first place (D224).
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
    /// **A `FURTHER` under an intervention is not a diagnosis.** A diagnostic that only
    /// *observes* leaves the program alone, so a verdict under it measures the emulator. One
    /// that *intervenes* - a poked value, a poisoned region, a reservation the guest never
    /// asked for - changes the program being measured, so the guest may be getting further
    /// on an answer that is simply wrong.
    ///
    /// This exists because that mistake was made here: a mapping moved a wall, the movement
    /// was read as confirming the hypothesis that motivated the mapping, and watching what
    /// the guest *wrote* one run later said the opposite (D224, D226).
    pub bought_under_intervention: bool,
    /// Whether this run ended without faulting at all.
    ///
    /// **Then reach has stopped measuring progress.** `Further` rests on the interface count
    /// alone here, so it means "reached more imports", not "got further" - and this project
    /// steers by that one word, so the difference belongs on screen rather than inferred
    /// (D301).
    ///
    /// The comparison used to invent a position rather than admit it had none: a run that
    /// faulted where the last one had not scored as *further along*, and one that stopped
    /// faulting scored as *back*. Both are fabrications, and both were unasserted - while
    /// `describe_end` a few lines above already refuses to pretend a missing fault is an
    /// address. The two halves of this file disagreed about the same absent value (D309).
    pub ended_without_a_fault: bool,
    /// What changed about the *run* rather than about the emulator, in words.
    ///
    /// **A verdict with this non-empty is not evidence.** The comparison still renders,
    /// because the numbers are real and refusing to show them helps nobody - but it
    /// measures a settings change, and anything reading it has to be told so rather than
    /// left to infer it from a changelog (D181).
    pub conditions_changed: Vec<String>,
}

/// Describes where a run ended, for comparison and for display.
///
/// A run that hit the time limit has no fault position at all, and saying so is not the
/// same as saying it died at address zero.
fn describe_end(trace: &CallTrace) -> String {
    if let Some(fault) = &trace.fault {
        return match (&fault.region, fault.offset) {
            (Some(region), Some(offset)) => format!("{region}+{offset:#x}"),
            _ => format!("{:#x}", fault.instruction_pointer),
        };
    }
    // The guest stopping itself is a *decision*, not a failure, and describing it as a
    // time limit reports the opposite of what happened.
    trace
        .stopped
        .clone()
        .unwrap_or_else(|| "ran to the time limit".to_owned())
}

/// Compares a run against the one before it.
///
/// Pure, so the verdict is testable without running a guest - which matters because this
/// is the only measure of progress the project has, and a bug in it would silently
/// mis-rank every change made from here on (D080).
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
    // Positions only compare when both runs died in the same place-kind; an instruction
    // pointer from a different code path is a different number about a different thing.
    let position = match (&after.fault, &before.fault) {
        (Some(a), Some(b)) if a.region == b.region => Some(a.offset.cmp(&b.offset)),
        // **A missing fault is not a position, in either direction.** A run that did not
        // fault has no place it got to; ordering it against one that did is comparing a
        // number with its absence. The surface count still measures something and decides
        // the verdict alone from here (D309).
        _ => None,
    };

    // The two signals disagreeing is a real case with its own answer, so it is matched
    // first; after that either signal moving alone decides it.
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
        // Only when the verdict is progress. An intervention that changed nothing needs no
        // caveat, and a caveat on every instrumented run would be noise people learn to
        // scroll past - which is how a warning stops working (D227).
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

    /// **A trace written before this field existed still loads.**
    ///
    /// The traces directory is not wiped between versions - the work list reads every file in
    /// it, including ones written weeks ago. A field without a default turns those into parse
    /// errors, and `cmd_worklist` skips what it cannot parse *with a note rather than a
    /// failure*, so the whole history would have quietly stopped counting.
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

    /// A recorded answer of **zero** survives, and is not confused with absence: zero is
    /// `OK`, the commonest answer, and losing it would blind the tail to every successful
    /// call (D459).
    #[test]
    fn a_zero_answer_survives_and_is_not_absence() {
        let mut trace = base();
        trace.tail = vec![TracedCall {
            sequence: 3,
            label: "libkernel::sceKernelMapDirectMemory".to_owned(),
            arg0: 0x6000_0080_0d28,
            from: 0x4000_0159_6189,
            returned: Some(0),
        }];
        let text = serde_json::to_string(&trace).expect("serialises");
        let back: CallTrace = serde_json::from_str(&text).expect("parses back");
        assert_eq!(back.tail[0].returned, Some(0));
    }

    /// An **unknown** answer writes nothing, so a reader can never read it back as zero -
    /// "we did not see what it returned" and "it returned zero" are opposite claims.
    #[test]
    fn an_unknown_answer_is_absent_rather_than_zero() {
        let mut trace = base();
        trace.tail = vec![TracedCall {
            sequence: 1,
            label: "libc::strlen".to_owned(),
            arg0: 0x10,
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

    /// A trace written before the field existed still loads, reading as unknown - the traces
    /// directory is not wiped between versions and the work list reads every file in it.
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

    fn trace(distinct: usize, calls: u64, region: Option<&str>, offset: Option<u64>) -> CallTrace {
        CallTrace {
            module: "m".to_owned(),
            reached: "Entered".to_owned(),
            total_calls: calls,
            distinct,
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
        // The case that exposed this: the same imports reached, and the fault further along
        // the same function. A summary naming *imports* would be a falsehood printed beside
        // a `(+0)` distinct count, in the line the whole project steers by (D224).
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
        // "same - nothing moved" against nothing is a lie that reads as a real result.
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
        // The guest executed code it could not reach before, which is the definition
        // this project uses (D080).
        let before = trace(3, 10, Some("image"), Some(0x100));
        let after = trace(3, 10, Some("image"), Some(0x900));
        assert_eq!(compare(Some(&before), &after).verdict, Verdict::Further);
    }

    #[test]
    fn more_interface_along_a_different_path_is_not_reported_as_a_regression() {
        // The case that forced two signals: eight more subsystems reached behind an
        // instruction pointer that had gone backwards, because the path changed
        // underneath it (D129). Calling that BACK would have buried a real gain.
        let before = trace(3, 10, Some("image"), Some(0x900));
        let after = trace(9, 40, Some("image"), Some(0x100));
        let seen = compare(Some(&before), &after);
        assert_eq!(seen.verdict, Verdict::MoreInterfaceDifferentPath);
        assert!(!seen.verdict.is_progress() || seen.distinct_delta > 0);
    }

    #[test]
    fn positions_in_different_regions_are_not_compared() {
        // An instruction pointer from a different code path is a different number about
        // a different thing; ordering them would invent a result.
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
        // The summary lives here so two shims cannot describe one measurement
        // differently, which is the whole reason this moved (D160).
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
        // **The reward hack, and the whole reason conditions are recorded.** Making
        // unimplemented functions answer `ok` is one line of TOML and improves every
        // number at once: the guest stops checking, runs on, and reaches imports it never
        // reached before. Nothing has been implemented. Anything steering by a call count
        // finds this within a few iterations because it is the highest-scoring single
        // change available.
        //
        // The verdict still renders - the numbers are real - but it cannot be allowed to
        // render *unqualified*.
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
        // Same build, same title, different machine: a faster host reaches further inside
        // the same number of seconds. Two contributors comparing runs would be comparing
        // their hardware, which is the failure that matters once results are shared.
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
        // It changes on every release and would fire on every comparison, drowning the two
        // conditions that actually change what a run does. Recorded for a result somebody
        // else contributed, not for the local loop.
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
        // The ordinary case has to stay quiet, or the caveat becomes noise and stops being
        // read - the same rule the stack-conformance line follows in reverse.
        let conditions = Conditions {
            experiments: String::new(),
            intervened: false,
            memory_map: Vec::new(),
            limit_seconds: Some(20),
            call_budget: None,
            did_nothing: Vec::new(),
            default_return: "unimplemented".to_owned(),
            overrides: 3,
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

    #[test]
    fn only_calls_nothing_implements_count_against_the_total() {
        // The discount on the headline number. 60 of 100 calls reached real code, so the
        // run stands on 40% placeholder - and a report giving only the 100 lets the two
        // be confused in the direction that flatters.
        let trace = CallTrace {
            total_calls: 100,
            calls: vec![
                CalledImport {
                    index: 0,
                    label: "libc::memset".to_owned(),
                    calls: 60,
                    implemented: true,
                },
                CalledImport {
                    index: 1,
                    label: "libc::snprintf_s".to_owned(),
                    calls: 40,
                    implemented: false,
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
        // A raw code is a *specific* answer somebody established, which is not the same as
        // the loud default - but it is also not a report of "nothing implements this", so
        // a run under one is still standing on something it has not proved.
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
    /// The budget exists to make a verdict trustworthy, so a budget that changed between
    /// two runs is exactly the case where the verdict is not - a guest stopped at twenty
    /// million calls and one stopped at ten reaches less for a reason that has nothing to
    /// do with the build (D238).
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

        // And identical budgets are not a difference, or every run would carry the note
        // and people would learn to skip the line.
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

    /// **A run that stopped faulting is not a run that went backwards.**
    ///
    /// The comparison scored a missing fault as position `Less` and reported `BACK` from it -
    /// on a run where the wall the last one hit had gone. Nothing asserted either arm, so it
    /// had never been looked at; `describe_end`, thirty lines above, already refuses to
    /// pretend a missing fault is an address (D309).
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

    /// **And a run that started faulting did not thereby get further.**
    ///
    /// The mirror fabrication: position `Greater` purely because this run has a fault and the
    /// last one did not, which read as `FURTHER` on the same imports. Reach is the one word
    /// this project steers by, so inventing it is the expensive direction to be wrong in.
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
    /// Dropping the invented ordering must not drop the measurement that survives it: more
    /// imports is still progress whether or not anything faulted.
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
