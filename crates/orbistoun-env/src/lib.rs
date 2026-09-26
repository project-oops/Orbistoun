//! Every environment variable orbistoun reads, declared in one place (D221).
//!
//! Each variable is declared here, read through here, and listed by `orbistoun-cli env`. A
//! misspelled variable is an absence rather than an error, so the registry is what lets a run
//! report a name that looks like ours and is not declared.
//!
//! A setting configures the emulator and may persist; a diagnostic changes the program being
//! observed to learn something, and never comes from a file. Run configuration proper is
//! `orbistoun_service::FileConfig` in `config.toml`; this crate has no dependencies because
//! `orbistoun-paths` needs it to locate the data root, and so `config.toml` itself.

/// What a variable is for, and therefore what may be done with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Configures how the emulator behaves. Persistent by nature.
    Setting,
    /// Changes the program in order to learn something, then goes away. Never read from a file.
    Diagnostic,
}

impl Kind {
    /// How it is written in a listing.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Setting => "setting",
            Self::Diagnostic => "diagnostic",
        }
    }

    /// Whether a value for this may come from anywhere but the environment itself.
    pub const fn may_persist(self) -> bool {
        matches!(self, Self::Setting)
    }
}

/// Whether a diagnostic leaves the program alone or changes it.
///
/// Declared rather than judged at the time, so a run report can put a caveat beside a verdict
/// earned under an intervention (D227).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// Reads the program without changing it. A verdict under this measures the emulator.
    Observes,
    /// Changes the program in order to learn from the difference. A result under this is never a
    /// diagnosis on its own: it needs a second observation of what the guest did with the
    /// intervention.
    Intervenes,
}

impl Effect {
    /// How it is written in a listing.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Observes => "observes",
            Self::Intervenes => "intervenes",
        }
    }

    /// Whether a verdict earned under this needs a caveat printed beside it.
    pub const fn needs_caveat(self) -> bool {
        matches!(self, Self::Intervenes)
    }
}

/// One variable, and everything a person needs to know about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Var {
    /// The name, as it is spelled in a shell.
    pub name: &'static str,
    /// Setting or diagnostic.
    pub kind: Kind,
    /// What it does, in one line.
    pub summary: &'static str,
    /// A value that works, so a listing can be copied rather than guessed at.
    pub example: &'static str,
    /// Which crate reads it, so a reader can go and look.
    pub read_by: &'static str,
    /// Whether it leaves the program alone or changes it.
    ///
    /// [`Effect::Observes`] for a setting, which configures the emulator rather than
    /// altering a run in flight.
    pub effect: Effect,
}

impl Var {
    /// What it is set to, or nothing.
    ///
    /// Trimmed, because a trailing space in a shell assignment is invisible and changes what the
    /// value parses as.
    pub fn get(&self) -> Option<String> {
        let raw = std::env::var(self.name).ok()?;
        let trimmed = raw.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_owned())
    }

    /// Whether it is set to anything at all.
    pub fn is_set(&self) -> bool {
        self.get().is_some()
    }
}

// Settings.

/// Where orbistoun keeps everything it writes.
pub const PORTABLE_MODE: Var = Var {
    name: "ORBISTOUN_PORTABLE_MODE",
    kind: Kind::Setting,
    summary: "keep all data beside the binary instead of in the platform data directory",
    example: "1",
    effect: Effect::Observes,
    read_by: "orbistoun-paths",
};

/// An explicit data root, overriding both portable and platform resolution.
pub const DATA_DIR: Var = Var {
    name: "ORBISTOUN_DATA_DIR",
    kind: Kind::Setting,
    summary: "put the data root at an explicit path",
    example: "/tmp/orbistoun",
    effect: Effect::Observes,
    read_by: "orbistoun-paths",
};

/// Whether the per-title device sandbox keeps what a guest wrote, or starts each run empty.
///
/// The sandboxed writable device paths (`/mnt/usb0`, `/data`, `/download0`) are per-title writable
/// overlays (D251). By default what a guest writes persists, so a title's saves and a probe's
/// reports survive the run. `ephemeral` empties the title's overlay at the start of each run, for
/// when a file left by an earlier run would confuse a fresh one.
pub const SANDBOX: Var = Var {
    name: "ORBISTOUN_SANDBOX",
    kind: Kind::Setting,
    summary: "retain (default) keeps the per-title sandbox between runs; ephemeral empties it each run",
    example: "ephemeral",
    effect: Effect::Observes,
    read_by: "orbistoun-worker",
};

