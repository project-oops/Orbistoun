//! Every environment variable orbistoun reads, declared in one place.
//!
//! # Why a crate for nine strings
//!
//! Because they were nine strings in five crates, and the list of them existed nowhere.
//! That has three costs, and all three were being paid:
//!
//! - **A typo does nothing.** A command-line flag spelled wrongly is refused; a variable
//!   spelled wrongly is simply absent, and the run reports an ordinary result. Catching
//!   that needs a list of what is real, and there was none - so the first attempt at the
//!   check hand-wrote a second copy of the names it had to excuse (D220).
//! - **Documentation drifts.** The diagnostics were described in three separate decision
//!   entries and then hand-copied into a table in `docs/WORKFLOW.md`. A hand-copied table
//!   is a second list, and second lists drift.
//! - **Nothing stops another one appearing.** A crate could read a new variable and
//!   nobody would find out until somebody grepped.
//!
//! So: declared here, read through here, and listed by `orbistoun-cli env` rather than by
//! anyone retyping them.
//!
//! # Settings and diagnostics are different things
//!
//! A **setting** configures how the emulator behaves and is meant to persist. A
//! **diagnostic** changes the program being observed in order to find something out, and
//! is meant to go away - "does this run depend on memory nobody wrote?" is asked once, not
//! configured (D185).
//!
//! The distinction is not decoration. It decides what may be persisted: a diagnostic left
//! in a file for three weeks stops being an experiment and becomes an undocumented
//! workaround for a bug nobody found. So if this ever grows `.env` support, **settings may
//! come from a file and diagnostics may not** - and refusing loudly is better than
//! silently honouring one (D221).
//!
//! It is also what makes the typo check correct rather than approximate: "is this a real
//! variable" is a lookup here, not a hand-maintained list of exceptions somewhere else.
//!
//! # This is not the configuration crate, and cannot be
//!
//! Most of what configures a run lives in `config.toml`, handled by
//! `orbistoun_service::FileConfig` - entry presentation, thread placement, memory
//! behaviour, the library folder, what unimplemented functions answer.
//!
//! That is deliberately somewhere else and **structurally has to be**: `FileConfig` is
//! composed of settings owned by `orbistoun-loader`, `orbistoun-kernel` and
//! `orbistoun-hle`, so it sits near the top of the spine. This crate sits at the bottom
//! with no dependencies at all, because `orbistoun-paths` needs it to work out where the
//! data root is - and therefore where `config.toml` is.
//!
//! So the two cannot merge, and naming this one `orbistoun-config` would claim ownership of
//! a file it does not own. What the environment carries is the two settings that cannot
//! live in the file, because they decide where the file is, plus the diagnostics - which
//! are not meant to persist at all. `docs/WORKFLOW.md` states the split for a reader.

/// What a variable is for, and therefore what may be done with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Configures how the emulator behaves. Persistent by nature.
    Setting,
    /// Changes the program in order to learn something, then goes away.
    ///
    /// **Never read from a file.** See the module documentation.
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
/// # Why this is a field and not a judgement made at the time
///
/// A diagnostic that only **observes** leaves the guest running the program it would have
/// run, so a verdict taken under it measures the emulator. One that **intervenes** - a
/// poked value, a poisoned region, a reservation the guest never asked for - changes the
/// program being measured, so a guest getting further may simply be getting further on an
/// answer that is wrong.
///
/// That distinction was made badly and by hand: a mapping moved a wall, the movement was
/// read as confirming the hypothesis behind the mapping, and watching what the guest
/// *wrote* one run later said the opposite (D224, D226). Declaring it means the run report
/// can say so at the moment somebody is about to draw the conclusion (D227).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// Reads the program without changing it. A verdict under this measures the emulator.
    Observes,
    /// Changes the program in order to learn from the difference.
    ///
    /// **A result under this is never a diagnosis on its own.** It needs a second
    /// observation, of a different kind, saying what the guest did with the intervention.
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
    /// Trimmed, because a trailing space in a shell assignment is invisible and would
    /// otherwise silently change what a value parses as.
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

