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

/// Which generation of the platform's file structures a guest is given.
pub const STAT_LAYOUT: Var = Var {
    name: "ORBISTOUN_STAT_LAYOUT",
    kind: Kind::Setting,
    summary: "which generation of `struct stat` and `struct dirent` a guest is given - `freebsd11` (default) or `current`",
    example: "current",
    read_by: "orbistoun-fs",
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
    summary: "snapshot <addr>[+len] before the guest runs and report every word it changed",
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
    STACK_FILL,
    DIRECT_FILL,
    BSS_FILL,
    ENTRY_ARGUMENT,
    HANDOFF_FIELDS,
    HANDOFF_POISON,
    DESCRIBE,
    RESOLVE,
    STAT_LAYOUT,
    RUNTIME_GLOBALS,
    HEAP_FILL,
    WATCH,
    WATCHPOINT,
    POKE,
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
        super::STACK_FILL,
        super::DIRECT_FILL,
        super::BSS_FILL,
        super::ENTRY_ARGUMENT,
        super::HANDOFF_FIELDS,
        super::HANDOFF_POISON,
        super::DESCRIBE,
        super::RESOLVE,
        super::STAT_LAYOUT,
        super::RUNTIME_GLOBALS,
        super::HEAP_FILL,
        super::WATCH,
        super::POKE,
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