/// When a drawn colour target is written back into guest memory (D714).
///
/// `flip` (the default) keeps the frame on the device across a frame's submissions and writes it
/// back when the guest flips, or before a submission draws into another target. `submit` writes it
/// back after every submission, for a guest that reads its own target between submissions.
pub const TARGET_WRITEBACK: Var = Var {
    name: "ORBISTOUN_TARGET_WRITEBACK",
    kind: Kind::Setting,
    summary: "flip (default) writes a drawn target back at the flip; submit writes it back after every submission",
    example: "submit",
    effect: Effect::Observes,
    read_by: "orbistoun-worker",
};

/// Sample where the guest's main thread is, about once a millisecond, and print the counts every
/// few seconds.
pub const PROFILE: Var = Var {
    name: "ORBISTOUN_PROFILE",
    kind: Kind::Diagnostic,
    summary: "1 samples the guest's main and device threads and prints where they spend their time; a larger number shows that many places",
    example: "40",
    effect: Effect::Observes,
    read_by: "orbistoun-worker",
};

/// Measure a submission's finer spans and print them once a second.
///
/// The always-on phases say which part of a frame is slow; these say which part of a submission is,
/// across its thread hand-offs and the command processor's own steps. Off by default; each span
/// costs a relaxed load.
pub const PERF_DETAIL: Var = Var {
    name: "ORBISTOUN_PERF_DETAIL",
    kind: Kind::Diagnostic,
    summary: "1 prints each submission span's time once a second, beside the perf phases",
    example: "1",
    effect: Effect::Observes,
    read_by: "orbistoun-worker",
};

/// Each submission the executor carried out, a line apiece.
///
/// Off by default because a GL title submits many times a frame and the lines cost frame time.
/// Refusals and failures are printed regardless.
pub const TRACE_SUBMITS: Var = Var {
    name: "ORBISTOUN_TRACE_SUBMITS",
    kind: Kind::Diagnostic,
    summary: "1 prints a line for every submission whose draws ran - refusals are printed regardless",
    example: "1",
    effect: Effect::Observes,
    read_by: "orbistoun-worker",
};

/// How long a guest is allowed to run, for the shell script's `run` verb.
///
/// Declared although no Rust reads it, so the typo check does not report it as a misspelling.
pub const LIMIT: Var = Var {
    name: "ORBISTOUN_LIMIT",
    kind: Kind::Setting,
    summary: "seconds a guest may run under ./bin/orbistoun run",
    example: "20",
    effect: Effect::Observes,
    read_by: "bin/orbistoun",
};

/// The commit a binary was built from.
///
/// Read at compile time, so it never appears in the environment of a run. Declared for the same
/// reason as [`LIMIT`].
pub const COMMIT: Var = Var {
    name: "ORBISTOUN_COMMIT",
    kind: Kind::Setting,
    summary: "stamped into a build so a report says which tree produced it (compile-time)",
    example: "a1b2c3d",
    effect: Effect::Observes,
    read_by: "orbistoun-service",
};

/// Credential for a hosted language-model provider.
pub const LLM_API_KEY: Var = Var {
    name: "ORBISTOUN_LLM_API_KEY",
    kind: Kind::Setting,
    summary: "credential for a hosted model provider, when one is configured",
    example: "sk-…",
    effect: Effect::Observes,
    read_by: "orbistoun-llm",
};

/// Let `sceKernelDlsym` answer for names this project declares but does not implement.
///
/// A name reached by import lands on a stub answering the placeholder; the same name reached by
/// `sceKernelDlsym` is refused, because the by-name table holds only implemented functions. A guest
/// handed a stub calls it and gets a placeholder, where a guest handed null may take a fallback, so
/// the alternative is a flag to measure rather than a default.
pub const DLSYM_STUBS: Var = Var {
    name: "ORBISTOUN_DLSYM_STUBS",
    kind: Kind::Diagnostic,
    summary: "resolve declared-but-unimplemented names by name too - does a guest do better or worse?",
    example: "1",
    read_by: "orbistoun-worker",
    effect: Effect::Intervenes,
};
// Diagnostics.