// --- Settings ----------------------------------------------------------------

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
/// The console's sandboxed writable device paths - `/mnt/usb0`, `/data`, `/download0` - are
/// per-title writable overlays here (D250, D251), and by default what a guest writes to them
/// **persists**: that is where a title's saves and a probe's reports live, and retaining them is
/// the point of modelling the sandbox at all. `ephemeral` empties the title's overlay at the
/// start of each run instead - closer to a console sandbox that carries no state between launches,
/// and the right choice when a file left by a previous run would confuse a fresh one.
///
/// The value the console really has is presumably ephemeral; the default here is the opposite on
/// purpose, because a proof of concept wants its evidence to survive the run that produced it.
pub const SANDBOX: Var = Var {
    name: "ORBISTOUN_SANDBOX",
    kind: Kind::Setting,
    summary: "retain (default) keeps the per-title sandbox between runs; ephemeral empties it each run",
    example: "ephemeral",
    effect: Effect::Observes,
    read_by: "orbistoun-worker",
};

/// How long a guest is allowed to run, for the shell script's `run` verb.
///
/// Declared here although no Rust reads it: this list is what a person consults and what
/// the typo check trusts, and a variable that is real but undeclared would be reported as
/// a misspelling - which is worse than not listing it.
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
/// Read at **compile** time, so it never appears in the environment of a run. Declared for
/// the same reason as [`LIMIT`].
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
/// # The inconsistency it measures
///
/// A name reached by **import** lands on a stub answering the placeholder; the same name reached
/// by `sceKernelDlsym` is refused, because the by-name table holds only implemented functions.
/// One function, two answers, decided by how the guest asked - and 266 names in a single payload
/// run are on the wrong side of it.
///
/// The console resolves both. Whether this should is genuinely two-sided: a guest handed a stub
/// calls it and gets a placeholder, where a guest handed null may take a fallback it would have
/// preferred. So the alternative is a flag to be measured rather than a change to be argued
/// (D632).
pub const DLSYM_STUBS: Var = Var {
    name: "ORBISTOUN_DLSYM_STUBS",
    kind: Kind::Diagnostic,
    summary: "resolve declared-but-unimplemented names by name too - does a guest do better or worse?",
    example: "1",
    read_by: "orbistoun-worker",
    effect: Effect::Intervenes,
};
// --- Diagnostics -------------------------------------------------------------

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
/// # The question it asks
///
/// A title registers a completion on its own queue through `sceAgcDriverAddEqEvent`, which
/// nothing implements, and then blocks on that queue for ever - while the two queues orbistoun
/// *does* feed are never waited on (D615). Nothing can post there, and nothing here knows what
/// identifier a post would carry: the registration call passes the queue and two zeroes.
///
/// This does not guess at the registration. It asks the narrower question a flip can answer on
/// its own: **if that wait completed, what would the guest do next?** A guest that proceeds says
/// a flip completion is near enough what it was waiting for; one that wakes and immediately
/// faults says which field of a delivered event it read. Either is evidence, and neither is an
/// implementation.
///
/// Off by default, and it intervenes: a run under it is not a measurement of the emulator, and
/// the report says so (D224, D226, D227).
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
/// # Why this is a setting and not a diagnostic
///
/// It changes what is **reported**, not what the guest does - the same place `ORBISTOUN_FINDINGS`
/// sits. A diagnostic here means a variable that changes the program in order to learn from the
/// difference, and a verdict earned under one carries a caveat; this earns none, because the
/// guest cannot tell it is on.
///
/// # Why it is off by default, when the failures are always recorded
///
/// `orbistoun-fs` records every path it *could not* answer unconditionally, and that costs
/// nothing on an ordinary run because failures are rare. Successes are the common case - a title
/// streaming assets opens hundreds - and paying a lock and a string for each on the guest's own
/// stack is the kind of observation that changes the thing observed (principle 9).
///
/// So it is asked for when the question is "what did it actually read", which is a question with
/// a wall behind it: PPSA03416 performed one file read of zero bytes in a whole run, and only the
/// paths it *failed* to open were visible (D578).
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
/// **The default repeats, because a measurement that cannot be repeated is not one** (D181,
/// D238). D256 declined to pin the clock on the grounds that a pinned one stops any title that
/// waits for time to pass, and was right - so the default is not pinned, it *advances by a
/// fixed step per reading*, which repeats and still moves (D582).
///
/// `host` restores the wall clock, for when the question is how long something really took.
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
/// **The head of the sequence, where the trace keeps only the tail.** The tail exists for the
/// fault - what was called just before it died. The head answers a different question that has
/// come up repeatedly and had no record: *what did two runs do differently before they diverged?*
/// PPSA03416's whole import drift is one branch between call 219 and call 226 (D602), and
/// nothing could say what those calls were (D603).
pub const TRACE_CALLS: Var = Var {
    name: "ORBISTOUN_TRACE_CALLS",
    kind: Kind::Setting,
    summary: "list the run's opening calls in order, with the address each was made from - the head of the sequence, where the trace keeps the tail",
    example: "1",
    read_by: "orbistoun-worker",
    effect: Effect::Observes,
};