/// Force argument dumps for named imports.
pub const DUMP: Var = Var {
    name: "ORBISTOUN_DUMP",
    kind: Kind::Diagnostic,
    summary: "dump arguments for these imports even though something implements them",
    example: "memalign,0x6abac2f3dc6f8cee",
    read_by: "orbistoun-worker",
    effect: Effect::Observes,
};

/// Post a flip completion to every event queue, not only the ones registered for it.
///
/// A title can register a completion on its own queue through `sceAgcDriverAddEqEvent`, which
/// nothing implements, and then block on that queue. This does not model the registration; it asks
/// what the guest does next if that wait completed. Off by default, and it intervenes (D227).
pub const FLIP_TO_ALL: Var = Var {
    name: "ORBISTOUN_FLIP_TO_ALL",
    kind: Kind::Diagnostic,
    summary: "post a flip completion to every queue - would waking that wait get the guest further?",
    example: "1",
    read_by: "orbistoun-video",
    effect: Effect::Intervenes,
};
/// Fill the guest stack before entering.
pub const STACK_FILL: Var = Var {
    name: "ORBISTOUN_STACK_FILL",
    kind: Kind::Diagnostic,
    summary: "fill the guest stack with this byte - does the run depend on memory nobody wrote?",
    example: "5a",
    read_by: "orbistoun-worker",
    effect: Effect::Intervenes,
};

/// Fill every heap allocation before the guest sees it.
pub const HEAP_FILL: Var = Var {
    name: "ORBISTOUN_HEAP_FILL",
    kind: Kind::Diagnostic,
    summary: "fill every allocation with this byte - the same question, for the heap",
    example: "a5",
    read_by: "orbistoun-libc",
    effect: Effect::Intervenes,
};

/// Serve every allocation from a region at a fixed address, instead of from the host heap.
pub const HEAP_BASE: Var = Var {
    name: "ORBISTOUN_HEAP_BASE",
    kind: Kind::Diagnostic,
    summary: "serve the heap from a region reserved at a fixed address - `default`, or a hexadecimal address - does the run depend on where the host put the heap?",
    example: "default",
    read_by: "orbistoun-libc",
    effect: Effect::Intervenes,
};

/// Run every placed module's initialisers before the guest starts, not only the ones it loads.
pub const START_MODULES: Var = Var {
    name: "ORBISTOUN_START_MODULES",
    kind: Kind::Diagnostic,
    summary: "run every placed module's DT_INIT_ARRAY before entry - does the guest depend on a module being initialised that it never asks to load?",
    example: "all",
    read_by: "orbistoun-worker",
    effect: Effect::Intervenes,
};

/// Fill the guest's `.bss` before it starts, instead of zeroing it.
pub const BSS_FILL: Var = Var {
    name: "ORBISTOUN_BSS_FILL",
    kind: Kind::Diagnostic,
    summary: "fill .bss with this byte - does the guest depend on a global nobody initialised?",
    example: "b5",
    read_by: "orbistoun-loader",
    effect: Effect::Intervenes,
};

/// What a global nothing implements holds, when a run enters past the runtime.
pub const RUNTIME_GLOBALS: Var = Var {
    name: "ORBISTOUN_RUNTIME_GLOBALS",
    kind: Kind::Diagnostic,
    summary: "globals nothing implements to point at a reporting stub, comma-separated - each says every register it was called with, rax included",
    example: "ptr_syscall",
    read_by: "orbistoun-worker",
    effect: Effect::Intervenes,
};

/// How many ranked findings a run prints before summarising the rest.
pub const FINDINGS: Var = Var {
    name: "ORBISTOUN_FINDINGS",
    kind: Kind::Setting,
    summary: "how many ranked findings to print - the default six summarises the rest as a count, which hides the arguments of every finding past it",
    example: "40",
    read_by: "orbistoun-cli",
    effect: Effect::Observes,
};

/// Which generation of the platform's file structures a guest is given.
pub const STAT_LAYOUT: Var = Var {
    name: "ORBISTOUN_STAT_LAYOUT",
    kind: Kind::Setting,
    summary: "which generation of `struct stat` and `struct dirent` a guest is given - `freebsd11` (default) or `current`",
    example: "current",
    read_by: "orbistoun-fs",
    effect: Effect::Observes,
};

/// Record every path a guest successfully opened, and report them at the end.
///
/// A setting, not a diagnostic: it changes what is reported, not what the guest does.
/// `orbistoun-fs` always records the paths it could not answer; successes are the common case, and
/// paying a lock and a string for each on the guest's stack changes the timing observed, so
/// recording them is opt-in.
pub const TRACE_OPENS: Var = Var {
    name: "ORBISTOUN_TRACE_OPENS",
    kind: Kind::Setting,
    summary: "record every path a guest opens successfully and list them at the end - off by default, because the failures are the rare case and the successes are not",
    example: "1",
    read_by: "orbistoun-fs",
    effect: Effect::Observes,
};

/// Which clock a guest reads.
///
/// The default advances by a fixed step per reading, so a run repeats and time still moves (D582).
/// `host` restores the wall clock, for measuring how long something really took.
pub const CLOCK: Var = Var {
    name: "ORBISTOUN_CLOCK",
    kind: Kind::Setting,
    summary: "which clock the guest reads - `logical` (default) advances a fixed step per reading, so two runs agree; `host` reads real time, which no two runs do",
    example: "host",
    read_by: "orbistoun-hle",
    effect: Effect::Observes,
};

/// The run's opening calls, in order, with the address each was made from.
///
/// The call trace keeps the tail, for the fault; this keeps the head, which says where two runs
/// diverged (D571).
pub const TRACE_CALLS: Var = Var {
    name: "ORBISTOUN_TRACE_CALLS",
    kind: Kind::Setting,
    summary: "list the run's opening calls in order, with the address each was made from - the head of the sequence, where the trace keeps the tail",
    example: "1",
    read_by: "orbistoun-worker",
    effect: Effect::Observes,
};

/// Every string the guest rendered with a format function, in order.
///
/// A guest usually explains why it is stopping, and most of that text is formatted into a buffer
/// handed to its own logger or to a service nothing implements, so it reaches no stream.
pub const TRACE_FORMAT: Var = Var {
    name: "ORBISTOUN_TRACE_FORMAT",
    kind: Kind::Setting,
    summary: "record every string the guest formats and list them at the end - off by default, because a title formats constantly",
    example: "1",
    read_by: "orbistoun-libc",
    effect: Effect::Observes,
};

/// Deliver the file the asynchronous file path resolved, into the buffer its command header names.
///
/// An experiment, and it intervenes: nothing establishes that the buffer means what this assumes,
/// so a verdict under it carries a caveat.
pub const APR_DELIVER: Var = Var {
    name: "ORBISTOUN_APR_DELIVER",
    kind: Kind::Diagnostic,
    summary: "read the file the asynchronous file path resolved into the buffer its command header names - does the guest accept bytes it was not told how to ask for?",
    example: "1",
    read_by: "orbistoun-kernel",
    effect: Effect::Intervenes,
};

/// Every mapping a guest was given, in the order it was given them.
///
/// A run reports the reservations that failed and nothing about the ones that succeeded, so without
/// this a pointer into guest memory cannot be traced back to the call that produced it. The arena
/// is bump-allocated, so the sequence also shows where two runs' addresses diverge.
pub const TRACE_MAPS: Var = Var {
    name: "ORBISTOUN_TRACE_MAPS",
    kind: Kind::Setting,
    summary: "record every mapping the guest is given, in order, and list them at the end - off by default, because a title maps for as long as it runs",
    example: "1",
    read_by: "orbistoun-kernel",
    effect: Effect::Observes,
};

/// What the handoff structure's unestablished fields hold.
pub const HANDOFF_FIELDS: Var = Var {
    name: "ORBISTOUN_HANDOFF_FIELDS",
    kind: Kind::Diagnostic,
    summary: "what the handoff structure's unestablished fields hold - `strict` stops on any use and names the field, `markers` lets a read succeed, `deep` names the offset behind one, `members` makes a call through one say how it was called, `zero` lets a guest check them",
    example: "zero",
    read_by: "orbistoun-worker",
    effect: Effect::Intervenes,
};