/// Every mapping a guest was given, in the order it was given them.
///
/// The same asymmetry [`TRACE_OPENS`] closes for the filesystem: a run reports the reservations
/// that *failed* and nothing about the ones that succeeded, so a pointer into guest memory
/// cannot be traced back to the call that produced it.
///
/// It is also what a determinism question needs. The arena is bump-allocated, so an address is a
/// function of everything placed before it - two runs whose addresses differ can be seen to
/// differ and not where, unless the sequence is recorded (D581).
/// Every string the guest rendered with a format function, in order.
///
/// **What a title says about itself just before it stops.** A guest usually explains why it is
/// giving up, and most of that explanation never reaches a stream: it is formatted into a buffer
/// the guest hands to its own logger, or to a service nothing implements. Rendered here and seen
/// nowhere (D590).
pub const TRACE_FORMAT: Var = Var {
    name: "ORBISTOUN_TRACE_FORMAT",
    kind: Kind::Setting,
    summary: "record every string the guest formats and list them at the end - off by default, because a title formats constantly",
    example: "1",
    read_by: "orbistoun-libc",
    effect: Effect::Observes,
};

/// Which arguments of the asynchronous path's resolve call take the identifier and the size.
///
/// Two digits: the argument that receives the identifier, then the one that receives the size.
/// The index says what the answers are and nothing says where they go, so the assignment is
/// named by a run and graded by the guest - six permutations, one boot each (D592).
pub const APR_ANSWER: Var = Var {
    name: "ORBISTOUN_APR_ANSWER",
    kind: Kind::Diagnostic,
    summary: "which arguments of the asynchronous path's resolve call take the identifier and the size, as two digits - the index says what the answers are and nothing says where they go",
    example: "23",
    read_by: "orbistoun-kernel",
    effect: Effect::Intervenes,
};

/// Deliver the file the asynchronous file path resolved, into the buffer its command header
/// names.
///
/// **An experiment, and it intervenes.** Nothing establishes that the buffer means what this
/// assumes: the guest submits a header claiming one command of twenty bytes over storage that is
/// entirely zero, having never called the library function that would put a command there
/// (D587). Reading the resolved file in anyway is a guess, and the guest is the only thing that
/// can grade it - so a verdict under this carries a caveat, which is what `Intervenes` buys.
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
/// The same asymmetry [`TRACE_OPENS`] closes for the filesystem: a run reports the reservations
/// that *failed* and nothing about the ones that succeeded, so a pointer into guest memory
/// cannot be traced back to the call that produced it.
///
/// It is also what a determinism question needs. The arena is bump-allocated, so an address is a
/// function of everything placed before it - two runs whose addresses differ can be seen to
/// differ and not where, unless the sequence is recorded (D581).
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
/// # Why this had to exist before the handoff instrument meant anything
///
/// `orbistoun-cli handoff` poisons one field of the handoff structure and asks whether the
/// guest used it. It set the poison and nothing else - so every run it made was under
/// whatever entry argument the configuration happened to name, and for a bare payload that
/// is not the handoff at all. It poisoned fields of a block the guest never received and
/// reported "no field was reached" about a structure that was never handed over.
///
/// That is the failure the third principle names one level up: a report saying more than its
/// measurement supports. The instrument now selects the argument it is asking about (D399).
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
/// **The gap this closes.** `StubPolicy` is keyed by symbol name and carries a 32-bit
/// code - both right for what it is, and both fatal for the question at a wall. A
/// function with no name cannot be keyed at all, so every attempt to change what it
/// answered silently fell back to the default and was recorded as an experiment that ran
/// and changed nothing. A region base is also 64-bit, which the policy cannot express.
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