/// What a guest is handed at its entry point, overriding the configured choice.
///
/// `orbistoun-cli handoff` sets this together with the poison, so a handoff field is poisoned in
/// the block the guest actually receives (D399).
pub const ENTRY_ARGUMENT: Var = Var {
    name: "ORBISTOUN_ENTRY_ARGUMENT",
    kind: Kind::Diagnostic,
    summary: "what the guest is handed at its entry point, overriding the configured choice - `handoff` gives it the resolver table an open-toolchain payload expects, `main` the argument count and vector a title expects, `zero` nothing at all",
    example: "handoff",
    read_by: "orbistoun-worker",
    effect: Effect::Intervenes,
};

/// Fill a structure this cannot describe with markers, and let the guest say what it reads.
pub const DESCRIBE: Var = Var {
    name: "ORBISTOUN_DESCRIBE",
    kind: Kind::Diagnostic,
    summary: "answer a call whose out-parameter layout is unknown by filling it with markers that name their own offset - a guest using a field then faults on an address that says which field it was",
    example: "module-info",
    read_by: "orbistoun-kernel",
    effect: Effect::Intervenes,
};

/// Which imports the loader resolves.
pub const RESOLVE: Var = Var {
    name: "ORBISTOUN_RESOLVE",
    kind: Kind::Diagnostic,
    summary: "which imports resolve - `all` gives every one a stub so a call is reported, `named` refuses the ones this build cannot even name, so a guest can tell a symbol that exists from one that does not",
    example: "named",
    read_by: "orbistoun-worker",
    effect: Effect::Intervenes,
};

/// Give one handoff field a value nothing maps, so its use names it.
pub const HANDOFF_POISON: Var = Var {
    name: "ORBISTOUN_HANDOFF_POISON",
    kind: Kind::Diagnostic,
    summary: "put an address nothing maps in this one handoff field - a run that faults on that address used the field, and a run that does not never reached it",
    example: "5",
    read_by: "orbistoun-worker",
    effect: Effect::Intervenes,
};

/// Fill every direct-memory mapping before the guest sees it.
pub const DIRECT_FILL: Var = Var {
    name: "ORBISTOUN_DIRECT_FILL",
    kind: Kind::Diagnostic,
    summary: "fill every direct-memory mapping with this byte - the third place nobody wrote",
    example: "d1",
    read_by: "orbistoun-kernel",
    effect: Effect::Intervenes,
};

/// Plant a value at the address in an argument, before an import answers.
pub const WRITE: Var = Var {
    name: "ORBISTOUN_WRITE",
    kind: Kind::Diagnostic,
    summary: "plant values at *(argN+off) of an import - which member was it waiting to have filled in?",
    example: "0x6abac2f3dc6f8cee:0+0:0x110000000000,0x6abac2f3dc6f8cee:0+24:0x440000000000",
    read_by: "orbistoun-worker",
    effect: Effect::Intervenes,
};

/// Force what an import answers, reaching functions the policy file cannot name.
///
/// `StubPolicy` is keyed by symbol name and carries a 32-bit code, so it cannot name a function
/// with no name or express a 64-bit region base. This can.
pub const RETURN: Var = Var {
    name: "ORBISTOUN_RETURN",
    kind: Kind::Diagnostic,
    summary: "force an import to answer this 64-bit value - reaches unnamed functions by hash",
    example: "0x6abac2f3dc6f8cee:0x700000000000",
    read_by: "orbistoun-worker",
    effect: Effect::Intervenes,
};

/// Reserve a region of guest address space before the guest runs.
pub const MAP: Var = Var {
    name: "ORBISTOUN_MAP",
    kind: Kind::Diagnostic,
    summary: "reserve <addr>[+len] before entry - does a fault there become a region the guest wanted?",
    example: "0xf0000+0x10000",
    read_by: "orbistoun-worker",
    effect: Effect::Intervenes,
};

/// Peek at a window of guest memory at a fault, so runtime-mapped code no static disassembly
/// reaches can be read.
///
/// `caller` dumps the window ending at the faulting call site (the first stack frame's return
/// address); `<addr>[+len]` dumps a fixed range. It only reads, so it observes. [`DUMP`] dumps an
/// import's arguments instead.
pub const PEEK: Var = Var {
    name: "ORBISTOUN_PEEK",
    kind: Kind::Diagnostic,
    summary: "hex-dump guest memory at a fault - `caller` for the faulting call site, or `<addr>[+len]`",
    example: "caller",
    read_by: "orbistoun-worker",
    effect: Effect::Observes,
};

/// Which shape of physical memory map the guest is shown.
///
/// A shape changes what the emulator presents, not the program's code, but it changes the program's
/// inputs, so a verdict under one is a verdict about that shape and is marked
/// [`Effect::Intervenes`].
pub const MAP_SHAPE: Var = Var {
    name: "ORBISTOUN_MAP_SHAPE",
    kind: Kind::Diagnostic,
    summary: "show the guest this physical map shape - whole, reserved-low or fragmented",
    example: "fragmented",
    read_by: "orbistoun-worker",
    effect: Effect::Intervenes,
};

/// Give every unimplemented function its own placeholder, so one found as data names its source.
///
/// Every stub answers the same placeholder, so one turning up in a guest's argument says
/// only that some unimplemented function produced it. Under this, a stub answers `PLACEHOLDER_BASE
/// | index`, so the value is the attribution (D567). It changes what the guest is told, so it
/// intervenes.
pub const TAG_PLACEHOLDERS: Var = Var {
    name: "ORBISTOUN_TAG_PLACEHOLDERS",
    kind: Kind::Diagnostic,
    summary: "give each unimplemented function its own placeholder, so one found as data names it",
    example: "1",
    read_by: "orbistoun-service",
    effect: Effect::Intervenes,
};

/// Write a value at a guest address before the guest runs.
pub const POKE: Var = Var {
    name: "ORBISTOUN_POKE",
    kind: Kind::Diagnostic,
    summary: "write <addr>:<value> into guest memory before entry - does the fault follow it?",
    example: "0x4000019e9cb0:0x11000000",
    read_by: "orbistoun-worker",
    effect: Effect::Intervenes,
};

/// Snapshot a region of guest memory and report what changed.
pub const WATCH: Var = Var {
    name: "ORBISTOUN_WATCH",
    kind: Kind::Diagnostic,
    summary: "snapshot <addr>[+len] and report what changed - or, for a region the guest maps while it runs, what it holds when the guest stops",
    example: "0x4000019e9c00+0x80",
    read_by: "orbistoun-worker",
    effect: Effect::Observes,
};

/// Trap on every access to an address and say which instruction made it.
///
/// A separate variable from [`WATCH`] (D276): a snapshot says which bytes changed, this says who
/// touched them. The snapshot's untouched words become the next run's watchpoints.
pub const WATCHPOINT: Var = Var {
    name: "ORBISTOUN_WATCHPOINT",
    kind: Kind::Diagnostic,
    summary: "trap on <addr>[+len][:w|rw], up to four, and report the instruction that touched it",
    example: "0x4000019e9c10:rw",
    read_by: "orbistoun-worker",
    effect: Effect::Observes,
};

/// Write self-identifying values into the memory-query structure.
pub const MARK_QUERY: Var = Var {
    name: "ORBISTOUN_MARK_QUERY",
    kind: Kind::Diagnostic,
    summary: "dye the memory-query fields so the guest says which one it read",
    example: "1",
    read_by: "orbistoun-kernel",
    effect: Effect::Intervenes,
};