/// Which shape of physical memory map the guest is shown.
///
/// # Why this is a diagnostic and the map itself is a setting
///
/// `MapShape` has existed since D218 with three variants and **nothing selected between
/// them**. The apparatus for the experiment was built and never wired to anything a run
/// could turn, so the question it was built to answer - *what map shape will the guest
/// accept?* - sat open while the function it blocks took 67.5% of every guest call.
///
/// A shape does not intervene in the way a poked value does: it changes what the emulator
/// *presents*, which is a legitimate configuration a real machine also has. But it changes
/// the program's inputs, so a verdict earned under one is a verdict about that shape, and
/// [`Effect::Intervenes`] is what makes a run report say so (D356).
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
/// # What it buys
///
/// Every stub answers `0x7fff_0001`, so a placeholder turning up in a guest's argument says
/// *some* unimplemented function produced it and never which. `error_used_as_pointer`'s own
/// action is a person's search - *"find what answered with that code just before"* - and D299
/// says a finding that sends a reader looking must carry what they are to look at.
///
/// Under this, a stub answers `0x7fff_0000 | index`, so the value **is** the attribution. It cost
/// four gigabytes to not have: a work-area sizer answered the placeholder and PPSA28061 handed it
/// to `malloc` twice, and the report could only list the three calls before it (D564, D567).
///
/// # Why it intervenes rather than observes
///
/// It changes what the guest is told. A guest that branches on the exact value takes a different
/// branch, so a verdict under it measures a settings change - which is what [`Effect::Intervenes`]
/// exists to say.
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
/// The other half of [`WATCH`], and deliberately a separate variable: a snapshot says which
/// bytes ended up different, this says who touched them. The cheap one is still the one to
/// run first, and the two compose - the snapshot names the words nobody wrote, and those
/// addresses become the watchpoints for the next run (D276).
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
/// **The one list.** Adding a variable anywhere without adding it here is caught by
/// `every_declared_variable_is_in_the_registry`, because a constant that is not in this
/// array is invisible to the listing and to the typo check - which is the whole failure
/// this crate exists to stop.
pub const REGISTRY: &[Var] = &[
    PORTABLE_MODE,
    DATA_DIR,
    SANDBOX,
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
    APR_ANSWER,
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
    MARK_QUERY,
];

/// The prefix every variable of ours carries.
pub const PREFIX: &str = "ORBISTOUN_";