/// Every variable, in the order a listing shows them.
///
/// A constant missing from this array is invisible to the listing and to the typo check;
/// `every_declared_variable_is_in_the_registry` catches it.
pub const REGISTRY: &[Var] = &[
    PORTABLE_MODE,
    DATA_DIR,
    SANDBOX,
    TARGET_WRITEBACK,
    PERF_DETAIL,
    TRACE_SUBMITS,
    PROFILE,
    LIMIT,
    COMMIT,
    LLM_API_KEY,
    DUMP,
    DLSYM_STUBS,
    FLIP_TO_ALL,
    STACK_FILL,
    DIRECT_FILL,
    BSS_FILL,
    ENTRY_ARGUMENT,
    HANDOFF_FIELDS,
    HANDOFF_POISON,
    DESCRIBE,
    RESOLVE,
    STAT_LAYOUT,
    TRACE_OPENS,
    TRACE_MAPS,
    TRACE_CALLS,
    APR_DELIVER,
    TRACE_FORMAT,
    CLOCK,
    RUNTIME_GLOBALS,
    HEAP_FILL,
    HEAP_BASE,
    START_MODULES,
    FINDINGS,
    WATCH,
    WATCHPOINT,
    POKE,
    TAG_PLACEHOLDERS,
    MAP,
    MAP_SHAPE,
    WRITE,
    RETURN,
    PEEK,
    MARK_QUERY,
];

/// The prefix every variable of ours carries.
pub const PREFIX: &str = "ORBISTOUN_";

/// Anything set that looks like ours and is not declared.
///
/// A misspelled variable is an absence, so a diagnostic that never ran reports an ordinary result.
/// Sorted, so warnings keep their order between runs.
pub fn unknown() -> Vec<String> {
    let mut found: Vec<String> = std::env::vars()
        .map(|(name, _)| name)
        .filter(|name| name.starts_with(PREFIX))
        .filter(|name| !REGISTRY.iter().any(|v| v.name == *name))
        .collect();
    found.sort();
    found
}