/// Anything set that looks like ours and is not declared.
///
/// **The reason the registry earns its keep.** A variable spelled wrongly is not an error,
/// it is an absence - so a diagnostic that never ran reports an ordinary result and is
/// believed. This is the only way to notice.
///
/// Sorted, so a warning does not change order between runs for no reason.
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
    /// Hand-listed, which is the one duplication this crate cannot remove - Rust has no way
    /// to enumerate a module's constants. So it is checked instead: this list and
    /// [`REGISTRY`] must agree, and the test below fails when they do not.
    const DECLARED: &[Var] = &[
        super::PORTABLE_MODE,
        super::DATA_DIR,
        super::SANDBOX,
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
        super::APR_ANSWER,
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
        super::MARK_QUERY,
        super::WATCHPOINT,
    ];

    #[test]
    fn every_declared_variable_is_in_the_registry() {
        // **The failure this guards.** A constant that exists and is not in the registry is
        // invisible to the listing and to the typo check, so it would be reported as a
        // misspelling of itself. Same shape as `Paths::all_dirs`, whose equivalent test
        // caught a missing entry the day this was written (D215).
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

    #[test]
    fn every_variable_says_what_it_is_for_and_how_to_set_it() {
        // A listing nobody can act on is a listing nobody reads. The example is what makes
        // it copyable rather than a prompt to go and find the documentation.
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

    #[test]
    fn only_a_diagnostic_may_change_the_program() {
        // **A setting configures the emulator; it does not alter a run in flight.** If one
        // ever needs to be an intervention, that is a change worth arguing about rather
        // than a field somebody flips - a verdict earned under it would carry a caveat, and
        // every ordinary run would carry it too (D227).
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
        // And the distinction has to be in use, or it is decoration: something observes and
        // something intervenes.
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

    #[test]
    fn a_build_always_identifies_itself_somehow() {
        // **Never empty, whatever the situation.** A commit if there is one, when it was
        // compiled if there is not, and the version either way. A footer that renders blank
        // is worse than one that admits it does not know, because a reader cannot tell it
        // apart from a footer nobody wired up (D222).
        let shown = super::build::line();
        assert!(shown.starts_with('v'), "{shown}");
        assert!(shown.contains(env!("CARGO_PKG_VERSION")), "{shown}");
        match super::build::commit() {
            Some(c) => assert!(shown.contains(c), "a known commit must be shown: {shown}"),
            // No commit is the state of this repository today, so this is the live path.
            None => assert!(shown.contains("built"), "{shown}"),
        }
    }

    #[test]
    fn a_diagnostic_may_never_be_persisted_and_a_setting_may() {
        // The rule that decides what a future `.env` may contain. A diagnostic left in a
        // file for three weeks stops being an experiment and becomes an undocumented
        // workaround for a bug nobody found (D185, D221).
        assert!(!Kind::Diagnostic.may_persist());
        assert!(Kind::Setting.may_persist());
        assert!(
            REGISTRY.iter().any(|v| v.kind == Kind::Diagnostic),
            "if nothing is a diagnostic, the distinction is not being used"
        );
    }
}

pub mod build {
    //! What this binary is, for showing somewhere a person will see it.
    //!
    //! # Why a build says which one it is
    //!
    //! A convention carried across projects: the commit is visible in the running application -
    //! a sidebar, a footer, a menu - so a bug report, a screenshot or a run result can be tied
    //! to a tree somebody else can check out. Locally, where there is no commit to name, it
    //! shows when the binary was last compiled instead, which answers the question a developer
    //! is actually asking: *am I looking at my last change?*
    //!
    //! # The gap this closed
    //!
    //! `ORBISTOUN_COMMIT` had been read by the reporting layer since it was written, and
    //! **nothing ever set it** - not CI, not the release workflow, not the shell script. Every
    //! run report ever produced said `binary_commit: "unknown"`. The build script asks git
    //! directly now, so it is populated everywhere with no configuration (D222).
    //!
    //! # Why this module still exists when the implementation is shared
    //!
    //! So that **every front end says the same thing**. `oops_build::stamp!` expands at its call
    //! site and reads *that* crate's version and commit, and only this crate's `build.rs` stamps
    //! one - so calling it in the CLI, in the window and in the service would produce three
    //! answers agreeing by coincidence. It is expanded here, once, in the crate they all depend
    //! on.
    //!
    //! What moved out: asking git, the `-dirty` suffix, hash shortening, the executable's own
    //! mtime, and the calendar arithmetic that used to be passed in as a closure because this
    //! crate did not want to carry it. All of that is `oops_build` now, and prosperous stopped
    //! carrying its own half of it at the same time.

    /// This build.
    #[must_use]
    pub fn stamp() -> oops_build::Stamp {
        oops_build::stamp!()
    }

    /// The commit this was built from, when there was one.
    ///
    /// Carries `-dirty` when the tree had uncommitted changes, because a binary built from
    /// edits is not the commit it would otherwise name - and a report pointing at a commit
    /// somebody can check out has to be true, or it is worse than saying nothing.
    ///
    /// Ask [`oops_build::Stamp::is_exact`] rather than testing this for `Some`: a dirty tree
    /// answers `Some` and names a tree that exists on exactly one machine.
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