/// Every variable currently set, with what it is set to.
///
/// For a run to record what it was under, and for `orbistoun-cli env` to show a person.
pub fn active() -> Vec<(&'static Var, String)> {
    REGISTRY
        .iter()
        .filter_map(|v| v.get().map(|value| (v, value)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Effect, Kind, PREFIX, REGISTRY, Var};

    /// Every constant declared above, for the completeness check.
    ///
    /// Hand-listed because Rust cannot enumerate a module's constants; the test below fails when
    /// this and [`REGISTRY`] disagree.
    const DECLARED: &[Var] = &[
        super::PORTABLE_MODE,
        super::DATA_DIR,
        super::SANDBOX,
        super::TARGET_WRITEBACK,
        super::PERF_DETAIL,
        super::TRACE_SUBMITS,
        super::PROFILE,
        super::LIMIT,
        super::COMMIT,
        super::LLM_API_KEY,
        super::DUMP,
        super::DLSYM_STUBS,
        super::FLIP_TO_ALL,
        super::STACK_FILL,
        super::DIRECT_FILL,
        super::BSS_FILL,
        super::ENTRY_ARGUMENT,
        super::HANDOFF_FIELDS,
        super::HANDOFF_POISON,
        super::DESCRIBE,
        super::RESOLVE,
        super::STAT_LAYOUT,
        super::TRACE_OPENS,
        super::TRACE_MAPS,
        super::TRACE_CALLS,
        super::APR_DELIVER,
        super::TRACE_FORMAT,
        super::CLOCK,
        super::RUNTIME_GLOBALS,
        super::HEAP_FILL,
        super::HEAP_BASE,
        super::START_MODULES,
        super::FINDINGS,
        super::WATCH,
        super::POKE,
        super::TAG_PLACEHOLDERS,
        super::MAP,
        super::MAP_SHAPE,
        super::WRITE,
        super::RETURN,
        super::PEEK,
        super::MARK_QUERY,
        super::WATCHPOINT,
    ];

    /// Every declared constant is in the registry.
    #[test]
    fn every_declared_variable_is_in_the_registry() {
        // A constant not in the registry would be reported as a misspelling of itself.
        for var in DECLARED {
            assert!(
                REGISTRY.contains(var),
                "{} is declared but not in REGISTRY",
                var.name
            );
        }
        assert_eq!(
            REGISTRY.len(),
            DECLARED.len(),
            "a variable was added to one list and not the other"
        );
    }

    /// Names are unique and carry the prefix.
    #[test]
    fn every_name_is_unique_and_carries_the_prefix() {
        for var in REGISTRY {
            assert!(
                var.name.starts_with(PREFIX),
                "{} would never be reached by the typo check",
                var.name
            );
            assert_eq!(
                REGISTRY.iter().filter(|o| o.name == var.name).count(),
                1,
                "{} is declared twice",
                var.name
            );
        }
    }

    /// Every variable has a summary and an example.
    #[test]
    fn every_variable_says_what_it_is_for_and_how_to_set_it() {
        // The example makes a listing copyable.
        for var in REGISTRY {
            assert!(!var.summary.is_empty(), "{} has no summary", var.name);
            assert!(!var.example.is_empty(), "{} has no example", var.name);
            assert!(
                !var.read_by.is_empty(),
                "{} says nothing about who reads it",
                var.name
            );
        }
    }

    /// Only a diagnostic changes the program.
    #[test]
    fn only_a_diagnostic_may_change_the_program() {
        // A setting configures the emulator and does not alter a run in flight; an intervening
        // setting would put a caveat on every ordinary run (D227).
        for var in REGISTRY {
            if var.kind == Kind::Setting {
                assert_eq!(
                    var.effect,
                    Effect::Observes,
                    "{} is a setting and must not intervene",
                    var.name
                );
            }
        }
        // The distinction is in use: something observes and something intervenes.
        let intervening = REGISTRY
            .iter()
            .filter(|v| v.effect == Effect::Intervenes)
            .count();
        assert!(
            intervening > 0,
            "nothing intervenes, so the caveat never fires"
        );
        assert!(
            intervening < REGISTRY.len(),
            "everything intervenes, so the caveat is on every run and means nothing"
        );
        assert!(Effect::Intervenes.needs_caveat());
        assert!(!Effect::Observes.needs_caveat());
    }

    /// A build always identifies itself.
    #[test]
    fn a_build_always_identifies_itself_somehow() {
        // Never empty: a commit if there is one, the compile time if not, and the version either
        // way. A blank footer cannot be told from one nobody wired up (D222).
        let shown = super::build::line();
        assert!(shown.starts_with('v'), "{shown}");
        assert!(shown.contains(env!("CARGO_PKG_VERSION")), "{shown}");
        match super::build::commit() {
            Some(c) => assert!(shown.contains(c), "a known commit must be shown: {shown}"),
            // The path taken when the build has no commit.
            None => assert!(shown.contains("built"), "{shown}"),
        }
    }

    /// A diagnostic may never be persisted, and a setting may.
    #[test]
    fn a_diagnostic_may_never_be_persisted_and_a_setting_may() {
        // A diagnostic is never persisted (D221).
        assert!(!Kind::Diagnostic.may_persist());
        assert!(Kind::Setting.may_persist());
        assert!(
            REGISTRY.iter().any(|v| v.kind == Kind::Diagnostic),
            "if nothing is a diagnostic, the distinction is not being used"
        );
    }
}

pub mod build {
    //! What this binary is, for showing somewhere a person will see it (D222).
    //!
    //! The commit is shown in every front end so a bug report, screenshot or run result ties to a
    //! tree; with no commit, the compile time is shown instead. `oops_build::stamp!` expands at its
    //! call site and reads that crate's version and commit, and only this crate's `build.rs` stamps
    //! one, so it is expanded here once for every front end.

    /// This build.
    #[must_use]
    pub fn stamp() -> oops_build::Stamp {
        oops_build::stamp!()
    }

    /// The commit this was built from, when there was one.
    ///
    /// Carries `-dirty` when the tree had uncommitted changes, because a binary built from edits is
    /// not the commit it would otherwise name. Ask [`oops_build::Stamp::is_exact`] rather than
    /// testing this for `Some`: a dirty tree answers `Some`.
    #[must_use]
    pub fn commit() -> Option<&'static str> {
        stamp().commit
    }

    /// The crate version this was built at.
    #[must_use]
    pub fn version() -> &'static str {
        oops_build::version!()
    }

    /// The build, in one short line: a commit if there is one, otherwise when it was built.
    #[must_use]
    pub fn line() -> String {
        stamp().line()
    }

    /// The same line, borrowed for the life of the process, for `clap`.
    #[must_use]
    pub fn line_static() -> &'static str {
        oops_build::line!()
    }
}
