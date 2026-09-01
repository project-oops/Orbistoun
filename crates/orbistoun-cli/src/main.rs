//! `orbistoun-cli` - an interaction shim.
//!
//! This binary holds **no logic** (D034). It parses arguments, calls
//! `orbistoun-service`, and formats what comes back. Anything resembling behaviour
//! belongs one layer down, where the GUI and worker mode can reach it too.
//!
//! Commands, ordered by how much of the emulator has to exist for them to work:
//!
//! - `symbols` - everything orbistoun declares. Works today.
//! - `policy` - emit a default stub-policy file to edit. Works today.
//! - `imports` - what a guest module needs. Requires the container parser, and says so
//!   rather than printing an empty list.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use orbistoun_service::{Service, ServiceConfig};

#[derive(Parser, Debug)]
#[command(
    name = "orbistoun-cli",
    about = "High-level console emulation research tool",
    version
)]
struct Cli {
    /// Hex-encoded NID hash suffix, overriding the one orbistoun ships with.
    ///
    /// Rarely needed. The shipped value is documented in
    /// `crates/orbistoun-nid/data/hash-suffix.toml` and verifies itself against
    /// published C library names. See docs/SYMBOLS.md.
    #[arg(long, global = true, default_value = "")]
    suffix_hex: String,

    /// Path to a symbol database (JSON: `suffix_hex` plus `names`).
    ///
    /// Supplies human-readable names for import hashes. Independent of the count of
    /// unresolved imports, which is about what orbistoun implements.
    #[arg(long, global = true)]
    symbols_db: Option<std::path::PathBuf>,

    #[command(subcommand)]
    command: Command,
}

/// Where a supplied word list came from.
///
/// The distinction the provenance record turns on: work this project did, versus work it
/// took from elsewhere. Defaults to `supplied`, because assuming the more generous label
/// is exactly the mistake an audit exists to prevent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum WordSource {
    /// Names this project's own conformance probe reported, running on real hardware.
    ///
    /// Ours, and the audit says so - but it is the one tier of our own work that neither
    /// this machine nor CI can reproduce, because it took a console to produce it (D213).
    Probe,
    /// Came from outside this project.
    Supplied,
}

/// Seconds of guest execution allowed before a run is stopped and reported.
///
/// Long enough for a guest to get well past its startup path, short enough that an
/// unattended sweep over a directory of titles finishes. Overridable per run.
const DEFAULT_GUEST_LIMIT_SECONDS: u64 = 20;

/// Imports a guest may call before it is stopped.
///
/// **Chosen so it cannot truncate a legitimate run.** The busiest title that is doing real
/// work makes 1,735 calls; the one this bounds spins on a single import and made 149
/// million. Twenty million is four orders of magnitude above the first and well below the
/// second, so it stops a runaway at a fixed number and leaves every other title untouched
/// (D238).
const DEFAULT_GUEST_CALL_BUDGET: u64 = 20_000_000;

#[derive(Subcommand, Debug)]
enum Command {
    /// List every library and function orbistoun declares.
    Symbols {
        /// Only show entries whose library or symbol name contains this.
        #[arg(long)]
        filter: Option<String>,
    },
    /// Print a default stub-policy file.
    Policy,
    /// Report a container's structure without executing or fully parsing it.
    Inspect {
        /// Path to a guest executable or module.
        path: std::path::PathBuf,
    },
    /// Reserve the address space a module demands, without executing it.
    Load {
        /// Path to a guest executable or module.
        path: std::path::PathBuf,
        /// Base address to place the module at, decimal or `0x`-prefixed hex.
        ///
        /// Modules link at zero and need one; executables carry absolute addresses
        /// and want zero.
        #[arg(long, default_value = "0", value_parser = parse_address)]
        base: u64,
    },
    /// Execute a guest, in a worker process.
    Run {
        /// Path to a guest executable.
        path: std::path::PathBuf,
        /// Seconds of guest execution to allow before stopping and reporting.
        ///
        /// A guest whose imports are all unimplemented can settle into a loop waiting
        /// for something that will never happen. Zero removes the limit.
        #[arg(long, default_value_t = DEFAULT_GUEST_LIMIT_SECONDS)]
        limit: u64,
        /// Imports the guest may call before stopping and reporting.
        ///
        /// The deterministic limit: two runs of one build stop at the same call, so a
        /// verdict between them measures the change rather than the machine. The clock
        /// above stays as a backstop for a guest that stops calling imports. Zero removes
        /// the budget.
        #[arg(long, default_value_t = DEFAULT_GUEST_CALL_BUDGET)]
        calls: u64,
        /// Present a named console profile for this run instead of the configured machine -
        /// e.g. `ps5-cex-12.40`, the measured reference target. Omit to use `shell.toml`.
        #[arg(long)]
        profile: Option<String>,
    },
    /// Find out which handoff fields a guest's runtime actually uses.
    ///
    /// # Why this is a command rather than a session of experiments
    ///
    /// The structure a payload's runtime is handed is not published, and the only thing that
    /// knows its shape is a guest running against it. Asking takes one run per field - poison
    /// the field with an address nothing maps, and see whether the guest faults on it - and
    /// doing that by hand means twelve edits of a settings file and reading twelve faults.
    ///
    /// **It works with no symbols and no source**, which is the point: the payloads happen to
    /// be open and to carry symbol tables, and nothing else this project will ever load is
    /// going to be (D390).
    Handoff {
        /// Path to a guest executable.
        path: std::path::PathBuf,
        /// How many fields to ask about.
        #[arg(long, default_value_t = 12)]
        fields: u64,
        /// Seconds each run may take.
        #[arg(long, default_value_t = 5)]
        limit: u64,
    },
    /// Turn the loop once against a title, with nobody reading the findings.
    ///
    /// Runs the guest, ranks what went wrong, and takes every step that is mechanical -
    /// sweeping a call's arguments, asking the other diagnostics, arming a watchpoint, and
    /// giving the guest a region when a sweep says it was missing one. Stops at the steps
    /// that are a person's, each with a sentence saying why.
    Turn {
        /// Path to a guest executable.
        path: std::path::PathBuf,
        /// Print what the turn established as a `learn` command, rather than only the steps.
        ///
        /// **Printed rather than written.** What a sweep measures is admissible; deciding to
        /// change a *tracked* file stays a deliberate act with a diff (D291).
        #[arg(long)]
        record: bool,
        /// Write what the turn measured into the learned policy, so the next run carries it.
        ///
        /// **Not a tracked file, and not one a person edits.** `learned.toml` sits beside
        /// `config.toml`, is folded in underneath it, and loses to every entry a person
        /// wrote. Deleting it is a complete undo, and nothing here can override a
        /// deliberate choice (D296).
        #[arg(long)]
        apply: bool,
        /// Check a submitted learned file against what this machine measures.
        ///
        /// **How a contribution is received.** A measurement is checked by measuring again
        /// rather than by trusting it, which is what makes a policy entry a better thing to
        /// accept than a diff: the claim is falsifiable by a command (D297).
        #[arg(long, value_name = "FILE")]
        verify: Option<std::path::PathBuf>,
    },
    /// Survey a module, persist a run report, and show the delta from last time.
    Report {
        /// Path to a guest executable or module.
        path: std::path::PathBuf,
    },
    /// Measure how much of a module's import list a symbol database can name.
    ///
    /// The self-verifying loop: a name list and suffix are correct exactly to the
    /// extent they explain hashes a real module imports. A collision is the proof.
    Verify {
        /// Path to a guest executable or module.
        path: std::path::PathBuf,
    },
    /// Search generated names for ones that hash to a module's unnamed imports.
    ///
    /// Fully clean-room: nothing is consulted. Names are proposed and the hash confirms
    /// or rejects each one, so a reported name is proved rather than guessed. A miss
    /// proves only that the name was not among those tried - extending the vocabulary
    /// is the method.
    Names {
        /// A guest executable or module, or a directory to search every module beneath.
        ///
        /// A directory is one search, not many: the unnamed imports of every module are
        /// unioned first, so the expensive sweep runs once and every module's strings are
        /// tried against every module's imports (D213).
        path: std::path::PathBuf,
        /// Threads to search with. Zero uses one per available core.
        #[arg(long, default_value_t = 0)]
        threads: usize,
        /// Grammar file to use instead of the built-in vocabulary.
        #[arg(long)]
        grammar: Option<std::path::PathBuf>,
        /// Extra newline-separated candidate names to try verbatim.
        #[arg(long)]
        words: Option<std::path::PathBuf>,
        /// Where the `--words` list came from, for the provenance record.
        ///
        /// Names our own conformance probe reported are `probe`: this project working
        /// something out, just not on a machine anyone here can re-run. Anything from
        /// outside is `supplied`, never verifies, and is listed separately by an audit.
        /// Without this every supplied name was recorded as though it came from the
        /// repository's own published-standard list, which was untrue (D119).
        #[arg(long, value_enum, default_value_t = WordSource::Supplied)]
        words_from: WordSource,
        /// Write the names found to a symbol database at this path.
        #[arg(long)]
        out: Option<std::path::PathBuf>,
        /// Write the hashes still unnamed to this path, as a prioritised work list.
        #[arg(long)]
        wanted: Option<std::path::PathBuf>,
        /// Also read candidates out of what a previous run captured from guest memory.
        ///
        /// The one source here that needed the guest to actually execute. Off by default
        /// because it depends on a run having happened and on dumps having been forced,
        /// and a source that silently contributes nothing is worse than one you asked for.
        #[arg(long)]
        from_trace: bool,
    },
    /// Record something learned about a guest function.
    ///
    /// The loop produces facts - what a function is for, what its arguments are, what it
    /// does at its edges - and until they are written down they exist only in a
    /// terminal. This appends them to the knowledge file so a session records a finding
    /// with a command rather than by hand-formatting TOML (D122).
    Learn(Learned),
    /// Read and update the per-title compatibility record.
    ///
    /// The half of a title file that says what happened, as opposed to what is
    /// configured. Written from a trace rather than by hand, so an entry is a
    /// transcription of a measurement rather than an opinion about one (D182).
    Compat {
        #[command(subcommand)]
        action: CompatAction,
    },
    /// The test corpus: fetch pinned guests from configured sources, run them, record results.
    ///
    /// A manifest of sources (`corpus/sources.toml`) names where guests come from; `sync`
    /// downloads them into gitignored `titles/` and pins each by hash; `run` turns the loop over
    /// every one, recording to `compat/` exactly as a hand `run` does. This is D042 made a verb:
    /// the breadth signal - does anything real get further this week - regenerated on demand.
    Corpus {
        #[command(subcommand)]
        action: CorpusAction,
    },
    /// Gather what this machine has to contribute, or check what somebody sent.
    ///
    /// **The loop does not need this repository**, and this is what makes that true in
    /// practice. Somebody running a binary against a title nobody here owns turns the same
    /// oracle; without a way to collect what they found, it stays on their machine.
    Submit {
        #[command(subcommand)]
        action: SubmitAction,
    },
    /// Print what is known about guest functions.
    Knows {
        /// A function name, or a fragment to match. Omit for a summary.
        pattern: Option<String>,
    },
    /// Answer the conformance probe's command protocol, so one driver can drive either.
    ///
    /// **The point is comparison.** obSCEne speaks a command protocol and orbistoun now
    /// answers the same commands, so a driver can be pointed at a probe or at this and
    /// diff the records live - rather than running both separately and reconciling files
    /// afterwards (`docs/BACKLOG.md`, D056).
    ///
    /// **Off unless asked for, and never in an automated path.** This opens a socket,
    /// which is exactly what the emulator should not do unprompted, and a check that
    /// listens on a port behaves differently depending on what else is on the machine.
    ///
    /// Only `report` is served today. A `call` needs a guest that is loaded and running,
    /// and this holds a service rather than a run - so the capability is not announced,
    /// because announcing one and then refusing it misleads a driver that has already
    /// planned around the reply.
    Serve {
        /// Address to listen on. Loopback by default, deliberately.
        ///
        /// A responder reachable from a network is one anything on that network can
        /// drive. Widening this is a decision worth typing out.
        #[arg(long, default_value = "127.0.0.1:9599")]
        bind: String,

        /// Serve without requiring a session secret.
        ///
        /// Sound on loopback, where the peer is something the same person started.
        /// Refused when `--bind` is not a loopback address, because "I did not want a
        /// password" and "anything on this network may invoke this" are different
        /// decisions and only one of them was made here.
        #[arg(long)]
        no_key: bool,

        /// Serve one session and exit, rather than accepting until interrupted.
        #[arg(long)]
        once: bool,
    },
    /// Emit the generated numbers block for the documentation, or check it for drift.
    ///
    /// `docs/PROJECT_STATUS.md` says its numbers are printed by the tool rather than
    /// counted by hand, and that a number no command produces will be wrong within a week.
    /// Both were true and the numbers drifted anyway - two files disagreed with the tool
    /// and with each other (D240).
    Status {
        /// Rewrite the block in every file that carries the markers.
        #[arg(long)]
        write: bool,
        /// Fail if any file's block differs from what the tool produces now.
        #[arg(long)]
        check: bool,
    },
    /// Show where orbistoun reads and writes, and whether it is in portable mode.
    ///
    /// The first question anyone asks when an artifact is not where they expected, and
    /// portable mode moves all of them at once - so guessing is expensive and printing
    /// is free.
    Paths,
    /// List every environment variable orbistoun reads, and what is set right now.
    ///
    /// **Because a variable typed wrongly is an absence, not an error.** A flag spelled
    /// wrongly is refused; `ORBISTOUN_STACK_FIL=5a` runs an ordinary experiment and reports
    /// an ordinary result. This is where you check what the names actually are, rather than
    /// finding a table in a document that somebody copied by hand (D221).
    Env,
    /// Emit every open question, ranked by how often a guest calls the function.
    ///
    /// **The handoff to a hardware probe.** Every `assumptions` line in the knowledge base
    /// is a thing this project has written down that it does not know, and each is
    /// answerable by measurement. Scattered across per-function files they are a candour
    /// exercise; gathered and ranked they are a work queue (D196).
    Questions {
        /// How many to show. Omit for all of them.
        #[arg(long)]
        top: Option<usize>,
        /// Emit JSON, for a probe or an agent to consume rather than a person to read.
        #[arg(long)]
        json: bool,
    },
    /// Rank what to implement next, across every guest run so far.
    ///
    /// Reads the call traces every run persists and totals them. A static import dump
    /// says what a module *might* call; this says what it actually did, and how often -
    /// which is the only thing that says where to spend the next hour.
    Worklist {
        /// How many entries to show.
        #[arg(long, default_value_t = 25)]
        top: usize,
    },
    /// Rebuild the standard-library word list from a FreeBSD source tree.
    ///
    /// The target C library is FreeBSD-derived, and FreeBSD publishes exactly what its
    /// libraries export. Harvesting those beats a hand-written list on every count:
    /// bigger, current, and citable to a named source at a named revision rather than
    /// to somebody's memory of the standards.
    ///
    /// Only the `Symbol.map` files are read, so a sparse checkout is plenty:
    ///
    ///     git clone --filter=blob:none --sparse https://github.com/freebsd/freebsd-src
    ///     cd freebsd-src && git sparse-checkout set lib/libc lib/libthr lib/msun lib/libutil
    Harvest {
        /// Path to a FreeBSD source checkout.
        source: std::path::PathBuf,
        /// Where to write the word list.
        #[arg(long, default_value = "crates/orbistoun-names/data/standard.txt")]
        out: std::path::PathBuf,
        /// How to describe the source in the file's header, e.g. a tag or commit.
        #[arg(long)]
        revision: Option<String>,
    },
    /// Re-derive every name in a symbol database from this repository's own inputs.
    ///
    /// The provenance check. A name this repository can produce is self-evidently
    /// derivable; one it cannot is the one that needs explaining. Cheap enough to run
    /// on every commit, which is what makes it evidence rather than a claim.
    Audit {
        /// Path to a symbol database.
        database: std::path::PathBuf,
        /// Grammar file to check against instead of the built-in vocabulary.
        #[arg(long)]
        grammar: Option<std::path::PathBuf>,
        /// Compare the unaccounted set against a written-down ceiling instead of failing
        /// on any at all.
        ///
        /// **Because a gate that is red on every run is a gate nobody reads.** Two hundred
        /// vendor names cannot be regenerated by the current grammar, and that is a known,
        /// recorded, slowly-shrinking fact rather than a regression. Failing the build on
        /// it every time trains people to ignore the job - and then the *new* unaccounted
        /// name, the one that arrived without anybody deciding, goes past unread.
        ///
        /// The file may only shrink: a name unaccounted and unlisted fails, and a name
        /// listed that has since been accounted for also fails, so the ceiling cannot
        /// quietly become permission. Same mechanism as the duplicate decision numbers and
        /// the line-continuation backlog (D208).
        #[arg(long)]
        ceiling: Option<std::path::PathBuf>,
        /// Search the whole space for names carrying no derivation record.
        ///
        /// Slow - it walks every candidate per unaccounted name - but it is the only
        /// way to answer "could this have been generated?" for a name that arrived
        /// without a record.
        #[arg(long)]
        deep: bool,
        /// Re-read every module a static record names, and confirm it contains the string.
        ///
        /// The tier of claim CI structurally cannot check, because it needs the guest
        /// material - so it is checked here, by whoever has it. Off by default: it reads
        /// and scans every module in the corpus, which is far too slow for the gate that
        /// runs on every commit, and reporting "unchecked" for a corpus that is simply
        /// absent would be noise in the place the gate is read (D213).
        #[arg(long)]
        verify_harvest: bool,
        /// Re-derive generated records the current grammar no longer confirms, and write
        /// them back.
        ///
        /// **Because the loop invalidates its own records.** An index is a position in an
        /// enumeration over the vocabularies, so every word learned from a confirmed name
        /// (D195) renumbers the candidates built from it - and names that were verified
        /// last run fall onto the unaccounted ceiling, a file whose whole rule is that it
        /// may only shrink. One sweep repairs every stale record at once (D213).
        #[arg(long)]
        repair: bool,
    },
    /// Report what a guest module imports, without executing it.
    Imports {
        /// Path to a guest executable.
        path: std::path::PathBuf,
    },
    /// Ask a live probe one question and print what it answers.
    ///
    /// The triage loop, in its smallest form: when this emulator cannot say what a function
    /// does, the console can be asked directly rather than guessed at.
    ///
    /// Prints the answer honestly - `returned 0x2`, `died`, `refused unauthorised` - because
    /// a command that did not answer must never read as one that did.
    Ask {
        /// `host:port` of the listening probe.
        address: String,
        /// Session secret, shown by the probe when it starts listening.
        #[arg(long)]
        key: Option<String>,
        /// The verb, then its arguments - `call 0x80019c40 0x0`, `read 0x8003f510 0x20`.
        #[arg(required = true, num_args = 1..)]
        command: Vec<String>,
        /// Seconds to wait before calling it a timeout.
        #[arg(long, default_value_t = 30)]
        budget: u64,
        /// Render the answer as the knowledge entry it would become.
        ///
        /// Shows the grade, the caveat, and - for a handle or pointer - that the value was
        /// recorded rather than handed to a guest. Printed, never written: a corpus is
        /// evidence, and evidence being read is not evidence being believed.
        #[arg(long)]
        as_knowledge: bool,
        /// What the operator says this ran on. Only a label; the connection ignores it.
        #[arg(long)]
        device: Option<String>,
        /// Assert the device named is the target platform itself.
        #[arg(long)]
        is_target: bool,
    },
    /// Drive a live session against a listening probe and record what it says.
    ///
    /// The probe listens and this connects: a console has no DNS and no configuration
    /// file, but it has an address a person can read off a screen.
    ///
    /// The transcript is written out, and that file is the product - a session is
    /// transient, a corpus is not.
    Session {
        /// `host:port` of the listening probe.
        address: String,
        /// Session secret, shown by the probe when it starts listening.
        ///
        /// Generated per startup and replaced by a restart, so a key that worked yesterday
        /// is a stale key today rather than a wrong one.
        #[arg(long)]
        key: Option<String>,
        /// Where to write the transcript.
        #[arg(long)]
        out: std::path::PathBuf,
        /// What the operator asserts this ran on.
        #[arg(long)]
        device: Option<String>,
        /// Firmware or version, where the operator knows it.
        #[arg(long)]
        firmware: Option<String>,
        /// Assert that the device named is the target platform itself.
        ///
        /// Usually unnecessary - `--device` carries it. Not "is it real hardware": a Deck
        /// is real and is not the target.
        #[arg(long)]
        is_target: bool,
        /// Seconds to wait for any one command before calling it a timeout.
        #[arg(long, default_value_t = 30)]
        budget: u64,
    },
    /// Read a probe transcript or corpus and report what it establishes.
    ///
    /// Answers the question worth asking before trusting any of it: how many of these
    /// results are facts about the target, rather than somebody's reasoning or a
    /// measurement taken on a different device. Needs no hardware - it reads files.
    Probe {
        /// A transcript or corpus file.
        path: std::path::PathBuf,
        /// What the operator asserts this ran on - a console, or a named emulator.
        ///
        /// Asked for rather than read off the records, because a probe cannot certify its
        /// own machine: inside an emulator it reports the emulator's version as the
        /// platform's, so a `target` arriving on the wire is a claim and not evidence.
        #[arg(long)]
        device: Option<String>,
        /// Firmware or version, where the operator knows it.
        #[arg(long)]
        firmware: Option<String>,
        /// Assert that the device named is the target platform itself.
        ///
        /// Usually unnecessary: `--device` carries the answer, and a name this project
        /// knows to be a stand-in - a Deck, a host build, a named emulator - is treated as
        /// one without being told twice.
        ///
        /// **Not "is it real hardware".** A Steam Deck is real hardware and is not the
        /// target; measurements taken on it describe a Deck. The question grading turns on
        /// is whether the silicon was the thing being emulated.
        #[arg(long)]
        is_target: bool,
        /// Render what was established as knowledge entries, to standard output.
        ///
        /// Printed rather than written. Merging into the knowledge base is a separate,
        /// deliberate act - a corpus is evidence, and evidence being read is not the same
        /// as evidence being believed.
        #[arg(long)]
        as_knowledge: bool,
    },
    /// Analyse a directory of shader binaries and rank what blocks translation.
    ///
    /// Answers the only question the shader work has: which single instruction, if
    /// supported, would unblock the most shaders. Needs no GPU, no driver and no
    /// running guest - it reads bytes.
    Shaders {
        /// Directory of shader binaries, one per file.
        path: std::path::PathBuf,
        /// Show only the top N blockers. Omit for the whole list.
        #[arg(long)]
        top: Option<usize>,
    },
    /// Show how the firmware skeleton lays libkernel out: what stub each export vaddr gets,
    /// which are unimplemented, and where a stub overruns its neighbour. The collision that
    /// corrupted getpid was invisible until it broke something; this makes the layout legible.
    Firmware {
        /// Show every export, not just collisions and unimplemented ones a payload might reach.
        #[arg(long)]
        all: bool,
    },
}

/// Everything `learn` records about one function.
///
/// A named struct rather than fields on the variant so the command takes one argument
/// instead of ten, and so the field list lives in exactly one place - it was previously
/// declared, destructured and re-passed, which is three chances to forget the new one.
#[derive(clap::Args, Debug)]
struct Learned {
    /// The function this is about.
    function: String,
    /// Which library it belongs to. Decides which file the entry lands in.
    #[arg(long)]
    library: String,
    /// How many integer arguments it takes, where that has been established.
    #[arg(long)]
    arity: Option<u8>,
    /// What the function is for.
    #[arg(long)]
    purpose: Option<String>,
    /// Behaviour a reimplementation would otherwise get wrong. Repeatable.
    #[arg(long = "edge")]
    edges: Vec<String>,
    /// A guest module it was seen in. Repeatable. Title ids, never paths.
    #[arg(long = "seen-in")]
    seen_in: Vec<String>,
    /// How the behaviour recorded here was established.
    ///
    /// Required whenever anything beyond a name is recorded, and there is deliberately
    /// no value meaning "I already knew it": every option names something that could
    /// contradict it. See `Oracle`.
    #[arg(long, value_enum)]
    known: Option<KnownBy>,
    /// Where to look to check it - a standard clause, a source file and revision, a
    /// probe identifier. Required by `--known published` and `--known measured`.
    #[arg(long)]
    cites: Option<String>,
    /// A specific claim in this entry that `--known` does not cover. Repeatable.
    ///
    /// Each one is a question real hardware could settle, so this is a worklist rather
    /// than an apology.
    #[arg(long = "assumes")]
    assumptions: Vec<String>,
    /// Anything else worth keeping.
    #[arg(long)]
    note: Option<String>,
}

/// What to do with a submission.
#[derive(clap::Subcommand, Debug)]
enum SubmitAction {
    /// Gather this machine's measurements and title results into one directory.
    Export {
        /// Where to write the bundle.
        #[arg(long, default_value = "submission")]
        out: std::path::PathBuf,
        /// Where the title records live.
        #[arg(long, default_value = "compat")]
        compat_dir: std::path::PathBuf,
    },
    /// Compare a received bundle against what this machine found.
    ///
    /// Re-derives rather than trusts. A claim this machine never measured is reported as
    /// unmeasured rather than as a contradiction - "we did not look" and "it is wrong" are
    /// different facts, which is what the `known_by` ladder exists to hold.
    Check {
        /// The bundle directory somebody sent.
        dir: std::path::PathBuf,
        /// Where this machine's title records live.
        #[arg(long, default_value = "compat")]
        compat_dir: std::path::PathBuf,
    },
}

/// What to do with the compatibility record.
#[derive(clap::Subcommand, Debug)]
enum CompatAction {
    /// Show every recorded title, furthest first.
    List {
        /// Where the records live.
        #[arg(long, default_value = "compat")]
        dir: std::path::PathBuf,
    },
    /// Render every record as a markdown table, ranked furthest first, into a tracked file.
    ///
    /// The same ranking `list` prints, as a document a person can read in the repository. A
    /// guest with a screenshot beside its record gets the image embedded.
    Markdown {
        /// Where the records live.
        #[arg(long, default_value = "compat")]
        dir: std::path::PathBuf,
        /// Where to write the table.
        #[arg(long, default_value = "COMPATIBILITY.md")]
        out: std::path::PathBuf,
        /// Directory of `<title>.png` screenshots, relative to the repo root.
        #[arg(long, default_value = "compat/screenshots")]
        shots: std::path::PathBuf,
    },
    /// Record what the last run of this title achieved.
    Record {
        /// Path to the guest executable that was run.
        path: std::path::PathBuf,
        /// Where the records live.
        #[arg(long, default_value = "compat")]
        dir: std::path::PathBuf,
        /// Anything the numbers do not say.
        #[arg(long)]
        note: Option<String>,
        /// Record even when the previous entry was better.
        ///
        /// For a deliberate correction - a previous entry measured wrongly, or a
        /// regression worth recording as the new truth. Never the default, because an
        /// automatic best-ever that quietly moves backwards is not a record of anything.
        #[arg(long)]
        force: bool,
    },
}

/// What to do with the test corpus.
#[derive(clap::Subcommand, Debug)]
enum CorpusAction {
    /// Show the manifest: every source, its assets, and whether each is pinned yet.
    List {
        /// The manifest to read.
        #[arg(long, default_value = "corpus/sources.toml")]
        manifest: std::path::PathBuf,
    },
    /// Fetch every source's assets into `titles/`, pinning or verifying each by hash.
    Sync {
        /// Only this source, by name. Omit for all.
        #[arg(long)]
        source: Option<String>,
        /// The manifest to read and pin into.
        #[arg(long, default_value = "corpus/sources.toml")]
        manifest: std::path::PathBuf,
        /// Where guest bytes land. Gitignored; never tracked.
        #[arg(long, default_value = "titles")]
        titles: std::path::PathBuf,
    },
    /// Sync, then run every guest and record what it reached to `compat/`.
    Run {
        /// Only this source, by name. Omit for all.
        #[arg(long)]
        source: Option<String>,
        /// The manifest to read.
        #[arg(long, default_value = "corpus/sources.toml")]
        manifest: std::path::PathBuf,
        /// Where guest bytes land.
        #[arg(long, default_value = "titles")]
        titles: std::path::PathBuf,
        /// Seconds each guest may run before it is stopped and reported.
        #[arg(long, default_value_t = DEFAULT_GUEST_LIMIT_SECONDS)]
        limit: u64,
        /// Imports each guest may call before it is stopped and reported.
        #[arg(long, default_value_t = DEFAULT_GUEST_CALL_BUDGET)]
        calls: u64,
        /// Present a named console profile for the runs, e.g. `ps5-cex-12.40`.
        #[arg(long)]
        profile: Option<String>,
    },
}

/// How a behavioural claim was established, as the command line spells it.
///
/// A mirror of [`orbistoun_hle::knowledge::Oracle`] rather than a re-export because clap's
/// derive needs its own trait on the type, and the knowledge crate should not grow a
/// command-line dependency to satisfy it. The test below holds the two in step.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
enum KnownBy {
    /// A published standard or published source that specifies this function.
    Published,
    /// Measured on real hardware by a conformance probe.
    Measured,
    /// The guest proceeded when answered this way, and stopped otherwise.
    GuestObserved,
    /// Nobody knows; the value recorded is a placeholder chosen to be least harmful.
    Assumed,
}

impl From<KnownBy> for orbistoun_hle::knowledge::Oracle {
    fn from(value: KnownBy) -> Self {
        match value {
            KnownBy::Published => Self::Published,
            KnownBy::Measured => Self::Measured,
            KnownBy::GuestObserved => Self::GuestObserved,
            KnownBy::Assumed => Self::Assumed,
        }
    }
}

/// Parses an address in decimal or `0x`-prefixed hex.
///
/// Addresses are overwhelmingly written in hex in this domain, so accepting only
/// decimal would be a papercut on every single use.
fn parse_address(text: &str) -> Result<u64, String> {
    let t = text.trim();
    let parsed = t.strip_prefix("0x").map_or_else(
        || t.parse::<u64>().map_err(|e| e.to_string()),
        |hex| u64::from_str_radix(hex, 16).map_err(|e| e.to_string()),
    );
    parsed.map_err(|e| format!("{t:?} is not an address: {e}"))
}

/// Formats a fraction as a percentage without a lossy `usize` cast.
fn percent(part: usize, whole: usize) -> f64 {
    if whole == 0 {
        return 0.0;
    }
    let part = f64::from(u32::try_from(part).unwrap_or(u32::MAX));
    let whole = f64::from(u32::try_from(whole).unwrap_or(u32::MAX));
    part / whole * 100.0
}

/// `symbols` - everything orbistoun declares.
fn cmd_symbols(service: &Service, filter: Option<&str>) {
    let mut shown = 0_usize;
    let mut implemented = 0_usize;
    for d in service.declared_symbols() {
        let matches = filter.is_none_or(|f| d.library.contains(f) || d.symbol.contains(f));
        if matches {
            // A leading marker rather than a trailing column: it lines up down the left
            // edge, so "how much of this is real" is answerable by looking rather than
            // by reading every row to the end.
            println!(
                "{} {:#018x}  {:<16}  {:<40}  argc={}",
                if d.implemented { "*" } else { " " },
                d.nid,
                d.library,
                d.symbol,
                d.arity
            );
            shown += 1;
            implemented += usize::from(d.implemented);
        }
    }
    eprintln!(
        "{shown} declared, {implemented} implemented (*), {} on stubs",
        shown - implemented
    );
}

/// `inspect` - a container's structure, without executing or fully parsing it.
fn cmd_inspect(service: &Service, path: &std::path::Path) -> Result<()> {
    let info = service.inspect_path(path)?;
    let wrapper = match info.wrapper {
        orbistoun_service::WrapperInfo::None => "none (bare ELF)".to_owned(),
        orbistoun_service::WrapperInfo::Wrapped {
            previous_generation,
            segment_count,
            stated_size,
        } => {
            let generation = if previous_generation {
                "previous generation"
            } else {
                "current generation"
            };
            format!("{generation}, {segment_count} segments, stated size {stated_size}")
        }
    };
    println!("wrapper {wrapper}");
    println!("elf offset {}", info.elf_offset);
    println!("entry {:#x}", info.entry);
    println!("e_type {:#06x}", info.e_type);
    println!("machine {:#06x}", info.machine);
    println!(
        "osabi {}{}",
        info.osabi,
        if info.osabi == 9 { " (FreeBSD)" } else { "" }
    );
    println!("program headers  {}", info.program_headers);
    println!("vendor segments  {}", info.vendor_segments);
    println!(
        "mapped segments  {:?}  (headers the wrapper locates data for)",
        info.mapped_segments
    );
    match info.proc_param.as_ref() {
        None => println!("proc param       none"),
        Some(p) => {
            println!(
                "proc param       size {:#x}  magic {}  entries {}  sdk {:#010x}",
                p.size,
                if p.magic_ok { "ORBI" } else { "absent" },
                p.entry_count,
                p.sdk_version,
            );
            println!(
                "  pointers       libc {:#x}  mem {:#x}  third {:#x}",
                p.libc_param_vaddr, p.mem_param_vaddr, p.third_param_vaddr
            );
            if p.mem_param_vaddr == 0 {
                println!("  mem param      none");
            } else {
                match p.mem_param_size {
                    Some(size) => println!(
                        "  mem param      vaddr {:#x}  size {:#x}",
                        p.mem_param_vaddr, size
                    ),
                    None => println!(
                        "  mem param      vaddr {:#x}  (maps to no segment)",
                        p.mem_param_vaddr
                    ),
                }
                // Raw, not interpreted: the field layout inside the block is not established
                // from a citable source, so a value is shown at its offset and named nothing.
                for (offset, value) in &p.mem_param_nonzero {
                    println!("    +{offset:#04x}       {value:#x}");
                }
                if p.mem_param_size.is_some() && p.mem_param_nonzero.is_empty() {
                    println!("    (all zero past the size field)");
                }
            }
        }
    }
    Ok(())
}

/// `report` - survey a module, persist a run report, and show the delta.
///
/// The operation the iterative loop uses: what does this need, and did the last change
/// help.
fn cmd_report(service: &Service, path: &std::path::Path) -> Result<()> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
    let out = service.survey_and_report(path, now)?;

    println!("run {}", out.report.run_id);
    println!("title {}", out.report.inputs.title_hash);
    println!("reached {:?}", out.report.reached);
    println!(
        "imports {} unresolved of {}",
        out.report.counts.distinct_unresolved,
        out.report
            .survey
            .as_ref()
            .map_or(0, orbistoun_service::SurveySummary::total)
    );
    if let Some(first) = out.report.first_unmet.as_ref() {
        println!(
            "first gap {:#018x}  {}",
            first.nid,
            first.symbol.as_deref().unwrap_or("<unknown>")
        );
    }

    match out.diff.as_ref() {
        None => println!("diff none - first run for this title"),
        Some(d) => {
            println!("diff vs {} ({:?})", d.previous, d.phase_change);
            if !d.same_inputs {
                println!("          inputs changed - a difference may be config drift");
            }
            println!(
                "          {} newly resolved, {} newly unresolved",
                d.newly_resolved.len(),
                d.newly_unresolved.len()
            );
        }
    }
    if let Some(at) = out.written_to.as_ref() {
        println!("written {}", at.display());
    }
    Ok(())
}

/// `imports` - what a guest module needs, without executing it.
fn cmd_imports(service: &Service, path: &std::path::Path) -> Result<()> {
    let survey = service.survey_path(path)?;
    println!("entry {:#x}", survey.entry);
    for i in &survey.imports {
        // **Data is marked, because for data the answer is wrong in kind.** An
        // unresolved function is something orbistoun has not written yet; an import
        // naming data that lands on a thunk has been given instruction bytes to
        // dereference, and a listing that showed the two identically would hide the
        // worse of them (D307).
        let data = if i.kind == orbistoun_proto::ImportKind::Object {
            "  [data]"
        } else {
            ""
        };
        println!(
            "{:#018x}  {}  {}{}",
            i.nid,
            i.library.as_deref().unwrap_or("?"),
            i.symbol.as_deref().unwrap_or("<unknown>"),
            data
        );
    }
    let data = survey
        .imports
        .iter()
        .filter(|i| i.kind == orbistoun_proto::ImportKind::Object)
        .count();
    eprintln!(
        "{} imports, {} unresolved",
        survey.total(),
        survey.unresolved()
    );
    if data > 0 {
        eprintln!(
            concat!(
                "{} of them name data, not a function - a thunk is the wrong kind of ",
                "answer there and orbistoun has no other one yet"
            ),
            data
        );
    }
    Ok(())
}

/// `load` - reserve the address space a module demands, without executing it.
fn cmd_load(service: &Service, path: &std::path::Path, base: u64) -> Result<()> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let layout = service.load_layout(&bytes, base)?;

    println!("base {:#x}", layout.base);
    println!(
        "span {:#x} .. {:#x}  ({} KiB)",
        layout.span_base,
        layout.span_base.saturating_add(layout.span_len),
        layout.span_len / 1024
    );
    for s in &layout.segments {
        let perms = format!(
            "{}{}{}",
            if s.read { 'r' } else { '-' },
            if s.write { 'w' } else { '-' },
            if s.execute { 'x' } else { '-' }
        );
        println!(
            "  [{:2}] {:#014x} {:>10}  {perms}",
            s.index, s.vaddr, s.memsz
        );
    }
    match layout.reservation_failure.as_deref() {
        None => eprintln!(
            "span placed: {} segments fit at {:#x}",
            layout.segments.len(),
            layout.span_base
        ),
        Some(why) => eprintln!("span NOT placed: {why}"),
    }
    Ok(())
}

/// `run` - execute a guest, in a worker process.
///
/// Both shims go through the worker uniformly (D033): the CLI gets no in-process fast
/// path just because a CLI crash is cheap. One execution path means the GUI's protocol
/// is exercised on every CLI run rather than only when someone opens the GUI.
/// Validates a `--profile` name and, if good, sets the env the spawned worker reads.
///
/// Validated here, before the worker is spawned, so an unknown name fails fast with the
/// alternatives rather than the worker silently falling back to the configured machine (D409).
fn set_profile_for_run(profile: Option<&str>) -> Result<()> {
    if let Some(name) = profile {
        if orbistoun_shell::profiles::machine(name).is_none() {
            anyhow::bail!(
                "no console profile named {name} - known profiles: {}",
                orbistoun_shell::profiles::names().join(", ")
            );
        }
        // SAFETY: single-threaded here, and the spawned worker inherits this at startup.
        unsafe { std::env::set_var("ORBISTOUN_MACHINE_PROFILE", name) };
    }
    Ok(())
}

fn cmd_run(
    path: &std::path::Path,
    limit: u64,
    calls: u64,
    profile: Option<&str>,
    symbols_db: Option<&std::path::Path>,
) -> Result<()> {
    set_profile_for_run(profile)?;
    let mut worker =
        orbistoun_worker::WorkerHandle::spawn_self().context("spawning a worker process")?;

    // Read before the run, because the run overwrites it. The comparison is the whole
    // point of keeping traces at all.
    let before = previous_trace(path);

    let events = worker
        .request(&orbistoun_proto::Request::Run {
            path: path.to_path_buf(),
            symbols_db: symbols_db.map(std::path::Path::to_path_buf),
            // Zero is the explicit way to ask for no limit, rather than a magic
            // sentinel: an unlimited run is a deliberate choice, not a default.
            limit_seconds: (limit > 0).then_some(limit),
            call_budget: (calls > 0).then_some(calls),
        })
        .context("driving the worker")?;

    for event in &events {
        match event {
            orbistoun_proto::Event::Reached { phase } => println!("reached {phase:?}"),
            // The terminal event repeats the furthest phase; the progress lines above
            // already said it, so only the outcome is new here.
            orbistoun_proto::Event::Terminated { outcome, .. } => match outcome {
                orbistoun_proto::Outcome::Halted { reason } => println!("halted {reason}"),
                other => println!("outcome {other:?}"),
            },
            orbistoun_proto::Event::Failed { error } => println!("failed {error}"),
            other => println!("event {other:?}"),
        }
    }

    worker.shutdown().context("shutting the worker down")?;

    // After the worker has exited, so the trace it wrote is complete.
    if let Some(after) = previous_trace(path) {
        report_progress(before.as_ref(), &after);
        record_compat(path, &after);
    }
    Ok(())
}

/// What one run said about one field.
enum Used {
    /// The guest faulted on the poisoned value, so it used the field.
    Yes {
        /// What it did with it - read, wrote, or called.
        kind: String,
        /// Where in the guest it did so.
        site: String,
    },
    /// The run ended somewhere else, so it never reached this field.
    No {
        /// What did happen, for the reader who wants to know the run was not simply broken.
        instead: String,
    },
}

/// `handoff` - which fields of the handoff structure a runtime uses.
///
/// One run per field. A fault **on the poisoned address** means the field was used; anything
/// else means the run never reached it, which is as much of an answer.
fn cmd_handoff(path: &std::path::Path, fields: u64, limit: u64) -> Result<()> {
    println!("asking {} which handoff fields it uses", path.display());
    println!("  one run per field, poisoned with an address nothing maps\n");

    let mut used = Vec::new();
    for field in 0..fields {
        let verdict = ask_about_field(path, field, limit)?;
        match &verdict {
            Used::Yes { kind, site } => {
                // The kind carries its own preposition - "read of", "instruction fetch
                // from" - so the value goes straight after it, as the fault report does.
                println!("  field {field:2}  USED         {kind} the field's own value, at {site}");
                used.push(field);
            }
            Used::No { instead } => println!("  field {field:2}  not reached  ({instead})"),
        }
    }

    println!();
    if used.is_empty() {
        println!("no field was reached - the run stops before the runtime reads its argument");
        return Ok(());
    }
    let names: Vec<String> = used.iter().map(u64::to_string).collect();
    println!("fields used: {}", names.join(", "));
    println!(
        "everything else was never reached, which is a fact about this run rather than about the structure"
    );
    Ok(())
}

/// Runs the guest once with one field poisoned, and reads what happened.
fn ask_about_field(path: &std::path::Path, field: u64, limit: u64) -> Result<Used> {
    // **The handoff argument, selected rather than assumed.** This asked which field of the
    // handoff structure a guest used while the run it measured was handed whatever the
    // configuration named - which for a bare payload is not the handoff at all. It poisoned
    // fields of a block the guest never received, and answered "no field was reached" about a
    // structure that was never handed over (D399).
    //
    // SAFETY: single-threaded here, and the child process reads it at startup. Both variables
    // are removed again below so a later run is not silently still under them.
    unsafe { std::env::set_var(orbistoun_env::ENTRY_ARGUMENT.name, "handoff") };
    // SAFETY: as above.
    unsafe { std::env::set_var(orbistoun_env::HANDOFF_POISON.name, field.to_string()) };
    let outcome = run_quietly(path, limit);
    // SAFETY: as above.
    unsafe { std::env::remove_var(orbistoun_env::HANDOFF_POISON.name) };
    // SAFETY: as above.
    unsafe { std::env::remove_var(orbistoun_env::ENTRY_ARGUMENT.name) };
    outcome?;

    // **The worker's own constants, not a copy of them.** This has to recognise the exact
    // address the other side planted, and two numbers that must agree are two numbers that
    // can drift - the same reason the harvested constants moved to one crate (D385).
    let poisoned = orbistoun_worker::POISON_BASE + field * orbistoun_worker::POISON_STRIDE;
    let Some(trace) = previous_trace(path) else {
        return Ok(Used::No {
            instead: "the run left no trace".to_owned(),
        });
    };
    let Some(fault) = trace.fault.as_ref() else {
        return Ok(Used::No {
            instead: "the run did not fault".to_owned(),
        });
    };
    if fault.address == poisoned {
        return Ok(Used::Yes {
            kind: fault.kind.clone(),
            site: match (&fault.region, fault.offset) {
                (Some(region), Some(offset)) => format!("{region}+{offset:#x}"),
                _ => format!("{:#x}", fault.instruction_pointer),
            },
        });
    }
    Ok(Used::No {
        instead: format!("{} {:#x}", fault.kind, fault.address),
    })
}

/// One run, with the guest's own output kept out of the way.
fn run_quietly(path: &std::path::Path, limit: u64) -> Result<()> {
    let mut worker =
        orbistoun_worker::WorkerHandle::spawn_self().context("spawning a worker process")?;
    let _ = worker
        .request(&orbistoun_proto::Request::Run {
            path: path.to_path_buf(),
            symbols_db: None,
            limit_seconds: (limit > 0).then_some(limit),
            call_budget: None,
        })
        .context("driving the worker")?;
    worker.shutdown().context("shutting the worker down")
}

/// `verify` - how much of a module's import list a symbol database can name.
fn cmd_verify(service: &Service, path: &std::path::Path) -> Result<()> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let (explained, total) = service.explain_imports(&bytes)?;
    println!(
        "{explained} of {total} imports named ({:.1}%)",
        percent(explained, total)
    );
    if service.symbol_db_len().is_none() {
        eprintln!("no --symbols-db given, so nothing could be named");
    }
    Ok(())
}

/// The suffix to hash with: whatever was asked for, else the one orbistoun ships.
///
/// A user should never have to supply this. Resolving imports is the central act of
/// high-level emulation, so the value is not optional equipment - it is the tool
/// working at all (D071).
fn suffix_for(cli: &Cli) -> Result<Vec<u8>> {
    if cli.suffix_hex.is_empty() {
        return Ok(orbistoun_nid::default_suffix());
    }
    orbistoun_nid::decode_hex(&cli.suffix_hex)
        .context("--suffix-hex must be an even number of hexadecimal digits")
}

/// `names` - search generated names for ones that hash to a module's unnamed imports.
struct NameSearch<'a> {
    /// Module to name the imports of, or a directory of them.
    path: &'a std::path::Path,
    /// Threads to search with; zero means one per core.
    threads: usize,
    /// Grammar to use instead of the built-in vocabulary.
    grammar: Option<&'a std::path::Path>,
    /// Extra candidate names to try verbatim.
    words: Option<&'a std::path::Path>,
    /// Where those names came from, for the provenance record.
    words_from: WordSource,
    /// Where to write the names found.
    out: Option<&'a std::path::Path>,
    /// Where to write the hashes still unnamed.
    wanted: Option<&'a std::path::Path>,
    /// Also harvest candidates out of what a previous run captured from guest memory.
    from_trace: bool,
}

/// Merges the names found into a database, never losing what was already there.
///
/// **Accumulates rather than overwrites.** A database is built from many modules over
/// many runs, and each run only ever sees the imports of the module it was given -
/// so writing the run's own findings would silently discard every name learned from
/// anything else. Names are cheap to keep and expensive to rediscover (D074).
///
/// An existing derivation always wins over a new one for the same name. The record
/// should say when a name was **first** worked out; rewriting it on every sweep would
/// turn a history into a timestamp of the last time somebody ran the tool.
fn write_symbol_db(
    path: &std::path::Path,
    suffix: &[u8],
    found: &[orbistoun_names::solve::Solved],
) -> Result<()> {
    // The suffix actually hashed with, not whatever the user typed. Writing the raw
    // argument recorded an empty string whenever the shipped default was used, and a
    // database with no suffix cannot be loaded at all - so every trace fell back to
    // printing hashes, which looked like the search having failed.
    let suffix_hex = orbistoun_nid::encode_hex(suffix);
    let mut file = match std::fs::read_to_string(path) {
        Ok(text) => orbistoun_nid::SymbolDbFile::from_json(&text)
            .with_context(|| format!("parsing the existing {}", path.display()))?,
        // Absent is the ordinary first run. Any other error is a real problem and must
        // not be mistaken for it, or a permissions fault silently starts from nothing.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => orbistoun_nid::SymbolDbFile {
            suffix_hex: suffix_hex.clone(),
            names: Vec::new(),
            derivations: std::collections::BTreeMap::new(),
        },
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };

    let before = file.names.len();
    let mut known: std::collections::BTreeSet<String> = file.names.iter().cloned().collect();
    for solved in found {
        if known.insert(solved.name.clone()) {
            // Written alongside the name, by the code that found it. This is what makes
            // "we generated these" checkable rather than merely stated (D073).
            file.derivations
                .insert(solved.name.clone(), solved.derivation.clone());
        }
    }
    // Sorted, so a diff shows what was learned rather than how the search was ordered.
    file.names = known.into_iter().collect();

    // Names only in `names`. NIDs are derived, never stored - a file carrying both could
    // hold a pair that does not hash to itself, and that would surface as a mystery
    // unresolved import much later (docs/SYMBOLS.md).
    let text = serde_json::to_string_pretty(&file).context("serialising the symbol database")?;
    std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))?;
    println!(
        "{}: {} names ({} new, {before} already known)",
        path.display(),
        file.names.len(),
        file.names.len() - before
    );
    Ok(())
}

/// Writes the hashes still unnamed, as the work list for the next round.
///
/// Persisted rather than left in a terminal. Without this every run rediscovers the
/// same list and forgets it again, and the list is precisely what the next round of
/// vocabulary work is aimed at.
fn write_wanted(
    service: &Service,
    path: &std::path::Path,
    unnamed: &[orbistoun_nid::Nid],
    found: &[orbistoun_names::solve::Solved],
) -> Result<()> {
    use std::fmt::Write as _;

    // One line per entry rather than a continued literal: a backslash-continued string
    // carries the source indentation into the file, which is how the last version of
    // this header ended up indented in the output.
    const HEADER: &[&str] = &[
        "# Import hashes orbistoun cannot yet name - the work list.",
        "#",
        "# These are system library functions, common to everything built for the",
        "# platform. A hash is derived from a function name and carries nothing of any",
        "# title. Accumulated across every module ever searched; entries disappear as",
        "# the vocabulary grows to explain them.",
        "#",
        "# Generated - never hand-edited. Refresh with: ./bin/orbistoun names",
    ];

    // Accumulated across runs like the database is, and for the same reason: each run
    // sees one module, and the work list is the union of what every module has wanted.
    let carried: std::collections::BTreeSet<u64> = match std::fs::read_to_string(path) {
        Ok(text) => text
            .lines()
            .map(str::trim)
            .filter_map(|l| l.strip_prefix("0x"))
            .filter_map(|l| u64::from_str_radix(l, 16).ok())
            .collect(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => std::collections::BTreeSet::new(),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let wanted = wanted_now(carried, unnamed, found, |nid| service.is_named(nid));

    let mut text = String::new();
    for line in HEADER {
        let _ = writeln!(text, "{line}");
    }
    for nid in &wanted {
        let _ = writeln!(text, "{}", orbistoun_nid::Nid::from_raw(*nid));
    }
    std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))?;
    println!("{}: {} hashes still unnamed", path.display(), wanted.len());
    Ok(())
}

/// What the work list holds after a run, given what can be named.
///
/// Pure, and separated from the file for the reason principle 8 gives: the rule is worth
/// pinning and the writing is not. The bug this shape exposes was invisible while the two
/// were one function, because reproducing it needed a corpus, a database and a search.
///
/// **The rule is "cannot be named now", not "was never solved".** Those read alike and
/// are not the same set. A hash named by any earlier run is excluded from every later
/// search - being named is exactly what excludes it - so it can never be solved again,
/// so a rule keyed on this run's results can never remove it. It stays forever. Measured
/// on the committed file: 116 of 3829 entries were hashes the database could already
/// name, against a header promising they "disappear as the vocabulary grows".
fn wanted_now(
    mut carried: std::collections::BTreeSet<u64>,
    unnamed: &[orbistoun_nid::Nid],
    found: &[orbistoun_names::solve::Solved],
    is_named: impl Fn(orbistoun_nid::Nid) -> bool,
) -> std::collections::BTreeSet<u64> {
    carried.extend(unnamed.iter().map(|nid| nid.as_raw()));
    carried.retain(|raw| !is_named(orbistoun_nid::Nid::from_raw(*raw)));
    // This run's own results too. They are not in that database yet - it is written from
    // `found` after this, and the service answering `is_named` was built before either.
    for solved in found {
        carried.remove(&solved.nid.as_raw());
    }
    carried
}

/// Searches the published word list, then any list the caller supplied.
///
/// Kept apart deliberately. They are different kinds of claim - one is fixed by
/// published standards, the other is whatever a caller handed over - and recording them
/// under one derivation is what made supplied names look as though they had come from
/// this repository's own list (D119).
/// Searches the module's own bytes for the names of its own imports.
///
/// # The strongest source there is, and the cheapest
///
/// Naming is generate-and-test against a one-way hash, so everything turns on whether the
/// true name is in the candidate set. A title's diagnostics, assertions and symbol tables
/// leave literal function names in its data - so for a large class of imports the answer is
/// lying inside the file already being parsed, and no guess is involved at all.
///
/// `sceKernelCreateSema` is why this exists. It blocked two titles, the generator could not
/// spell it - the vocabulary held `Semaphore`, so `sceKernelCreateSemaphore` was generated
/// and tested and the real name was never in the set - and the string was sitting in a
/// *third* title's bytes (D193).
///
/// Run before the generator because it costs one pass over a file that is already in
/// memory, and everything it resolves is removed from a search that is millions of times
/// more expensive.
/// Adds confirmed words to the grammar file, and says how many were new.
///
/// Writes the shipped grammar in place. That file is data and tracked, so a run genuinely
/// changes what the next one can reach - which is the loop compounding rather than
/// restarting (D195).
fn learn_vocabulary(words: &[String]) -> Result<usize> {
    let path = std::path::Path::new("crates/orbistoun-names/data/vendor.toml");
    if !path.exists() {
        // Running from an installed binary rather than a checkout. The names are still
        // confirmed and written; only the grammar cannot grow, and saying nothing is right.
        return Ok(0);
    }
    let before =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    // The injected vocabulary too, which is not in the file and which this would otherwise
    // re-learn word by word (D258).
    let injected = orbistoun_names::posix_vocabulary();
    let after = match orbistoun_names::strings::learn_words(&before, words, &injected) {
        orbistoun_names::strings::Learned::Nothing => return Ok(0),
        orbistoun_names::strings::Learned::Grammar(after) => after,
        // **Printed, not returned as zero.** A refusal that looked like "nothing new" would
        // be the harvest silently doing nothing, which is a worse failure than the one the
        // ceiling exists to prevent - and indistinguishable from a clean run (principle 3).
        orbistoun_names::strings::Learned::Refused(refusal) => {
            println!("  {}", refusal.say());
            return Ok(0);
        }
    };
    // Parsed before it is trusted. A grammar this cannot read is one the next run cannot
    // either, and writing it would break the search rather than widen it.
    let now = orbistoun_names::Grammar::parse(&after)
        .map(|g| g.vocabulary.get("learned").map_or(0, Vec::len))
        .context("the widened grammar no longer parses - not written")?;
    let was = orbistoun_names::Grammar::parse(&before)
        .ok()
        .and_then(|g| g.vocabulary.get("learned").map(Vec::len))
        .unwrap_or(0);
    let added = now.saturating_sub(was);

    // **What it now costs, said where it grows.** A refusal is loud (D330) and an acceptance
    // was silent, so a vocabulary could go from 177 words to thousands with nothing reporting
    // it - which is D320 exactly, at a price nobody is told about. The ceiling stops the
    // unaffordable case; this is what makes the affordable one visible.
    let before_cost = orbistoun_names::strings::round_cost(&before, was);
    let after_cost = orbistoun_names::strings::round_cost(&after, now);
    println!(
        "  learned {was} -> {now} words; a vocabulary round goes {before_cost} -> {after_cost} candidates"
    );
    if before_cost > 0 && after_cost / before_cost >= 2 {
        // A doubling is where somebody should look, not where anything is wrong. Said once,
        // with the multiple, rather than as a threshold nobody can see the position of.
        println!(
            "    that is {}x - worth reading before the next sweep",
            after_cost / before_cost
        );
    }

    std::fs::write(path, after).with_context(|| format!("writing {}", path.display()))?;
    Ok(added)
}

/// Whether a file name is a guest module worth searching.
///
/// **One implementation, deliberately.** This used to be four hand-written globs in
/// `bin/orbistoun` (`titles/*/eboot.bin`, then `.prx` at three fixed depths), and they
/// silently omitted `.sprx` entirely - eleven modules in the local corpus were never once
/// searched, and nothing said so, because a glob that matches nothing is not an error.
/// That is the same failure `is_version_script` was written to fix for symbol maps (D191).
/// Case-insensitive, because the corpus is read off a filesystem that does not care and
/// a module written out as `EBOOT.BIN` is the same module.
fn is_guest_module(name: &str) -> bool {
    if name.eq_ignore_ascii_case("eboot.bin") {
        return true;
    }
    std::path::Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("prx") || e.eq_ignore_ascii_case("sprx"))
}

/// Every guest module beneath a directory, in a stable order.
///
/// Hand-rolled for the same reason as [`find_symbol_maps`]: the whole job is "recurse and
/// match a filename", and a directory-walking dependency would be more code to audit than
/// the code it replaces.
fn collect_modules(root: &std::path::Path, found: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        // An unreadable directory costs its own modules, not the run.
        return;
    };
    // Sorted, so the search order - and therefore which module gets credited for a name
    // two modules both contain - does not depend on the filesystem.
    let mut here: Vec<std::path::PathBuf> = entries.flatten().map(|e| e.path()).collect();
    here.sort();
    for path in here {
        if path.is_dir() {
            collect_modules(&path, found);
        } else if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(is_guest_module)
        {
            found.push(path);
        }
    }
}

/// A module's path as it goes into a provenance record.
///
/// Normalised, because the same module reached two ways - a trailing slash on a directory
/// argument, a Windows separator - would otherwise produce two records that look like two
/// different modules. One of those is already in the database: a tab-completed path left
/// `titles/PPSA21564-app0//eboot.bin` on a hundred names.
fn record_path(path: &std::path::Path) -> String {
    let text = path.display().to_string().replace('\\', "/");
    let mut out = String::with_capacity(text.len());
    let mut last_was_slash = false;
    for c in text.chars() {
        if c == '/' && last_was_slash {
            continue;
        }
        last_was_slash = c == '/';
        out.push(c);
    }
    out
}

/// What one module contributed to a corpus search.
struct ModuleImports {
    /// Where it is, normalised for a provenance record.
    path: String,
    /// The path as given, for reading it a second time.
    ///
    /// **Twice on purpose.** The candidate strings cannot be harvested on the first pass,
    /// because the target set is not known until every module has been read - and holding
    /// fifty-three modules' candidate sets in memory to avoid re-reading them is the
    /// wrong trade for files this size.
    file: std::path::PathBuf,
    /// The hashes it imports and nothing yet names.
    unnamed: std::collections::HashSet<u64>,
}

/// Reads every module's unnamed imports, so one search can answer all of them.
///
/// **This is the change that made a corpus search affordable.** Searching modules one at a
/// time re-ran the entire 2.6-billion-candidate sweep for each, once per module, forty-two
/// times over - while a wider target set costs a search nothing at all, because every
/// candidate is tested by one hash-set lookup regardless of how many hashes are in it.
fn read_corpus(
    service: &Service,
    modules: &[std::path::PathBuf],
) -> Result<(Vec<ModuleImports>, Vec<orbistoun_nid::Nid>)> {
    let mut per_module = Vec::new();
    let mut union: std::collections::BTreeSet<u64> = std::collections::BTreeSet::new();
    for file in modules {
        let bytes = std::fs::read(file).with_context(|| format!("reading {}", file.display()))?;
        // A module that will not parse is skipped rather than fatal. Previous-generation
        // containers are expected in a real corpus and are not an error - but say so, or
        // a silently skipped module looks exactly like one with nothing to contribute.
        let unnamed = match service.unnamed_imports(&bytes) {
            Ok(nids) => nids,
            Err(e) => {
                eprintln!("  skipped {} - {e}", file.display());
                continue;
            }
        };
        union.extend(unnamed.iter().copied().map(orbistoun_nid::Nid::as_raw));
        per_module.push(ModuleImports {
            path: record_path(file),
            file: file.clone(),
            unnamed: unnamed
                .iter()
                .copied()
                .map(orbistoun_nid::Nid::as_raw)
                .collect(),
        });
    }
    let all = union
        .into_iter()
        .map(orbistoun_nid::Nid::from_raw)
        .collect();
    Ok((per_module, all))
}

/// Every name the sweep settled, each saying how it was found.
///
/// **With the source, always.** The sweep tries four of them and printed one sorted list,
/// so a name harvested from a module's own bytes and one the grammar produced were spelled
/// identically - and which it was decides whether the name may be used at all. Pointed at
/// another emulator's binary, module strings are that project's name list arriving through
/// a file, which D242 refuses at the front door. A reader could not tell, and neither could
/// the author (D257).
fn print_names_found(found: &[orbistoun_names::solve::Solved]) {
    for solved in found {
        println!(
            "  {}  {:<44}  {}",
            solved.nid,
            solved.name,
            how_it_was_found(&solved.derivation.method)
        );
    }
    print_names_per_source(found);
}

/// How many names each source settled.
///
/// So "the sweep named 29,403" can never be read as "this repository can now account
/// for 29,403" - which is the difference between a name the grammar earned and one read
/// out of somebody else's binary (D257).
fn print_names_per_source(found: &[orbistoun_names::solve::Solved]) {
    // Counted per source as well as in total, so "the sweep named 29,403" cannot be read
    // as "this repository can now account for 29,403".
    let mut per_source: std::collections::BTreeMap<String, usize> =
        std::collections::BTreeMap::new();
    for solved in found {
        let how = how_it_was_found(&solved.derivation.method);
        let source = how
            .split_whitespace()
            .next()
            .unwrap_or("unknown")
            .to_owned();
        *per_source.entry(source).or_default() += 1;
    }
    for (source, count) in &per_source {
        println!("  {count:>7} named by {source}");
    }
}

/// Reads identifier-shaped strings out of every module, and names what it can with them.
///
/// # Why a corpus finds names a module cannot find in itself
///
/// A name lying in one title's diagnostic text is the vendor's own spelling of a function
/// that every title on the platform imports. Searching each module against only its own
/// bytes threw that away: `sceKernelCreateSema` blocked two titles for weeks while the
/// string sat in a *third* title's data (D193), and the per-module search would have kept
/// missing it however many titles arrived.
///
/// Both cases are the same mechanism and the same proof - a hash collision - so they are
/// both [`orbistoun_nid::Method::Static`]. They are recorded as different sources because
/// they answer different questions: one says the module explains its own import, the other
/// says it took another module's material to explain it.
fn search_corpus_strings(
    hasher: &orbistoun_nid::NidHasher,
    corpus: &[ModuleImports],
    targets: &orbistoun_names::solve::Targets,
) -> Vec<orbistoun_names::solve::Solved> {
    let today = orbistoun_nid::today();
    let mut found: Vec<orbistoun_names::solve::Solved> = Vec::new();
    let mut solved_already: std::collections::HashSet<u64> = std::collections::HashSet::new();
    let mut tried = 0_u64;

    for module in corpus {
        let Ok(bytes) = std::fs::read(&module.file) else {
            continue;
        };
        let candidates = orbistoun_names::strings::candidates(&bytes);
        tried += candidates.len() as u64;
        for name in candidates {
            let nid = hasher.hash(&name);
            if !targets.wants(nid) || !solved_already.insert(nid.as_raw()) {
                continue;
            }
            // Which source it is, decided by the module that *wants* the hash rather than
            // by the one that carried the string. Asking the question the other way round
            // would call every name cross-module the moment two titles shared a string.
            let by = if module.unnamed.contains(&nid.as_raw()) {
                orbistoun_nid::StaticSource::ModuleStrings
            } else {
                orbistoun_nid::StaticSource::CrossModule
            };
            found.push(orbistoun_names::solve::Solved {
                nid,
                name,
                derivation: orbistoun_nid::Derivation::new(
                    orbistoun_nid::Method::Static {
                        by,
                        from: module.path.clone(),
                    },
                    today.as_str(),
                ),
            });
        }
    }

    let cross = found
        .iter()
        .filter(|s| {
            matches!(
                s.derivation.method,
                orbistoun_nid::Method::Static {
                    by: orbistoun_nid::StaticSource::CrossModule,
                    ..
                }
            )
        })
        .count();
    println!(
        "module strings: {tried} candidates across {} modules, {} named ({cross} from another module's bytes)",
        corpus.len(),
        found.len()
    );

    // **What a confirmed name is worth beyond itself.** Its parts are words the generator
    // was missing, and adding them makes every other name built from the same words
    // reachable *by generation* - which is what lets the provenance audit account for them
    // without the title, since a title can never be in this repository (D193).
    let mut words: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for solved in &found {
        words.extend(orbistoun_names::strings::parts_of(&solved.name));
    }
    // **Written, not suggested.** A word a person has to copy across is a step that does
    // not happen unattended, and the whole point is that the next title does not hit the
    // same gap. Data rather than code, so nothing rebuilds (D195).
    if !words.is_empty() {
        let list: Vec<String> = words.into_iter().collect();
        // Never fatal. The names are already confirmed and written; failing to widen the
        // grammar costs the *next* search, not this one.
        match learn_vocabulary(&list) {
            Ok(0) => {}
            Ok(added) => println!("  learned {added} new word(s) into the candidate grammar"),
            Err(e) => println!("  could not widen the grammar: {e}"),
        }
    }
    found
}

/// What reading a module's persisted argument dumps produced.
struct DumpHarvest {
    /// Names confirmed.
    solved: Vec<orbistoun_names::solve::Solved>,
    /// Captures the previous run left behind.
    captures: usize,
    /// Identifier-shaped runs found in them.
    candidates: usize,
    /// Whether a previous run of this module existed at all.
    had_trace: bool,
}

/// Reads identifier-shaped strings out of what a guest handed to its imports as it ran.
///
/// # The first harvester here that needs the guest to actually execute
///
/// Everything else reads a file at rest. This reads the bytes a pointer argument pointed
/// at, captured by the dispatch path while the guest was running (D198) - memory after the
/// loader mapped and relocated it, which can hold text no module contains as a literal: a
/// path assembled at runtime, a name read out of a data file, a string a decompressor
/// produced.
///
/// Cheap, because the capture already happened for a different reason. A run report is an
/// artefact taken for diagnosis; this is a second question asked of it.
fn search_trace_dumps(
    hasher: &orbistoun_nid::NidHasher,
    targets: &orbistoun_names::solve::Targets,
    module: &std::path::Path,
) -> DumpHarvest {
    let Some(trace) = previous_trace(module) else {
        return DumpHarvest {
            solved: Vec::new(),
            captures: 0,
            candidates: 0,
            had_trace: false,
        };
    };
    if trace.dumps.is_empty() {
        return DumpHarvest {
            solved: Vec::new(),
            captures: 0,
            candidates: 0,
            had_trace: true,
        };
    }

    // Both forms, because they can differ. `text` is what the dump renderer thought was
    // readable; `bytes` is everything, and an identifier can sit next to a byte the
    // renderer gave up on. Separated by a zero, so a run cannot straddle two captures and
    // invent an identifier that was never in guest memory.
    let mut material: Vec<u8> = Vec::new();
    for dump in &trace.dumps {
        material.extend_from_slice(dump.text.as_bytes());
        material.push(0);
        material.extend(decode_hex_bytes(&dump.bytes));
        material.push(0);
    }

    let candidates = orbistoun_names::strings::candidates(&material);
    let (solved, stats) = orbistoun_names::solve::solve_names(
        hasher,
        targets,
        &candidates,
        &orbistoun_nid::Derivation::new(
            orbistoun_nid::Method::Runtime {
                by: orbistoun_nid::RuntimeSource::ArgumentDump,
                how: format!(
                    "read out of guest memory during a run of {}, from what it passed to an import",
                    record_path(module)
                ),
            },
            orbistoun_nid::today().as_str(),
        ),
    );
    DumpHarvest {
        solved,
        captures: trace.dumps.len(),
        candidates: usize::try_from(stats.tried).unwrap_or(usize::MAX),
        had_trace: true,
    }
}

/// The runtime harvest across a whole corpus, reported once rather than per module.
///
/// **One line, including when the answer is nothing.** Fifty-three modules and six previous
/// runs would otherwise print forty-seven identical "no previous run" lines, and a report
/// nobody reads is the same as no report. What it must never do is stay silent about having
/// contributed nothing - a source that quietly does no work looks exactly like one that is
/// working (D213).
fn search_corpus_dumps(
    hasher: &orbistoun_nid::NidHasher,
    corpus: &[ModuleImports],
    targets: &orbistoun_names::solve::Targets,
) -> Vec<orbistoun_names::solve::Solved> {
    let mut found = Vec::new();
    let (mut traced, mut captures, mut candidates) = (0_usize, 0_usize, 0_usize);
    for module in corpus {
        let harvest = search_trace_dumps(hasher, targets, &module.file);
        if harvest.had_trace {
            traced += 1;
        }
        captures += harvest.captures;
        candidates += harvest.candidates;
        found.extend(harvest.solved);
    }

    if traced == 0 {
        println!("argument dumps: no previous run of any module in this corpus - nothing to read");
        return found;
    }
    println!(
        "argument dumps: {candidates} candidates from {captures} captures across {traced} previous run(s), {} named",
        found.len()
    );
    if captures > 0 && found.is_empty() {
        // Said out loud, because "0 named" from a source that is working and a source that
        // is misconfigured read identically. Dumps are forced per-import rather than
        // captured broadly, so most of them are scalars with no bytes at all.
        println!(
            "  (dumps are forced per-import - see WORKFLOW.md. Most captured so far are scalars carrying no text)"
        );
    }
    found
}

/// Bytes back out of the hex a dump is stored as.
///
/// Anything that is not a clean pair of hex digits is dropped rather than guessed at. A
/// mis-decoded byte would invent an identifier boundary that was never there, and the
/// candidate it produced would be tested against every wanted hash for nothing.
fn decode_hex_bytes(text: &str) -> Vec<u8> {
    let digits: Vec<u8> = text
        .bytes()
        .filter(u8::is_ascii_hexdigit)
        .collect::<Vec<u8>>();
    digits
        .chunks_exact(2)
        .filter_map(|pair| {
            let hi = char::from(pair[0]).to_digit(16)?;
            let lo = char::from(pair[1]).to_digit(16)?;
            u8::try_from(hi * 16 + lo).ok()
        })
        .collect()
}

fn search_word_lists(
    hasher: &orbistoun_nid::NidHasher,
    targets: &orbistoun_names::solve::Targets,
    words: Option<&std::path::Path>,
    words_from: WordSource,
) -> Result<Vec<orbistoun_names::solve::Solved>> {
    // The published list and any supplied list are searched separately, because they
    // are different kinds of claim and must be recorded as such. Merging them is what
    // made supplied names appear to have come from the repository's own word list.
    let mut found: Vec<orbistoun_names::solve::Solved> = Vec::new();
    let today = orbistoun_nid::today();
    let (solved, stats) = orbistoun_names::solve::solve_names(
        hasher,
        targets,
        orbistoun_names::standard_names(),
        &orbistoun_nid::Derivation::new(
            orbistoun_nid::Method::PublishedStandard {
                list: orbistoun_names::solve::STANDARD_LIST.to_owned(),
            },
            &today,
        ),
    );
    println!(
        "published names: {} tried, {} named",
        stats.tried, stats.found
    );
    if stats.found == 0 {
        // Worth saying out loud, because it is the one cheap check on the suffix
        // itself. These names are fixed by published standards, so if a module links a
        // C library and not one of them matches, the names are not what is wrong.
        eprintln!(
            "{}",
            concat!(
                "no published standard name matched; if this module imports a C library, ",
                "that is a strong sign --suffix-hex is wrong"
            )
        );
    }
    found.extend(solved);

    if let Some(words) = words {
        let text = std::fs::read_to_string(words)
            .with_context(|| format!("reading {}", words.display()))?;
        let where_from = words.display().to_string();
        let derivation = match words_from {
            WordSource::Probe => orbistoun_nid::Method::Runtime {
                by: orbistoun_nid::RuntimeSource::ProbeTranscript,
                how: format!(
                    "names read from {where_from}, reported by this project's own conformance probe"
                ),
            },
            WordSource::Supplied => orbistoun_nid::Method::Supplied { source: where_from },
        };
        let (solved, stats) = orbistoun_names::solve::solve_names(
            hasher,
            targets,
            orbistoun_names::word_list(&text),
            &orbistoun_nid::Derivation::new(derivation, &today),
        );
        println!(
            "supplied names: {} tried, {} named ({:?})",
            stats.tried, stats.found, words_from
        );
        found.extend(solved);
    }

    Ok(found)
}
/// `names` - search for names that hash to what a module, or a whole corpus, imports.
///
/// # Why a directory is one search rather than many
///
/// Given a directory, every module beneath it is read, their unnamed imports are unioned,
/// and **one** search answers all of them. That is not a convenience: searching per module
/// re-ran the full 2.6-billion-candidate sweep once for each, forty-two times over a local
/// corpus, while widening the target set costs a sweep nothing - each candidate is one
/// hash-set lookup whether the set holds one hash or four thousand.
///
/// It also makes the corpus a source rather than a list. A name lying in one title's
/// diagnostic text explains an import of a title that never mentions it, which a
/// module-at-a-time search structurally cannot find (D213).
fn cmd_names(service: &Service, cli: &Cli, search: &NameSearch<'_>) -> Result<()> {
    let NameSearch {
        path,
        threads,
        grammar,
        words,
        words_from,
        out,
        wanted,
        from_trace,
    } = *search;
    let hasher = orbistoun_nid::NidHasher::new(suffix_for(cli)?);

    let mut modules = Vec::new();
    if path.is_dir() {
        collect_modules(path, &mut modules);
        anyhow::ensure!(
            !modules.is_empty(),
            concat!(
                "no guest modules under {} - nothing to search against. The generator ",
                "proposes candidates; confirming one needs a real import table to collide ",
                "with. See docs/PROVENANCE.md"
            ),
            path.display()
        );
        println!("{} modules under {}", modules.len(), path.display());
    } else {
        modules.push(path.to_path_buf());
    }

    let (corpus, unnamed) = read_corpus(service, &modules)?;
    let targets = orbistoun_names::solve::Targets::new(unnamed.iter().copied());
    println!(
        "{} distinct imports have no name yet, across {} readable module(s)",
        targets.len(),
        corpus.len()
    );
    if targets.is_empty() {
        return Ok(());
    }

    let threads = if threads == 0 {
        std::thread::available_parallelism().map_or(1, std::num::NonZero::get)
    } else {
        threads
    };

    let mut found: Vec<orbistoun_names::solve::Solved> = Vec::new();

    // Guest material first: no guess is involved in any of it, and every name it settles
    // is one the far more expensive sweep below no longer has to reach.
    found.extend(search_corpus_strings(&hasher, &corpus, &targets));
    if from_trace {
        found.extend(search_corpus_dumps(&hasher, &corpus, &targets));
    }

    // Published names next. They are not guesses and they cost almost nothing to try.
    found.extend(search_word_lists(&hasher, &targets, words, words_from)?);

    let grammar = match grammar {
        Some(file) => {
            let text = std::fs::read_to_string(file)
                .with_context(|| format!("reading {}", file.display()))?;
            orbistoun_names::Grammar::parse(&text)?
        }
        None => orbistoun_names::Grammar::builtin()?,
    };
    let patterns = grammar.patterns()?;
    let space: u64 = patterns.iter().map(orbistoun_names::Pattern::len).sum();
    println!(
        "generated names: {space} candidates across {} patterns, {threads} threads",
        patterns.len()
    );

    let started = std::time::Instant::now();
    let (solved, stats) =
        orbistoun_names::solve::solve_patterns(&hasher, &targets, &patterns, threads);
    let elapsed = started.elapsed();
    // Integer arithmetic throughout: the count runs into the billions, past where an
    // `f64` holds it exactly, and a throughput figure is only ever read to one or two
    // significant digits anyway.
    let millis = u64::try_from(elapsed.as_millis())
        .unwrap_or(u64::MAX)
        .max(1);
    let rate = stats.tried.saturating_mul(1000) / millis;
    println!(
        "generated names: {} tried in {:.1}s ({rate}/s), {} named",
        stats.tried,
        elapsed.as_secs_f64(),
        stats.found
    );
    found.extend(solved);

    // A hash can be settled by more than one source in a single run - a string in one
    // module and a generated candidate both produce the same name. Keep the first, which
    // is the cheapest to reproduce, because sources run in that order deliberately.
    found.sort_by(|a, b| a.name.cmp(&b.name));
    found.dedup_by(|a, b| a.name == b.name);
    print_names_found(&found);
    println!(
        "{} of {} named ({:.1}%)",
        found.len(),
        targets.len(),
        percent(found.len(), targets.len())
    );

    if let Some(out) = out {
        write_symbol_db(out, &suffix_for(cli)?, &found)?;
    }

    if let Some(path) = wanted {
        write_wanted(service, path, &unnamed, &found)?;
    }
    Ok(())
}
/// The trace a previous run of this module left behind.
///
/// A thin wrapper over `orbistoun_report::trace::load_previous`, kept only to supply the
/// traces directory - the reading and the format knowledge live below the shims, because
/// the GUI compares runs from the same files (D160).
fn previous_trace(module: &std::path::Path) -> Option<orbistoun_report::trace::CallTrace> {
    let paths = orbistoun_paths::Paths::resolve();
    orbistoun_report::trace::load_previous(&paths.traces_dir(), module)
}
/// Says whether this run got further than the last.
///
/// **Presentation only.** The measurement itself is `orbistoun_report::trace::compare`,
/// one layer down, because the GUI has to reach the same verdict from the same two
/// traces. Two shims computing "did this help?" separately is how they come to disagree
/// about the only number this project steers by (D160).
fn report_progress(
    before: Option<&orbistoun_report::trace::CallTrace>,
    after: &orbistoun_report::trace::CallTrace,
) {
    use orbistoun_report::trace::Verdict;

    let progress = orbistoun_report::trace::compare(before, after);
    println!();
    println!("progress");

    if progress.verdict == Verdict::FirstRun {
        println!("  {}", progress.verdict.summary());
        println!(
            "  reached {} distinct imports, died at {}",
            after.distinct, progress.fault
        );
        print_standing(after);
        print_abi(after);
        print_wall(after);
        return;
    }

    println!(
        "  imports  {} distinct ({:+}), {} calls ({:+})",
        after.distinct, progress.distinct_delta, after.total_calls, progress.calls_delta
    );
    print_standing(after);
    println!(
        "  fault {}{}",
        progress.fault,
        match &progress.previous_fault {
            Some(previous) => format!("   (was {previous})"),
            None => String::new(),
        }
    );
    print_reads(after);
    println!(
        "  verdict  {:8} {}",
        progress.verdict.label(),
        progress.verdict.summary()
    );
    for change in &progress.conditions_changed {
        println!("           ! {change}, so this verdict measures a settings change");
    }
    // **Before the verdict is read, not after.** A diagnostic that applied zero times
    // makes the run an ordinary one wearing a label, and the whole risk is that its
    // unchanged result is recorded as an elimination. Two were (D229, D230, D241).
    for what in &after.conditions.did_nothing {
        println!("           !! {what} - this run measured nothing it was asked to measure");
    }
    if progress.bought_under_intervention {
        // **Printed where the conclusion gets drawn.** This exists because the mistake was
        // made here: a reservation moved a wall, the movement read as confirming the
        // hypothesis behind it, and watching what the guest *wrote* one run later said the
        // opposite (D224, D226, D227).
        println!("           ! this run altered the program, so getting further may mean the");
        println!("             guest accepted a wrong answer. Check what it *wrote*, not only");
        println!("             that it moved - ORBISTOUN_WATCH is what answers that");
    }
    print_abi(after);
    print_formats(after);
    print_fault_detail(after);
    print_unattached_dumps(after);
    print_findings(after);
    print_wall(after);
}

/// What the guest was doing when it died, beyond where.
///
/// **The trace held all of this and the report printed a region and an offset.** Reading
/// the operation, the address and the registers meant parsing the trace by hand - which is
/// the tool failing at its one job, and is how a person ends up writing a throwaway script
/// to answer a question the run already knew (D197).
fn print_fault_detail(trace: &orbistoun_report::trace::CallTrace) {
    let Some(f) = &trace.fault else {
        return;
    };
    println!("  faulted  {} {:#x}", f.kind, f.address);
    if let Some(r) = &f.registers {
        for line in r.lines() {
            println!("           {line}");
        }
    }
    if !f.frames.is_empty() {
        let path: Vec<String> = f
            .frames
            .iter()
            .take(4)
            .map(|x| format!("{:#x}", x.return_address))
            .collect();
        println!("           called from {}", path.join(" <- "));
    }
}

/// Arguments captured for functions no finding mentions.
///
/// A forced dump is a question somebody asked deliberately - usually about an
/// implementation they wrote and suspect. Showing it only underneath a finding meant the
/// answer was recorded and never displayed, because an implemented function has no finding
/// (D197).
fn print_unattached_dumps(trace: &orbistoun_report::trace::CallTrace) {
    let findings = orbistoun_report::diagnose::findings(trace);
    let shown: Vec<&str> = findings.iter().map(|f| f.what.as_str()).collect();
    let loose: Vec<&orbistoun_report::trace::ArgumentDump> = trace
        .dumps
        .iter()
        .filter(|d| !shown.iter().any(|w| w.contains(&d.label)))
        .collect();
    if loose.is_empty() {
        return;
    }
    println!();
    println!("captured arguments");
    let mut last = "";
    for d in loose {
        if d.label != last {
            println!("  {}", d.label);
            last = &d.label;
        }
        println!("{}", dump_line(d));
    }
}

/// One captured argument, as a line.
///
/// **One renderer, because there were two.** The findings list and the unattached-dump
/// list each had their own copy of this three-way branch, and the third case below had to
/// be added to both or the same dump would read differently depending on which list it
/// appeared in (D217).
fn dump_line(d: &orbistoun_report::trace::ArgumentDump) -> String {
    if !d.bytes.is_empty() {
        let text = if d.text.is_empty() {
            String::new()
        } else {
            format!("  \"{}\"", d.text)
        };
        return format!(
            "      arg{} = {:#x} -> {} = {}{}",
            d.slot, d.value, d.at, d.bytes, text
        );
    }
    if d.at.is_empty() {
        // A scalar: a size, a flag, a count. Evidence in its own right (D198).
        return format!("      arg{} = {:#x}", d.slot, d.value);
    }
    // Address-shaped, and nothing readable there. Said out loud, because otherwise it
    // renders exactly like the line above and the two mean opposite things - one is a
    // number the guest chose, the other is a pointer that is wrong or a region this run
    // never declared.
    format!("      arg{} = {:#x} -> {}", d.slot, d.value, d.at)
}

/// What the run says is worth doing, most actionable first.
///
/// **The output this project is ultimately for.** Everything above is a measurement; this
/// is the conclusion drawn from it - and it is rendered from the same structured findings
/// that go into the trace, so a person and a machine are reading the same thing rather
/// than one parsing the other's prose (D179).
fn print_findings(trace: &orbistoun_report::trace::CallTrace) {
    use orbistoun_report::diagnose::Confidence;

    let findings = orbistoun_report::diagnose::findings(trace);
    if findings.is_empty() {
        return;
    }
    println!();
    println!("what to do about it");
    for finding in findings.iter().take(6) {
        let mark = match finding.confidence {
            Confidence::Certain => "!",
            Confidence::Likely => "?",
            Confidence::Possible => "~",
        };
        println!("  {mark} {}", finding.what);
        for line in &finding.evidence {
            println!("      {line}");
        }
        // What the guest was pointing at, beneath the finding that names the function.
        // A finding says "implement this"; the dump says what it was handed, which is the
        // difference between knowing the job and being able to do it (D194).
        for dump in trace
            .dumps
            .iter()
            .filter(|d| finding.what.contains(&d.label))
        {
            println!("{}", dump_line(dump));
        }
        if let Some(action) = &finding.action {
            println!("    -> {action}");
        }
    }
    if findings.len() > 6 {
        println!("  ... and {} more", findings.len() - 6);
    }
}

/// Whether formatted writes actually produced anything.
///
/// **"Implemented" and "answered correctly" are different claims.** A refused format still
/// counts as a call reaching an implementation, so the standing figure rises while the
/// guest receives an empty string - which is exactly the sort of improvement-shaped
/// non-improvement this project keeps having to guard against (D183).
fn print_formats(trace: &orbistoun_report::trace::CallTrace) {
    let f = &trace.formats;
    if f.calls == 0 {
        return;
    }
    if f.refused == 0 && f.truncated == 0 {
        println!("  formats  {} writes, all honoured", f.calls);
        return;
    }
    println!(
        "  formats  {} writes, {} refused, {} truncated",
        f.calls, f.refused, f.truncated
    );
    if !f.first_fault.is_empty() {
        println!("           ! {}", f.first_fault);
    }
}

/// How much of the run rested on something real.
///
/// **The discount on the headline.** A call count reads as progress, and it is progress
/// exactly to the extent the calls were answered by an implementation rather than a
/// placeholder. Reporting the total alone lets the two be confused, and the confusion has
/// a direction: the cheapest way to raise a call count is to make every unimplemented
/// function claim success, at which point the guest runs much further on nothing at all.
///
/// Not a prohibition. Answering `ok` everywhere is a legitimate bisection technique and the
/// loop depends on being able to try it - what makes it a hack is doing it unlabelled, so
/// the label is what this prints (D181).
fn print_standing(trace: &orbistoun_report::trace::CallTrace) {
    if trace.total_calls == 0 {
        return;
    }
    let stubbed = trace.stubbed_calls();
    println!(
        "  standing {} of {} calls answered by an implementation ({}% on stubs)",
        trace.total_calls - stubbed,
        trace.total_calls,
        trace.stubbed_share()
    );
    if trace.conditions.answers_blindly() {
        println!(
            concat!(
                "           ! stubs are answering {} rather than reporting unimplemented, ",
                "so reaching further this run means less, not more"
            ),
            trace.conditions.default_return
        );
    }
}

/// Whether the guest received every byte it asked for.
///
/// Printed whenever anything was read, including when it is clean - a line that only
/// appears on failure cannot be told apart from one nobody wired up (D175).
fn print_reads(trace: &orbistoun_report::trace::CallTrace) {
    let reads = &trace.reads;
    if reads.reads == 0 {
        return;
    }
    if reads.short == 0 {
        println!(
            "  files {} reads, {} KiB, none cut short",
            reads.reads,
            reads.bytes / 1024
        );
    } else {
        println!(
            "  files {} of {} reads were CUT SHORT ({} KiB delivered)",
            reads.short,
            reads.reads,
            reads.bytes / 1024
        );
    }
}

/// Whether the guest called us the way the calling convention says it must.
///
/// Printed on every run, including when it is clean - a line that only appears on failure
/// cannot be distinguished from a line nobody wired up, and this measures something that
/// was silently untested for the whole life of the project (D159).
fn print_abi(trace: &orbistoun_report::trace::CallTrace) {
    let abi = &trace.abi;
    if abi.misaligned_calls == 0 {
        println!(
            "  abi {} calls, all on a conforming stack",
            trace.total_calls
        );
        return;
    }
    println!(
        "  abi {} of {} calls arrived on a MISALIGNED stack",
        abi.misaligned_calls, trace.total_calls
    );
    if let (Some(sequence), Some(rsp)) = (abi.first_misaligned_sequence, abi.first_misaligned_rsp) {
        let import = abi.first_misaligned_import.as_deref().unwrap_or("unknown");
        // The remainder is the diagnosis: 0 means control arrived by a jump where a call
        // was expected, anything odd means the stack was already wrong upstream.
        println!(
            "           first at #{sequence} {import}, rsp {rsp:#x} (rsp % 16 = {})",
            rsp % 16
        );
    }
}

/// The last calls before the guest died, in order.
///
/// Only shown when there *was* a fault: for a run that ended cleanly the ranked list is
/// the right view, and this would be noise. At a wall it is the opposite - "what did it
/// call last" is the only question worth asking, and a list ranked by frequency cannot
/// answer it at any length (D154).
///
/// Consecutive repeats are collapsed. A guest clearing memory calls `memset` three
/// hundred times in a row, and printing them individually buries the two calls either
/// side that actually matter.
fn print_wall(trace: &orbistoun_report::trace::CallTrace) {
    if trace.fault.is_none() || trace.tail.is_empty() {
        return;
    }
    println!();
    println!("last calls before the fault");

    // Keyed on the answer as well as the label, so a run of calls that returned *different*
    // values does not collapse into one line that hides the very thing worth seeing - a
    // function that answered a pointer once and zero the next time (D459).
    let mut runs: Vec<(&str, u64, Option<u64>, u64, u64)> = Vec::new();
    for call in &trace.tail {
        match runs.last_mut() {
            Some((label, _, ret, _, count)) if *label == call.label && *ret == call.returned => {
                *count += 1;
            }
            _ => runs.push((&call.label, call.arg0, call.returned, call.from, 1)),
        }
    }
    for (label, arg0, returned, from, count) in runs {
        let repeat = if count > 1 {
            format!(" x{count}")
        } else {
            String::new()
        };
        // The call site is shown beside the call because it is the same address space the
        // fault's frame walk reports - so a frame in the stack trace stops being a bare
        // number the moment an import was called from it (D173).
        let site = if from == 0 {
            String::new()
        } else {
            format!("   from {from:#x}")
        };
        // What it answered, when the call had returned before the trace was read. Shown
        // because at these walls the wrong value is usually the one we handed back, not the
        // one the guest passed in - and a tail without it could not point at that (D459).
        let answered = match returned {
            Some(ret) => format!(" -> {ret:#x}"),
            None => String::new(),
        };
        // The first argument is shown because at a wall it is often the whole answer: a
        // guest passing back an address it was handed a moment earlier makes the chain
        // visible with no other tooling.
        println!("  {label}({arg0:#x}){answered}{repeat}{site}");
    }
}

/// Where a library's knowledge file lives.
fn knowledge_path(library: &str) -> std::path::PathBuf {
    std::path::Path::new("crates/orbistoun-hle/data/knowledge").join(format!("{library}.toml"))
}

/// `learn` - record something established about a guest function.
///
/// Merges rather than replaces. A session that learns one edge case should not have to
/// restate everything already known, and it must not silently drop it either.
fn cmd_learn(learned: &Learned) -> Result<()> {
    use orbistoun_hle::knowledge::{KnowledgeFile, Record};

    let path = knowledge_path(&learned.library);
    let mut file = match std::fs::read_to_string(&path) {
        Ok(text) => KnowledgeFile::parse(&text)
            .with_context(|| format!("parsing the existing {}", path.display()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => KnowledgeFile {
            library: learned.library.clone(),
            functions: Vec::new(),
        },
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };

    // **The merge rule is not here.** It belongs to the crate that owns the format, so the
    // loop can record what it measured through the same rule rather than a second copy of it
    // (D291, D292). What a shim keeps is where the file is, and what to say afterwards.
    let record = Record {
        function: learned.function.clone(),
        arity: learned.arity,
        purpose: learned.purpose.clone(),
        edge_cases: learned.edges.clone(),
        found_in: learned.seen_in.clone(),
        known_by: learned.known.map(Into::into),
        cites: learned.cites.clone(),
        assumptions: learned.assumptions.clone(),
        note: learned.note.clone(),
    };
    let faults = file.merge(&record, &orbistoun_nid::today());

    // **Refused rather than defaulted.** A default would pick a provenance on the writer's
    // behalf, and every available default is a lie: `assumed` understates work that was
    // really done, and anything stronger overstates it. Refusing costs one retry and is
    // the only option that cannot record something untrue (D180).
    if !faults.is_empty() {
        anyhow::bail!(
            concat!(
                "{}

Record how it is known: ",
                "--known published|measured|guest-observed|assumed
",
                "(published and measured also need --cites)"
            ),
            faults.join(
                "
"
            )
        );
    }

    if file.library.is_empty() {
        learned.library.clone_into(&mut file.library);
    }
    let text = file.render().context("rendering the knowledge file")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
    println!(
        "{}: recorded {} ({} known, {} understood)",
        path.display(),
        learned.function,
        file.functions.len(),
        file.functions.iter().filter(|f| !f.is_bare()).count()
    );
    Ok(())
}

/// `turn` - take every mechanical step one run's findings call for.
///
/// **The shim holds no logic.** Everything decided here is decided by `orbistoun-turn`; this
/// resolves where things are, spawns itself as the guest runner, and prints (principle 13).
fn cmd_turn(
    path: &std::path::Path,
    record: bool,
    apply: bool,
    verify: Option<&std::path::Path>,
) -> Result<()> {
    use orbistoun_turn::{experiment::Finding, trial::GuestTrial, turn};

    // **A verifying turn runs where nothing has been learned.** An applied measurement removes
    // the wall it was measured at, so re-deriving it from this machine's own state finds
    // nothing - and applying one would make it permanently unverifiable (D298).
    let scratch = verify
        .is_some()
        .then(tempfile::tempdir)
        .transpose()
        .context("making a directory for a verifying run")?;
    let paths = scratch
        .as_ref()
        .map_or_else(orbistoun_paths::Paths::resolve, |dir| {
            orbistoun_paths::Paths::resolve_with(
                &orbistoun_paths::EnvSnapshot {
                    portable_flag: false,
                    data_dir: Some(dir.path().to_path_buf()),
                },
                None,
                None,
            )
        });
    let traces = paths.traces_dir();
    std::fs::create_dir_all(&traces).with_context(|| format!("creating {}", traces.display()))?;

    // Spawns *this* binary as the guest runner, which is what worker mode already does:
    // the runner is then literally the same build and cannot be a stale copy.
    let binary = std::env::current_exe().context("finding this executable")?;
    let mut trial = GuestTrial::new(&binary, path, &traces);
    if let Some(dir) = &scratch {
        // The child reads its own learned file too, so the isolation has to reach it.
        trial = trial.with_env(orbistoun_env::DATA_DIR.name, dir.path().to_string_lossy());
    }

    let baseline = trial
        .spawn(&[])
        .map_err(|e| anyhow::anyhow!("the first run could not be made: {e}"))?;
    let trace = orbistoun_report::trace::load_previous(&traces, path)
        .context("the run wrote no trace to read back")?;
    let found = orbistoun_report::diagnose::findings(&trace);

    let plan = turn::plan(&found, baseline.fault);
    println!(
        "{} finding(s), {} step(s), {} of them mechanical",
        found.len(),
        plan.len(),
        plan.iter().filter(|s| s.is_automatic()).count()
    );

    let taken = turn::turn(&mut trial, &plan)
        .map_err(|e| anyhow::anyhow!("a step could not be run: {e}"))?;

    for result in &taken {
        println!("  {}", result.say());
    }

    // **Always, and this used to need a flag.** A turn that measured a contract and was given
    // no flag printed it and wrote nothing - so the measurement existed only in a terminal,
    // which `CLAUDE.md` names as already lost. Three titles were diagnosed this way and two of
    // the results went nowhere.
    //
    // Writing a proposal is inert: a file nothing applies, undone by deleting it. **Applying**
    // changes what the next run does, which is why that still needs `--apply` and an oracle
    // behind it. Emitting and applying are different acts and only one of them needed gating
    // (D355).
    // Asked after the findings, because a question is what a turn does when the run itself
    // has stopped producing mechanical steps - and it costs boots, so it goes last (D356).
    attempt_questions(&mut trial, &baseline)?;

    let proposed = write_proposals(path, &plan, &taken)?;
    if proposed > 0 {
        println!(
            "  {proposed} proposal(s) written to {}/ - nothing applied",
            orbistoun_submit::PATCHES_DIR
        );
    }

    if apply {
        apply_patches(&paths, path, &plan, &taken)?;
    }
    if let Some(submitted) = verify {
        verify_against(&paths, path, submitted, &plan, &taken)?;
    }
    if record {
        for (step, result) in plan.iter().zip(taken.iter()) {
            let (turn::Step::SweepArguments { target }, turn::Taken::Swept(finding)) =
                (step, result)
            else {
                continue;
            };
            let satisfied = taken
                .iter()
                .any(|t| matches!(t, turn::Taken::Confirmed { reached, was, .. } if reached > was));
            if let Some((library, learned)) = turn::promote(target, finding, satisfied) {
                print_learn_command(library.as_deref(), &learned);
            } else if !matches!(finding, Finding::OutParameter { .. }) {
                println!("  nothing to record: the sweep established no contract");
            }
        }
    }
    Ok(())
}

/// Writes what a turn measured into the learned policy.
///
/// **Underneath what a person wrote, never over it.** The file is folded in by the worker with
/// `StubPolicy::absorb`, which keeps every deliberate entry - so the worst a wrong guess costs
/// is a run, and deleting the file is a complete undo (D296).
///
/// Refuses to write a patch whose evidence has not been earned. A patch that touches guest
/// memory needs a conformance check covering it; a moved wall is not enough, because a wrong
/// write is invisible until something unrelated breaks (principle 3).
fn apply_patches(
    paths: &orbistoun_paths::Paths,
    title: &std::path::Path,
    plan: &[orbistoun_turn::turn::Step],
    taken: &[orbistoun_turn::turn::Taken],
) -> Result<()> {
    let mut learned = orbistoun_hle::learned::Learned::load(&paths.learned_file())
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let mut written = 0_usize;

    // **What the probe said before anything was applied.** A measurement that declares it needs
    // a conformance check is graded against this; one that does not is kept on reach alone
    // (D296, D302). Absent probe, absent gate - and said out loud rather than quietly downgraded.
    let binary = std::env::current_exe().context("finding this executable")?;
    let probe = probe_module();
    let before = if let Some(path) = &probe {
        Some(probe_score(&binary, path, paths.data_root())?)
    } else {
        println!("  no conformance probe at {PROBE_MODULE}; grading on the corpus alone");
        None
    };

    // **And every guest this machine has.** The probe grades what somebody wrote a check
    // for; the corpus grades what a change does to guests nobody wrote anything for, which
    // is the case title mining is actually about (D303).
    let titles = corpus_titles();
    let corpus_before = corpus_score(&binary, &titles, paths.data_root());
    println!(
        "  grading against {} guest(s) on this machine",
        titles.len()
    );

    for measurement in measurements(title, plan, taken) {
        // **Said, not assumed.** The evidence a measurement needs is a property of what it
        // claims, and announcing it is what stops "the wall moved" being read as "the
        // behaviour is right" (D296).
        println!(
            "
  measured {}: needs {:?}",
            measurement.function, measurement.evidence
        );
        for assumption in &measurement.assumes {
            println!("    assumes: {assumption}");
        }
        // Applied to a scratch copy so a refused patch never touches the real file.
        let mut trial = learned.clone();
        trial.record(measurement.clone());
        let scratch = tempfile::tempdir().context("a directory to grade the patch in")?;
        std::fs::write(
            scratch.path().join(orbistoun_paths::LEARNED_FILE),
            trial.to_toml().map_err(|e| anyhow::anyhow!("{e}"))?,
        )
        .context("writing the patch to grade")?;

        // **Every guest votes, and one regression refuses the change.** Nothing here can weigh
        // one guest's correctness against another's, so a patch that helps three and breaks
        // one is a trade nobody has the exchange rate for (D303).
        let corpus_after = corpus_score(&binary, &titles, scratch.path());
        let on_the_corpus = corpus_before.against(&corpus_after);
        println!("    corpus: {}", on_the_corpus.say());
        if !on_the_corpus.broken.is_empty() {
            println!("    not kept");
            continue;
        }

        // A patch that hands the guest memory cannot be judged on reach alone: a wrong write is
        // invisible until something unrelated breaks, which is principle 3's opening sentence.
        let needs_a_spec =
            measurement.evidence == orbistoun_hle::learned::Evidence::ConformanceCheck;
        let kept = match (needs_a_spec, before.as_ref(), probe.as_ref()) {
            (false, ..) => true,
            (true, Some(graded), Some(path)) => {
                let verdict = graded.against(&probe_score(&binary, path, scratch.path())?);
                println!("    probe: {}", verdict.say());
                verdict.is_an_improvement()
            }
            // **Falls back to the corpus rather than refusing outright.** Refusing every
            // memory-handing patch on a machine without a probe would leave the common case -
            // somebody mining a title - unable to keep anything at all (D303).
            (true, ..) => {
                println!("    no probe; kept on the corpus alone, which cannot say *correct*");
                !on_the_corpus.fixed.is_empty()
            }
        };
        if !kept {
            println!("    not kept");
            continue;
        }

        learned.record(measurement);
        written += 1;
    }

    if written == 0 {
        println!("  nothing measured that a policy could carry");
        return Ok(());
    }
    let path = paths.learned_file();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let text = learned.to_toml().map_err(|e| anyhow::anyhow!("{e}"))?;
    std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
    println!("  wrote {written} measurement(s) to {}", path.display());
    Ok(())
}

/// Every measurement a turn established, in the shape the learned file keeps.
///
/// **Everything the run knew and used to throw away.** Which guest demonstrated it, when,
/// which build, and what the claim rests on that nothing measured - all of it was printed to a
/// terminal and lost before the file carried it, and all of it is what makes an entry
/// checkable by somebody else (D297).
fn measurements(
    title: &std::path::Path,
    plan: &[orbistoun_turn::turn::Step],
    taken: &[orbistoun_turn::turn::Taken],
) -> Vec<orbistoun_hle::learned::Measurement> {
    use orbistoun_hle::learned::{Evidence, Measurement};
    use orbistoun_turn::{patch, turn};

    let mut out = Vec::new();
    for (step, result) in plan.iter().zip(taken.iter()) {
        // Two shapes produce a measurement: a swept out-parameter contract, and the function
        // whose placeholder the guest was found to be dereferencing. The second is the only
        // one keepable on a moved wall, because it writes nothing (D296, D299).
        // The qualified name travels beside the patch, because the library half is the part
        // `from_finding` strips and the part a promotion needs (D328).
        let proposed = match (step, result) {
            (turn::Step::SweepArguments { target }, turn::Taken::Swept(finding)) => {
                patch::from_finding(target, finding).map(|patch| (target.as_str(), patch))
            }
            // **Whichever of the two answers reached further.** Both were run; recording the
            // rule's one regardless would make the comparison decorative (D300).
            (
                _,
                turn::Taken::Sourced {
                    function, answer, ..
                },
            ) => Some((
                function.as_str(),
                match answer {
                    turn::Answer::Zero => patch::from_placeholder_source(function),
                    turn::Answer::Region { .. } => {
                        patch::from_placeholder_source_as_region(function)
                    }
                },
            )),
            _ => None,
        };
        let Some((qualified, proposed)) = proposed else {
            continue;
        };
        out.push(Measurement {
            function: proposed.function,
            library: qualified
                .split_once("::")
                .map_or_else(String::new, |(library, _)| library.to_owned()),
            // The containing directory, not the path: a path is a fact about one machine, and
            // an entry meant to travel should carry nothing about the sender's disk.
            measured: title_id(title).unwrap_or_else(|| "unknown".to_owned()),
            on: orbistoun_nid::today(),
            by: build_stamp(),
            known: orbistoun_hle::knowledge::Oracle::GuestObserved,
            evidence: match proposed.evidence {
                patch::Evidence::Further => Evidence::Further,
                patch::Evidence::ConformanceCheck => Evidence::ConformanceCheck,
            },
            answers: proposed.answers,
            region: proposed.region,
            assumes: proposed.assumptions,
        });
    }
    out
}

/// Which build established a measurement.
fn build_stamp() -> String {
    format!(
        "orbistoun {} ({})",
        env!("CARGO_PKG_VERSION"),
        option_env!("ORBISTOUN_COMMIT").unwrap_or("unknown")
    )
}

/// How every guest on this machine fared, for grading a change against a corpus.
///
/// **The oracle when nobody wrote a check.** A probe grades what somebody thought to test, and
/// title mining is the point - a person runs a commercial title, it dies on a stub, and there
/// is no check for it and nobody to write one. Every other title they own is an independent
/// guest with its own expectations of the same function, so the corpus is the suite (D303).
fn corpus_score(
    binary: &std::path::Path,
    titles: &[std::path::PathBuf],
    data_dir: &std::path::Path,
) -> orbistoun_turn::conformance::Corpus {
    use orbistoun_turn::conformance::{Corpus, Reach};

    let mut corpus = Corpus::default();
    // **Where the child writes its traces, not somewhere of our choosing.** The worker resolves
    // its own trace directory from `ORBISTOUN_DATA_DIR`; handing the trial a different one made
    // every run report "no trace was written", which the loop below skipped in silence - so a
    // sweep of seven guests graded none of them and reported that nothing had changed.
    let traces = orbistoun_turn::trial::traces_in(data_dir);
    std::fs::create_dir_all(&traces).ok();
    for title in titles {
        let mut trial = orbistoun_turn::trial::GuestTrial::new(binary, title, &traces)
            .with_env(orbistoun_env::DATA_DIR.name, data_dir.to_string_lossy());
        let outcome = match orbistoun_turn::experiment::Trial::spawn_axes(&mut trial, &[]) {
            Ok(outcome) => outcome,
            Err(e) => {
                // **Said, not skipped.** A guest that could not be run says nothing about the
                // change - and a sweep that quietly graded fewer guests than it claimed is how
                // "no regression" comes to mean "nobody looked" (principle 3).
                println!("    {} could not be run: {e}", title.display());
                continue;
            }
        };
        corpus.saw(
            &title_id(title).unwrap_or_else(|| "unknown".to_owned()),
            Reach {
                reached: outcome.reached,
                touched: outcome.touched,
                faulted: outcome.fault.is_some(),
            },
        );
    }
    corpus
}

/// Every guest this machine can run, the probe included.
fn corpus_titles() -> Vec<std::path::PathBuf> {
    let paths = orbistoun_paths::Paths::resolve();
    let library = orbistoun_service::FileConfig::load(&paths.config_file())
        .map(|config| config.library.root.clone())
        .unwrap_or_default();
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&library) {
        for entry in entries.flatten() {
            let module = entry.path().join("eboot.bin");
            if module.exists() {
                out.push(module);
            }
        }
    }
    out.sort();
    out
}

/// Where the conformance probe lives, if it has been put there.
///
/// **A title like any other**, so the gate needs no special path handling and a machine without
/// it is not broken - it simply cannot grade anything that asks to be graded, and says so.
const PROBE_MODULE: &str = "titles/obscene/eboot.bin";

/// The probe, if this machine has one.
fn probe_module() -> Option<std::path::PathBuf> {
    let path = std::path::PathBuf::from(PROBE_MODULE);
    path.exists().then_some(path)
}

/// Runs the conformance probe and reads what it graded.
///
/// **The oracle the fix loop was missing.** `FURTHER` says the guest got past something and
/// says nothing once it stops faulting; the probe grades checks against a spec, by name. With
/// one in hand a generator is free to be dumb, which is the arrangement the naming loop has
/// always had and the fix loop never did (D302).
fn probe_score(
    binary: &std::path::Path,
    probe: &std::path::Path,
    data_dir: &std::path::Path,
) -> Result<orbistoun_turn::conformance::Score> {
    let output = std::process::Command::new(binary)
        .arg("run")
        .arg(probe)
        .arg("--calls")
        .arg(PROBE_CALL_BUDGET.to_string())
        .env(orbistoun_env::DATA_DIR.name, data_dir)
        .output()
        .with_context(|| format!("running the probe at {}", probe.display()))?;
    // The probe writes its records to the error stream, which the worker inherits.
    let transcript = String::from_utf8_lossy(&output.stderr);
    Ok(orbistoun_turn::conformance::Score::read(&transcript))
}

/// How far the probe is allowed to run when it is being used as a gate.
///
/// Generous: a budget that stops it early removes checks from the report, and a shorter report
/// reads as "nothing regressed" when it means "we stopped looking" - which the verdict counts
/// as a regression precisely so this cannot pass silently.
const PROBE_CALL_BUDGET: u64 = 2_000_000;

/// `turn --verify` - re-derive a submitted file locally and report where it disagrees.
///
/// **This is what makes receiving one safe.** A measurement is checked by measuring again, not
/// by trusting it, which is why a policy entry is a better contribution than a diff: the claim
/// is falsifiable by a command (D297).
///
/// "Not measured here" is reported and is **not** a refutation - it usually means the title is
/// absent or the run never reached the call, and reporting it as a contradiction would turn
/// "we did not look" into "it is wrong".
fn verify_against(
    paths: &orbistoun_paths::Paths,
    title: &std::path::Path,
    submitted: &std::path::Path,
    plan: &[orbistoun_turn::turn::Step],
    taken: &[orbistoun_turn::turn::Taken],
) -> Result<()> {
    use orbistoun_hle::learned::{Disagreement, Learned};

    let _ = paths;
    let theirs = Learned::load(submitted).map_err(|e| anyhow::anyhow!("{e}"))?;
    let mut ours = Learned::default();
    for measurement in measurements(title, plan, taken) {
        ours.record(measurement);
    }

    let disagreements = ours.disagreements(&theirs);
    println!(
        "
  verified {} submitted measurement(s) against {} of our own",
        theirs.measurements.len(),
        ours.measurements.len()
    );
    if disagreements.is_empty() {
        println!("  every one agrees with what this machine measured");
        return Ok(());
    }
    for disagreement in &disagreements {
        match disagreement {
            Disagreement::NotMeasuredHere { function } => println!(
                "  ?  {function}: not measured here - the title may be absent, or the run never reached it"
            ),
            Disagreement::Differs {
                function,
                here,
                there,
            } => println!("  !  {function}: here {here}, submitted {there}"),
        }
    }
    Ok(())
}

/// Prints what a turn earned as the command that would record it.
///
/// **Printed, not run.** A sweep's conclusion is admissible; changing a tracked file stays a
/// deliberate act somebody reviews as a diff (D291).
fn print_learn_command(library: Option<&str>, learned: &orbistoun_hle::knowledge::Record) {
    use std::fmt::Write as _;

    /// Where a library was not carried by the label, since `learn` insists on one.
    const UNATTRIBUTED: &str = "libkernel";

    // A quoted argument, in a form a shell will hand over whole. Inner double quotes become
    // single ones rather than being escaped: these strings are English sentences this code
    // wrote, so there is nothing to preserve and a quoting scheme that survives copy-paste
    // is worth more than fidelity to a character nothing puts there.
    let quote = |s: &str| format!("\"{}\"", s.replace('"', "'"));

    // One line per argument, continued with a backslash a shell reads - **not** a Rust string
    // continuation, which `cargo fmt` collapses while baking the source indentation into the
    // rendered text. That is D184, and the first draft of this function tripped the guard
    // written for it.
    let mut out = format!(
        "\n  orbistoun-cli learn {} --library {} --known guest-observed",
        learned.function,
        library.unwrap_or(UNATTRIBUTED)
    );
    for edge in &learned.edge_cases {
        let _ = write!(out, " \\\n    --edge {}", quote(edge));
    }
    for assumption in &learned.assumptions {
        let _ = write!(out, " \\\n    --assumes {}", quote(assumption));
    }
    println!("{out}");
}

/// `knows` - print what is known about guest functions.
fn cmd_knows(pattern: Option<&str>) {
    /// Written explicitly so the replacement below is not a bare escape in a call.
    const NEWLINE: &str = "
";
    /// What a wrapped purpose line is prefixed with, to line up under the first.
    const PURPOSE_CONTINUATION: &str = "
            ";

    let knowledge = orbistoun_hle::knowledge::Knowledge::builtin();

    let Some(pattern) = pattern else {
        println!(
            "{} functions recorded, {} understood beyond a name",
            knowledge.len(),
            knowledge.understood()
        );
        print_provenance_summary(&knowledge);
        for f in knowledge.functions() {
            let mark = if f.is_bare() { " " } else { "*" };
            println!(
                "  {mark} {:<40} {}",
                f.name,
                knowledge.library_of(&f.name).unwrap_or("")
            );
        }
        println!("{NEWLINE}(* means something beyond the name is recorded)");
        return;
    };

    for f in knowledge.functions().filter(|f| f.name.contains(pattern)) {
        println!(
            "{NEWLINE}{}  [{}]",
            f.name,
            knowledge.library_of(&f.name).unwrap_or("?")
        );
        if let Some(arity) = f.arity {
            println!("  arity {arity}");
        }
        if !f.purpose.is_empty() {
            // Indented so a multi-line purpose reads as one block rather than
            // colliding with the labels beneath it.
            let indented = f.purpose.trim().replace(NEWLINE, PURPOSE_CONTINUATION);
            println!("  purpose {indented}");
        }
        for (i, a) in f.arguments.iter().enumerate() {
            println!("  arg {i} {:<12} {:<8} {}", a.name, a.kind, a.note);
        }
        for edge in &f.edge_cases {
            println!("  edge {edge}");
        }
        if !f.found_by.is_empty() || !f.found_in.is_empty() {
            println!(
                "  found {} {}{}",
                f.found_by,
                if f.found_in.is_empty() {
                    String::new()
                } else {
                    format!("in {} ", f.found_in.join(", "))
                },
                f.found_on
            );
        }
        if let Some(known) = f.known_by {
            println!(
                "  known {:<14}{}",
                known.label(),
                if f.cites.is_empty() { "" } else { &f.cites }
            );
        }
        for assumption in &f.assumptions {
            println!("  assumes {assumption}");
        }
        if !f.note.is_empty() {
            println!("  note {}", f.note);
        }
    }
}

/// How the knowledge base knows what it claims, and how much of it is guessing.
///
/// **Printed unprompted, because a provenance field nobody looks at is a provenance field
/// nobody maintains.** Two hundred entries all resting on an assumption and two hundred
/// measured against hardware are the same count and completely different projects; only
/// this breakdown tells them apart.
///
/// The open-question total is expected to *rise* as more is written down - an assumption
/// only appears once somebody notices it - and to fall as hardware answers them. A number
/// that only ever falls is measuring candour rather than knowledge.
fn print_provenance_summary(knowledge: &orbistoun_hle::knowledge::Knowledge) {
    use orbistoun_hle::knowledge::Oracle;

    let counts = [
        Oracle::Published,
        Oracle::Measured,
        Oracle::GuestObserved,
        Oracle::Assumed,
    ]
    .map(|o| (o, knowledge.resting_on(o)));

    let shown: Vec<String> = counts
        .iter()
        .filter(|(_, n)| *n > 0)
        .map(|(o, n)| format!("{n} {}", o.label()))
        .collect();
    if !shown.is_empty() {
        println!("  resting on {}", shown.join(", "));
    }

    let open = knowledge.open_questions();
    if open > 0 {
        println!("  {open} open questions a probe on real hardware could settle");
    }

    // Never silent on a fault. A knowledge base that quietly contains unaccounted claims
    // is worse than one with none, because it reads as though it had been checked.
    let faults = knowledge.provenance_faults();
    if !faults.is_empty() {
        println!(
            "  {} entries do not account for what they claim:",
            faults.len()
        );
        for fault in faults.iter().take(10) {
            println!("    ! {fault}");
        }
    }
}

/// The title a module path belongs to.
///
/// The containing directory, which is the same identifier the knowledge files already use
/// in `found_in` - and deliberately **not** the file name: a bare `eboot.bin` is identical
/// in every title and would have them all sharing one record.
fn title_id(path: &std::path::Path) -> Option<String> {
    path.parent()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
}

/// Where one title's record lives.
fn compat_path(dir: &std::path::Path, title: &str) -> std::path::PathBuf {
    dir.join(format!("{title}.toml"))
}

/// Reads a title record, or an empty one.
fn load_compat(dir: &std::path::Path, title: &str) -> Result<orbistoun_overrides::OverrideFile> {
    let path = compat_path(dir, title);
    match std::fs::read_to_string(&path) {
        Ok(text) => orbistoun_overrides::OverrideFile::from_toml(&text)
            .with_context(|| format!("parsing {}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Ok(orbistoun_overrides::OverrideFile::default())
        }
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

/// Every title result this machine holds, both slots.
///
/// Reads the same directory `compat list` does, because a submission has to carry what the
/// tree carries - two readers of one record is how they come to disagree (D160).
fn gathered_results(dir: &std::path::Path) -> Result<orbistoun_submit::Results> {
    let mut results = orbistoun_submit::Results::default();
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        // Not an error. A machine that has recorded no title still has measurements worth
        // sending, and refusing here would make the common binary-only case unable to
        // contribute anything at all.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(results),
        Err(e) => return Err(e).with_context(|| format!("reading {}", dir.display())),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "toml") {
            continue;
        }
        let Some(title) = path.file_stem().map(|n| n.to_string_lossy().into_owned()) else {
            continue;
        };
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let file = orbistoun_overrides::OverrideFile::from_toml(&text)
            .with_context(|| format!("parsing {}", path.display()))?;
        if let Some(status) = file.status {
            results.status.insert(title.clone(), status);
        }
        if let Some(experiment) = file.experiment {
            results.experiment.insert(title, experiment);
        }
    }
    Ok(results)
}

/// What this machine has to contribute, as a bundle.
fn gathered_bundle(
    paths: &orbistoun_paths::Paths,
    compat_dir: &std::path::Path,
) -> Result<orbistoun_submit::Bundle> {
    let learned = orbistoun_hle::learned::Learned::load(&paths.learned_file())
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    // **Generated first, then whatever a person wrote.** A hand-written patch is the more
    // considered of the two and must not be displaced by one this produced, so it is added
    // second and wins on name.
    let into = std::path::Path::new(orbistoun_submit::PATCHES_DIR);
    let mut proposals = generated_proposals(&learned, into)?;
    for written in local_proposals()? {
        proposals.retain(|generated| generated.file != written.file);
        proposals.push(written);
    }

    let bundle = orbistoun_submit::Bundle::gather(
        learned,
        gathered_results(compat_dir)?,
        build_stamp(),
        orbistoun_nid::today(),
    );
    Ok(bundle.proposing(proposals))
}

/// Attempts the highest-ranked open question that names its own experiment.
///
/// # Why a turn asks a question at all
///
/// The dispatcher is driven by run reports - what crashed, this time. That leaves the other
/// half of what this project knows unread: 277 open questions, ranked by how often a guest
/// calls the function, each written by somebody who had just failed to answer it. The first
/// of them blocks 67.5% of every call the corpus makes, names its own experiment, and the
/// apparatus for that experiment has existed unwired since D218.
///
/// So a turn now ends by asking one. Not all of them - a question costs boots, and the
/// ranking exists precisely because they are not equally worth asking (D356).
///
/// **Reported rather than concluded.** What comes back is what each run did; deciding that a
/// shape is *the* shape needs the guest to accept it and get further, which is a judgement
/// this prints the evidence for rather than making.
fn attempt_questions(
    trial: &mut orbistoun_turn::trial::GuestTrial,
    baseline: &orbistoun_turn::experiment::Outcome,
) -> Result<()> {
    use orbistoun_turn::experiment::Trial as _;
    use orbistoun_turn::question::{Answers, Question, attemptable};

    let knowledge = orbistoun_hle::knowledge::Knowledge::builtin();
    let called = calls_by_function();

    let mut questions = Vec::new();
    for f in knowledge.functions() {
        let asked = f.open_questions_asked();
        if asked.is_empty() || f.answerable_by.is_empty() {
            continue;
        }
        let (calls, _) = called.get(&f.name).copied().unwrap_or((0, 0));
        let open = asked.len();
        for label in &f.answerable_by {
            // **A label nothing recognises is an error, not a silence.** A knowledge file
            // naming an experiment this build does not have is a claim nobody can act on, and
            // dropping it quietly is how it stays that way (principle 3).
            let Some(answers) = Answers::named(label) else {
                println!(
                    "  ! {} names an experiment nothing here has: {label}",
                    f.name
                );
                continue;
            };
            questions.push(Question {
                function: f.name.clone(),
                asked: format!("{open} open question(s)"),
                calls,
                answers: Some(answers),
            });
        }
    }

    let ranked = attemptable(&questions);
    let Some(question) = ranked.first() else {
        return Ok(());
    };

    println!();
    println!(
        "  asking the top open question - {} ({} calls in the corpus)",
        question.function, question.calls
    );
    // **Not one question, because a label is attached to the function.** Printing
    // `asked.first()` paired the experiment with whichever open question happened to be first,
    // which on the top-ranked entry was about argument 1 while the experiment varies the map.
    // A false pairing reads as an answer to the wrong thing (D356).
    println!(
        "    {}; the axes below say what each run asks",
        question.asked
    );

    let Some(answers) = question.answers else {
        return Ok(());
    };
    for axes in answers.axes() {
        let asked = axes
            .first()
            .map_or_else(String::new, orbistoun_turn::axis::Axis::question);
        let outcome = trial
            .spawn_axes(&axes)
            .map_err(|e| anyhow::anyhow!("a question could not be asked: {e}"))?;
        // The same vocabulary the diagnostic axes report in, so a reader is not asked to
        // learn a second one - and the same distinction between a fault that moved and a
        // guest that was broken earlier (D331).
        let change = orbistoun_turn::axis::compare(baseline, &outcome, outcome.planted);
        println!("    {asked}");
        println!("      {}", describe_change(&change));

        // **And what the run was for.** Reach answers "did it crash differently"; the question
        // asked which boundary the guest feeds back, and that is arithmetic on the offsets it
        // queried against the map it was shown - both now in the trace (D357).
        if matches!(answers, Answers::MapShape) {
            if let Ok(trace) = trial.trace() {
                let map = queried_map(&trace);
                let queried = queried_offsets(&trace, &question.function);
                match orbistoun_turn::question::walked_by(&map, &queried) {
                    orbistoun_turn::question::Reading::WalksBy(walk) => {
                        println!("      *** it walks by {walk:?} - the question is answered");
                        // **Written down, not printed.** The loop just established something
                        // nothing in this project knew, and a finding that exists only as
                        // terminal output is already lost - the same rule that made a turn
                        // emit proposals for what it measured (D355, D358).
                        match write_answer(&question.function, walk, &map) {
                            Ok(true) => println!("      recorded as a proposal in patches/"),
                            Ok(false) => {}
                            Err(e) => println!("      could not write the answer: {e:#}"),
                        }
                    }
                    orbistoun_turn::question::Reading::Undecided(why) => {
                        println!("      undecided: {why}");
                    }
                }
            }
        }
    }
    Ok(())
}

/// Writes an answered question into `patches/`, as a change to the entry that asked it.
///
/// # Why an answer is a patch and not a knowledge write
///
/// The loop measured something nothing here knew. That is exactly what a **proposal** is for:
/// inert, undone by deleting it, and promoted by somebody who reads it - the ladder D322
/// settled. Writing it straight into a tracked knowledge file would be the loop editing the
/// project's own record of what it knows, without anybody seeing the diff.
///
/// `known_by = "measured"` rather than `guest-observed`, and the distinction is real: the
/// guest was not merely watched, it was **put in a situation constructed to separate two
/// readings** and its answer was arithmetic. That is the second-strongest oracle this project
/// has, behind a published standard.
///
/// Returns whether anything was written. Nothing is, when the entry already records the
/// answer - re-proposing a settled question every run is how a `patches/` directory becomes
/// noise nobody reads (D358).
fn write_answer(
    function: &str,
    walk: orbistoun_turn::question::Walk,
    map: &[(u64, u64, bool)],
) -> Result<bool> {
    let bare = function.rsplit("::").next().unwrap_or(function);
    let knowledge = orbistoun_hle::knowledge::Knowledge::builtin();
    let Some(library) = knowledge.library_of(bare) else {
        return Ok(false);
    };
    let path = knowledge_path(library);
    let existing =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;

    let says = match walk {
        orbistoun_turn::question::Walk::End => {
            "the guest walks the map by feeding back each region's END"
        }
        orbistoun_turn::question::Walk::NextStart => {
            "the guest walks the map by feeding back the NEXT REGION'S START"
        }
    };
    // Already settled, so nothing to propose. Checked on the sentence rather than the
    // function, because an entry can carry several answers.
    if existing.contains(says) {
        return Ok(false);
    }

    let hole = map
        .windows(2)
        .find(|p| p[1].0 > p[0].1)
        .map_or_else(String::new, |p| {
            format!(
                " A hole from {:#x} to {:#x} separated them.",
                p[0].1, p[1].0
            )
        });
    // **One sentence, built once**, because it is both the thing written and the thing checked
    // for when deciding whether this question is already settled.
    // **Named arguments, not implicit captures.** `concat!` produces a macro call rather than
    // a literal, and implicit capture only works on a literal - so `{says}` inside one is
    // "there is no argument named says". Naming them keeps both the capture and the
    // one-line-literal rule the prose gate enforces (D362).
    let says = format!(
        concat!(
            "MEASURED: {says}, established by running the title against a map with a gap in it ",
            "and reading which side of the hole it queried next.{hole} Answers the open ",
            "question about the second field that D218 left, which no contiguous map could ",
            "settle."
        ),
        says = says,
        hole = hole
    );
    let quoted = format!("\"{}\"", says.replace('\\', "\\\\").replace('"', "\\\""));

    let display = path.display().to_string().replace('\\', "/");
    // **Joined where the key exists, added where it does not.** The first version always
    // inserted, producing `duplicate key edge_cases in table function` from a patch that
    // `git apply` accepted without complaint (D358).
    let diff = match orbistoun_turn::patch::key_line_of(&existing, bare, "edge_cases") {
        Some(at) => {
            let line = existing.lines().nth(at).unwrap_or_default();
            let Some(rest) = line.strip_prefix("edge_cases = [") else {
                return Ok(false);
            };
            orbistoun_turn::patch::replacing_diff(
                &display,
                &existing,
                at,
                // Trimmed, because a multi-line array leaves `rest` empty and the join would
                // end the line with a space - which `git apply` warns about and a reviewer
                // has to look twice at.
                format!("edge_cases = [{quoted}, {rest}").trim_end(),
            )
        }
        None => orbistoun_turn::patch::inserting_diff(
            &display,
            &existing,
            &format!("name = \"{bare}\""),
            &format!("edge_cases = [{quoted}]\n"),
        ),
    };
    let Some(diff) = diff else {
        return Ok(false);
    };

    let into = std::path::Path::new(orbistoun_submit::PATCHES_DIR);
    std::fs::create_dir_all(into).with_context(|| format!("creating {}", into.display()))?;
    let file = format!("{bare}-answer.patch");
    std::fs::write(into.join(&file), diff)
        .with_context(|| format!("writing the answer for {bare}"))?;

    let mut held = match std::fs::read_to_string(into.join(orbistoun_submit::PROPOSALS_FILE)) {
        Ok(text) => orbistoun_submit::Proposals::parse(&text)
            .map_err(|e| anyhow::anyhow!("reading the proposals already there: {e}"))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            orbistoun_submit::Proposals::default()
        }
        Err(e) => return Err(e).context("reading the proposals already there"),
    };
    held.proposal.retain(|p| p.file != file);
    held.proposal.push(orbistoun_submit::Proposal {
        file,
        what: format!("record what {bare} does with the map it is shown"),
        proposed_by: build_stamp(),
        known: orbistoun_hle::knowledge::Oracle::Measured,
        evidence: "ran against a map with a gap and read which side of it the guest queried"
            .to_owned(),
        assumes: vec![
            concat!(
                "the reading holds for maps with one hole; nothing establishes what a guest does ",
                "with several"
            )
            .to_owned(),
        ],
    });
    let text = held
        .to_toml()
        .map_err(|e| anyhow::anyhow!("rendering the proposals: {e}"))?;
    std::fs::write(into.join(orbistoun_submit::PROPOSALS_FILE), text)
        .context("writing the proposals")?;
    Ok(true)
}

/// The physical memory map a run recorded presenting.
///
/// Read from the trace rather than recomputed from the shape that was asked for: a shape whose
/// regions did not fit falls back, and recomputing would compare offsets against a map the
/// guest was never shown (D357).
fn queried_map(trace: &serde_json::Value) -> Vec<(u64, u64, bool)> {
    trace
        .pointer("/conditions/memory_map")
        .and_then(serde_json::Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|r| {
                    let r = r.as_array()?;
                    Some((
                        r.first()?.as_u64()?,
                        r.get(1)?.as_u64()?,
                        r.get(2)?.as_bool()?,
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Every first argument the guest passed to one import, in call order.
///
/// **In order, and duplicates kept.** A walk is a sequence, and a guest that queries the same
/// offset twice is saying something a set would discard.
///
/// **From `tail`, not `calls`.** `calls` is a summary - one row per import with a count and no
/// arguments - so reading it found no offsets at all and reported "the guest queried fewer than
/// two" for a title making twenty million of exactly those calls. `tail` is the ordered record
/// that carries them (D357).
fn queried_offsets(trace: &serde_json::Value, function: &str) -> Vec<u64> {
    let bare = function.rsplit("::").next().unwrap_or(function);
    trace
        .get("tail")
        .and_then(serde_json::Value::as_array)
        .map(|calls| {
            calls
                .iter()
                .filter(|c| {
                    c.get("label")
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|l| l.ends_with(bare))
                })
                .filter_map(|c| c.get("arg0").and_then(serde_json::Value::as_u64))
                .collect()
        })
        .unwrap_or_default()
}

/// One line for what a question's run did, in the dispatcher's own words.
fn describe_change(change: &orbistoun_turn::axis::Change) -> String {
    use orbistoun_turn::axis::Change;

    match change {
        Change::Nothing => "no different from the baseline".to_owned(),
        Change::MovedTo { address } => format!("the guest died at {address:#x} instead"),
        Change::BrokeEarlier {
            address,
            reached,
            was,
        } => format!("broke it earlier: {address:#x}, reaching {reached} against {was}"),
        Change::NoLongerFaulted => {
            "it stopped faulting - reach has saturated, so the probe is what settles it".to_owned()
        }
        Change::NotApplied => "**applied zero times** - this measured nothing".to_owned(),
    }
}

/// Writes what a turn measured as inert proposals, and says how many.
///
/// # Why this needs no flag
///
/// A proposal is a file nothing applies. It changes no behaviour, it is undone by deleting
/// it, and `patches/` is not tracked - so the caution that gates `--apply` does not reach it.
/// That caution is about **policy**, which decides what the next run does; producing the
/// artefact is not that act (D355).
///
/// What it replaces is worse than a flag: a turn given neither flag printed its findings and
/// wrote nothing, so a measured contract survived only as terminal output. `CLAUDE.md` is
/// explicit that anything existing only in a conversation is already lost, and two of three
/// titles diagnosed in one sitting lost their results exactly that way.
///
/// Skipped where the turn measured nothing - an empty `patches/` directory would say a turn
/// had run and found nothing worth proposing, which is a different claim from not having run.
fn write_proposals(
    title: &std::path::Path,
    plan: &[orbistoun_turn::turn::Step],
    taken: &[orbistoun_turn::turn::Taken],
) -> Result<usize> {
    let measured = measurements(title, plan, taken);
    if measured.is_empty() {
        return Ok(0);
    }

    let into = std::path::Path::new(orbistoun_submit::PATCHES_DIR);
    std::fs::create_dir_all(into).with_context(|| format!("creating {}", into.display()))?;

    let mut held = match std::fs::read_to_string(into.join(orbistoun_submit::PROPOSALS_FILE)) {
        Ok(text) => orbistoun_submit::Proposals::parse(&text)
            .map_err(|e| anyhow::anyhow!("reading the proposals already there: {e}"))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            orbistoun_submit::Proposals::default()
        }
        Err(e) => return Err(e).context("reading the proposals already there"),
    };

    let mut written = 0;
    for measurement in &measured {
        // No library, no file to aim a diff at. A measurement recorded before the field
        // existed genuinely does not say which knowledge file it belongs in (D328).
        if measurement.library.is_empty() {
            continue;
        }
        let path = knowledge_path(&measurement.library);
        let Ok(existing) = std::fs::read_to_string(&path) else {
            continue;
        };
        // One claim per function. An entry already there means somebody promoted this, and
        // re-proposing it asks a reviewer to decide which of two is current.
        if existing.contains(&format!("name = \"{}\"", measurement.function)) {
            continue;
        }

        let entry = orbistoun_turn::patch::knowledge_entry(measurement);
        let display = path.display().to_string().replace('\\', "/");
        let file = format!("{}.patch", measurement.function);
        std::fs::write(
            into.join(&file),
            orbistoun_turn::patch::appending_diff(&display, &existing, &entry),
        )
        .with_context(|| format!("writing the patch for {}", measurement.function))?;

        // Replaced rather than appended, for the reason `Learned::record` gives: two entries
        // for one function are two claims about the same thing and nothing here can say which
        // is current. The newer turn measured the newer emulator.
        held.proposal.retain(|p| p.file != file);
        held.proposal.push(orbistoun_submit::Proposal {
            file,
            what: format!("promote {} into {display}", measurement.function),
            proposed_by: build_stamp(),
            known: measurement.known,
            evidence: format!("measured against {}", measurement.measured),
            assumes: measurement.assumes.clone(),
        });
        written += 1;
    }

    if written > 0 {
        let text = held
            .to_toml()
            .map_err(|e| anyhow::anyhow!("rendering the proposals: {e}"))?;
        let path = into.join(orbistoun_submit::PROPOSALS_FILE);
        std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(written)
}

/// The promotion each measurement implies, as a patch a maintainer can apply.
///
/// # What the loop can generate without recalling anything
///
/// `learned.toml` is one machine's cache; a knowledge file is what the emulator **ships**. So
/// the change a measurement is asking for is an entry in one, and every field of it comes from
/// the measurement - which is what keeps a generated patch clear of principle 1. Nothing is
/// recalled, so nothing can be recall dressed as reasoning (D328).
///
/// **Skipped where the entry already exists**, because a second entry for one function is two
/// claims about the same thing and nothing here can say which is current. Skipped too where
/// nothing knows which library declares it - a patch aimed at a guessed file is a patch
/// somebody has to undo.
fn generated_proposals(
    learned: &orbistoun_hle::learned::Learned,
    into: &std::path::Path,
) -> Result<Vec<orbistoun_submit::Proposal>> {
    let mut out = Vec::new();
    for measurement in &learned.measurements {
        // No library, no patch. A promotion aimed at a guessed file is one somebody has to
        // undo, and a measurement recorded before the field existed genuinely does not say.
        if measurement.library.is_empty() {
            continue;
        }
        let path = knowledge_path(&measurement.library);
        let Ok(existing) = std::fs::read_to_string(&path) else {
            continue;
        };
        // One claim per function. The knowledge file already naming it means somebody has
        // promoted this, and re-proposing it would ask a reviewer to decide which is current.
        if existing.contains(&format!("name = \"{}\"", measurement.function)) {
            continue;
        }

        let entry = orbistoun_turn::patch::knowledge_entry(measurement);
        let display = path.display().to_string().replace('\\', "/");
        let diff = orbistoun_turn::patch::appending_diff(&display, &existing, &entry);
        let file = format!("{}.patch", measurement.function);
        std::fs::create_dir_all(into).with_context(|| format!("creating {}", into.display()))?;
        std::fs::write(into.join(&file), diff)
            .with_context(|| format!("writing the patch for {}", measurement.function))?;

        out.push(orbistoun_submit::Proposal {
            file,
            what: format!("promote {} into {display}", measurement.function),
            proposed_by: build_stamp(),
            // **The measurement's own oracle, carried rather than restated.** A promotion is
            // no better known than the observation behind it, and a generator that claimed
            // otherwise would be manufacturing the one field that stops provenance being
            // assumed.
            known: measurement.known,
            evidence: format!("measured against {}", measurement.measured),
            assumes: measurement.assumes.clone(),
        });
    }
    Ok(out)
}

/// Source changes waiting in `patches/`, and what each rests on.
///
/// **Read from a file a person writes, not inferred from the diffs.** A patch cannot say
/// where the behaviour in it came from - only whoever produced it can - and inferring it
/// would manufacture the one field that exists to stop provenance being assumed (principle 1).
fn local_proposals() -> Result<Vec<orbistoun_submit::Proposal>> {
    let path =
        std::path::Path::new(orbistoun_submit::PATCHES_DIR).join(orbistoun_submit::PROPOSALS_FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        // No patches is the ordinary case, not a problem to report.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let held = orbistoun_submit::Proposals::parse(&text)
        .map_err(|e| anyhow::anyhow!("parsing {}: {e}", path.display()))?;
    for proposal in &held.proposal {
        let diff = std::path::Path::new(orbistoun_submit::PATCHES_DIR).join(&proposal.file);
        // **Refused rather than skipped.** A proposal naming a diff that is not there would
        // travel as a description of a change nobody can read, which is worse than no entry.
        anyhow::ensure!(
            diff.exists(),
            "{} names {} and it does not exist",
            path.display(),
            diff.display()
        );
    }
    Ok(held.proposal)
}

/// Routes a `submit` subcommand, so `dispatch` stays a table rather than a body.
fn dispatch_submit(action: &SubmitAction) -> Result<()> {
    let paths = orbistoun_paths::Paths::resolve();
    match action {
        SubmitAction::Export { out, compat_dir } => cmd_submit_export(&paths, compat_dir, out),
        SubmitAction::Check { dir, compat_dir } => cmd_submit_check(&paths, compat_dir, dir),
    }
}

/// `submit export` - gather what this machine has to contribute into one directory.
fn cmd_submit_export(
    paths: &orbistoun_paths::Paths,
    compat_dir: &std::path::Path,
    out: &std::path::Path,
) -> Result<()> {
    let bundle = gathered_bundle(paths, compat_dir)?;
    if bundle.is_empty() {
        // **Refused, and the reason is the message.** An empty bundle reads as "this machine
        // found nothing" when it almost always means the loop was never turned, and those
        // are different facts (principle 3).
        anyhow::bail!(
            concat!(
                "nothing to export - no measurements in {} and no title records in {}.\n\n",
                "Turn the loop first: `orbistoun-cli turn <title> --record` measures, and\n",
                "`orbistoun-cli compat record <title>` records how far a title got."
            ),
            paths.learned_file().display(),
            compat_dir.display()
        );
    }

    std::fs::create_dir_all(out).with_context(|| format!("creating {}", out.display()))?;
    for (name, text) in bundle.to_files().map_err(|e| anyhow::anyhow!("{e}"))? {
        let path = out.join(name);
        std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
    }
    // The diffs travel as files, because a patch is read with the tools people already read
    // patches with. `local_proposals` has already refused any entry naming one that is absent.
    if !bundle.proposals.is_empty() {
        let into = out.join(orbistoun_submit::PATCHES_DIR);
        std::fs::create_dir_all(&into).with_context(|| format!("creating {}", into.display()))?;
        for proposal in &bundle.proposals {
            let from = std::path::Path::new(orbistoun_submit::PATCHES_DIR).join(&proposal.file);
            std::fs::copy(&from, into.join(&proposal.file))
                .with_context(|| format!("copying {}", from.display()))?;
        }
    }

    println!("{}", out.display());
    println!(
        "  {} measurement(s), {} title result(s), built by {}",
        bundle.manifest.measurements, bundle.manifest.titles, bundle.manifest.by
    );
    if bundle.manifest.by.contains("unknown") {
        // **Said where it is produced.** A claim that cannot name the tree it came from is
        // not checkable by whoever receives it, and this is the last moment anybody can fix
        // it before it leaves the machine.
        println!("  ! this build cannot name its commit, so a receiver cannot check it");
        println!("    against a tree - commit first, or set ORBISTOUN_COMMIT");
    }
    if bundle.proposals.is_empty() {
        println!(
            "  send the directory. Nothing in it is a title file: it carries claims, each one\n  reproducible by anybody holding the same title"
        );
    } else {
        // **Said separately, because it is a different offer.** The claims are checkable by
        // a command; a patch is somebody's reading time, and a summary that folded the two
        // together would understate what is being asked for (D322).
        println!(
            "  {} source change(s) as well, in {}/ - those are not claims and nothing",
            bundle.proposals.len(),
            orbistoun_submit::PATCHES_DIR
        );
        println!("  can check them for you. A receiver reads them and runs the gate");
        let vouching = bundle.needing_a_voucher().len();
        if vouching > 0 {
            println!("  ! {vouching} of them rest on nothing better than a guess, and say so");
        }
    }
    Ok(())
}

/// `submit check` - compare a received bundle against what this machine found.
fn cmd_submit_check(
    paths: &orbistoun_paths::Paths,
    compat_dir: &std::path::Path,
    dir: &std::path::Path,
) -> Result<()> {
    let read = |name: &str| -> Result<String> {
        let path = dir.join(name);
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))
    };
    let proposals = match std::fs::read_to_string(dir.join(orbistoun_submit::PROPOSALS_FILE)) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e).context("reading the proposals"),
    };
    let theirs = orbistoun_submit::Bundle::from_files(
        &read(orbistoun_submit::MANIFEST_FILE)?,
        &read(orbistoun_submit::LEARNED_FILE)?,
        &read(orbistoun_submit::RESULTS_FILE)?,
        proposals.as_deref(),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))?;
    let ours = gathered_bundle(paths, compat_dir)?;

    // **Counted from the contents.** Quoting the manifest would report the sender's
    // arithmetic as this machine's measurement, and a bundle edited after it was written
    // then announces totals nothing here checked (D315).
    let (measurements, titles) = theirs.counts();
    println!(
        "{measurements} measurement(s) and {titles} title result(s), submitted by {} on {}",
        theirs.manifest.by, theirs.manifest.on
    );
    if !theirs.manifest_matches_contents() {
        println!(
            "  ! the manifest says {} and {} - it was written before the contents changed",
            theirs.manifest.measurements, theirs.manifest.titles
        );
    }

    // **Listed apart from everything else, because nothing here can check them.** A
    // measurement is settled by re-deriving it; a patch is settled by a person reading it and
    // running the gate. Printing them in the same list would let a diff inherit the trust the
    // measurements earned (D322).
    if !theirs.proposals.is_empty() {
        println!();
        println!(
            "  {} source change(s) proposed - NOT checked by anything here:",
            theirs.proposals.len()
        );
        for proposal in &theirs.proposals {
            println!(
                "    {} - {} [{}, by {}]",
                proposal.file,
                proposal.what,
                proposal.known.label(),
                proposal.proposed_by
            );
            for assumption in &proposal.assumes {
                println!("      assumes: {assumption}");
            }
        }
        let vouching = theirs.needing_a_voucher();
        if !vouching.is_empty() {
            println!(
                concat!(
                    "    {} of them rest on nothing better than a guess. Read the diff, run\n",
                    "    the gate, and merge one only if you can say where the behaviour came\n",
                    "    from - which is principle 1, and the reason a model in the loop is a\n",
                    "    third route to the same problem."
                ),
                vouching.len()
            );
        }
    }

    // **Re-derived, not trusted.** Agreement is silence, and the differences are named
    // individually rather than counted - a count cannot be acted on (D297).
    let disagreements = ours.disagreements(&theirs);
    if disagreements.is_empty() {
        println!("  everything in it agrees with what this machine found");
        return Ok(());
    }
    println!("  {} to settle:", disagreements.len());
    for said in &disagreements {
        println!("    {}", said.say());
    }
    println!();
    println!(concat!(
        "  Nothing here is refuted. A claim this machine never measured is `assumed`\n",
        "  until somebody holding that title confirms it, which is what the `known_by`\n",
        "  ladder is for."
    ));
    Ok(())
}

/// `compat list` - every recorded title, furthest first.
/// Show the corpus manifest: every source and whether each asset is pinned yet.
fn cmd_corpus_list(manifest: &std::path::Path) -> Result<()> {
    let m = orbistoun_corpus::load(manifest)?;
    if m.source.is_empty() {
        println!("no sources in {}", manifest.display());
        return Ok(());
    }
    for src in &m.source {
        println!("{} ({})", src.name, src.kind);
        println!("  cite {}", src.cite);
        if let Some(todo) = &src.todo {
            println!("  TODO {todo}");
        }
        for a in &src.asset {
            let pin = a
                .sha256
                .as_deref()
                .map(|h| &h[..h.len().min(12)])
                .unwrap_or("unpinned");
            println!("    {:<40} {pin}", a.file);
        }
    }
    Ok(())
}

/// Fetch every source's assets into `titles/`, pinning or verifying each by hash.
fn cmd_corpus_sync(
    manifest: &std::path::Path,
    titles: &std::path::Path,
    only: Option<&str>,
) -> Result<()> {
    // A source's relative `path` (and thus a local source) resolves from the repo root, which is
    // where the CLI runs. Kept explicit so the corpus crate does not guess a working directory.
    let root = std::path::Path::new(".");
    let mut m = orbistoun_corpus::load(manifest)?;
    let client = orbistoun_corpus::client()?;
    let mut pinned = false;
    let mut mismatches = 0usize;
    for src in &mut m.source {
        if only.is_some_and(|s| s != src.name) {
            continue;
        }
        println!("{}:", src.name);
        for o in src.sync(root, titles, &client)? {
            let tag = match &o.state {
                orbistoun_corpus::State::PinnedNew => "pinned",
                orbistoun_corpus::State::Verified => "verified",
                orbistoun_corpus::State::Reused => "cached",
                orbistoun_corpus::State::LocalSnapshot => "local",
                orbistoun_corpus::State::Mismatch { .. } => "MISMATCH",
            };
            println!(
                "  {tag:<9} {:<40} {:>9} bytes  {}",
                o.file,
                o.bytes,
                &o.sha256[..o.sha256.len().min(12)]
            );
            if matches!(
                o.state,
                orbistoun_corpus::State::PinnedNew | orbistoun_corpus::State::LocalSnapshot
            ) {
                pinned = true;
            }
            if let orbistoun_corpus::State::Mismatch { expected } = &o.state {
                mismatches += 1;
                println!("    expected {expected}");
            }
        }
    }
    if pinned {
        orbistoun_corpus::save(manifest, &m)?;
        println!("wrote pins into {}", manifest.display());
    }
    if mismatches > 0 {
        anyhow::bail!(
            "{mismatches} asset(s) did not match their pin - a fixed tag's bytes changed; \
             review before re-pinning"
        );
    }
    Ok(())
}

/// Sync, then run every guest and record what it reached to `compat/`.
fn cmd_corpus_run(
    manifest: &std::path::Path,
    titles: &std::path::Path,
    only: Option<&str>,
    limit: u64,
    calls: u64,
    profile: Option<&str>,
) -> Result<()> {
    cmd_corpus_sync(manifest, titles, only)?;
    let m = orbistoun_corpus::load(manifest)?;
    for src in &m.source {
        if only.is_some_and(|s| s != src.name) {
            continue;
        }
        for a in &src.asset {
            let path = src.target(titles, &a.file);
            println!();
            println!("=== {} / {} ===", src.name, a.file);
            // The ordinary run path, which records to compat/ on its own. Deliberately no
            // handoff diagnostic: an intervened run is not recorded (D227), so this measures
            // the honest default-entry baseline. When the elfldr handoff becomes the default
            // entry for payload-shaped guests, these records reflect the progress a diagnostic
            // handoff run shows today (see D411).
            cmd_run(&path, limit, calls, profile, None)?;
        }
    }
    Ok(())
}

fn cmd_compat_list(dir: &std::path::Path) -> Result<()> {
    let mut rows: Vec<(String, orbistoun_overrides::Status)> = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            println!("no records yet - {} does not exist", dir.display());
            return Ok(());
        }
        Err(e) => return Err(e).with_context(|| format!("reading {}", dir.display())),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "toml") {
            continue;
        }
        let Some(title) = path.file_stem().map(|n| n.to_string_lossy().into_owned()) else {
            continue;
        };
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let file = orbistoun_overrides::OverrideFile::from_toml(&text)
            .with_context(|| format!("parsing {}", path.display()))?;
        if let Some(status) = file.status {
            rows.push((title, status));
        }
    }
    if rows.is_empty() {
        println!("no titles recorded yet");
        return Ok(());
    }
    // Ranked and rendered one layer down, so the table and the record cannot disagree
    // about which title is closest to running - and so a test can hold the whole shape
    // against the records in the tree (D184).
    let ranked = orbistoun_overrides::frontier(rows);
    println!("{} titles recorded", ranked.len());
    println!();
    print!("{}", orbistoun_overrides::render_frontier(&ranked));
    Ok(())
}

/// `compat markdown` - render every record as a ranked markdown table into a tracked file.
fn cmd_compat_markdown(
    dir: &std::path::Path,
    out: &std::path::Path,
    shots: &std::path::Path,
) -> Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            println!("no records yet - {} does not exist", dir.display());
            return Ok(());
        }
        Err(e) => return Err(e).with_context(|| format!("reading {}", dir.display())),
    };
    let mut rows = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "toml") {
            continue;
        }
        let Some(title) = path.file_stem().map(|n| n.to_string_lossy().into_owned()) else {
            continue;
        };
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let file = orbistoun_overrides::OverrideFile::from_toml(&text)
            .with_context(|| format!("parsing {}", path.display()))?;
        // Prefer the honest baseline (`status`); fall back to the `experiment` slot, marking it so
        // a reader knows the number came from a run with overrides and is less comparable (D181).
        let (status, experiment) = match (file.status, file.experiment) {
            (Some(s), _) => (s, false),
            (None, Some(e)) => (e, true),
            // A file that only configures a title and has measured nothing is not a row.
            (None, None) => continue,
        };
        // A screenshot if one sits beside the record. Embedded relative to the repo root, which
        // is where the table is written by default.
        let shot = shots.join(format!("{title}.png"));
        let screenshot = shot.exists().then(|| {
            shots
                .join(format!("{title}.png"))
                .to_string_lossy()
                .replace('\\', "/")
        });
        rows.push(orbistoun_overrides::Row {
            title,
            status,
            experiment,
            screenshot,
        });
    }
    if rows.is_empty() {
        println!("no titles recorded yet");
        return Ok(());
    }
    let count = rows.len();
    let table = orbistoun_overrides::render_markdown(&rows);
    let doc = format!(
        "# Compatibility\n\n\
         _Generated by `orbistoun-cli compat markdown` from the records in `compat/`. Do not edit \
         by hand; re-run it. Ranked by how far each guest got - reach, then distinct imports, then \
         standing, then calls._\n\n\
         **From** is `run` for the honest default-entry baseline and `experiment` for a run \
         recorded with overrides (less comparable - D181). A 📷 marks a guest with a captured \
         framebuffer; see the Screenshots section.\n\n\
         {table}"
    );
    std::fs::write(out, doc).with_context(|| format!("writing {}", out.display()))?;
    println!("wrote {} ({count} titles)", out.display());
    Ok(())
}

/// `compat record` - transcribe the last run of a title into its record.
fn cmd_compat_record(
    path: &std::path::Path,
    dir: &std::path::Path,
    note: Option<&str>,
    force: bool,
) -> Result<()> {
    let title = title_id(path)
        .with_context(|| format!("{} has no containing directory to name it", path.display()))?;

    let trace = previous_trace(path).with_context(|| {
        format!("no trace for {title} - run it first, so there is a measurement to record")
    })?;

    let mut status = orbistoun_report::trace::status_of(&trace, orbistoun_nid::today());
    if let Some(note) = note {
        note.clone_into(&mut status.notes);
    }

    match keep_status(dir, &title, &status, force)? {
        Kept::NotBetter { slot, previous } => anyhow::bail!(
            concat!(
                "{}: not recorded - the {} entry is better or equal ({} {} imports, ",
                "{} calls).\n\nUse --force to record it anyway."
            ),
            title,
            slot,
            previous.reach.label(),
            previous.imports,
            previous.calls
        ),
        Kept::Written { slot, path } => {
            println!(
                "{}: [{slot}] {} - {} imports, {} calls, {}% standing",
                path.display(),
                status.reach.label(),
                status.imports,
                status.calls,
                status.standing
            );
            if status.propped_up() {
                // Said at the moment it is written, not left for whoever reads the file
                // later. The number is real and it is not a claim about the emulator as it
                // stands.
                println!(
                    "  under {} - kept apart from the honest record",
                    status.describe_policy()
                );
            }
        }
    }
    Ok(())
}

/// What happened to a status offered to the record.
enum Kept {
    /// Written into the named slot.
    Written {
        /// Which slot took it.
        slot: &'static str,
        /// The file written.
        path: std::path::PathBuf,
    },
    /// That slot already holds something better or equal.
    NotBetter {
        /// Which slot was compared against.
        slot: &'static str,
        /// What it holds.
        previous: orbistoun_overrides::Status,
    },
}

/// Puts a status in the slot its policy belongs to, if it improves on what is there.
///
/// # Why this is one function and not two callers doing the same thing
///
/// A run records automatically and `compat record` records on request, and they were about to
/// be two implementations of *which slot, and is this better* - the pair of rules that decide
/// whether the compatibility database means anything. Two copies of that drift, and the drift
/// is invisible until the two disagree about one title (D323).
///
/// **Routed rather than refused.** A run under a measured policy is a real result about a
/// different question, so it goes in its own slot; the only refusal left is within a slot,
/// where an automatic best-ever that moves backwards is not a record of anything (D312).
fn keep_status(
    dir: &std::path::Path,
    title: &str,
    status: &orbistoun_overrides::Status,
    force: bool,
) -> Result<Kept> {
    let propped = status.propped_up();
    let slot = if propped { "experiment" } else { "status" };

    let mut file = load_compat(dir, title)?;
    let previous = if propped {
        file.experiment.as_ref()
    } else {
        file.status.as_ref()
    };
    if let Some(previous) = previous {
        if !force && !status.beats(previous) {
            return Ok(Kept::NotBetter {
                slot,
                previous: previous.clone(),
            });
        }
    }
    if propped {
        file.experiment = Some(status.clone());
    } else {
        file.status = Some(status.clone());
    }

    let text = file.to_toml().context("rendering the record")?;
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let path = compat_path(dir, title);
    std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
    Ok(Kept::Written { slot, path })
}

/// Keeps what a run achieved, in the slot its policy belongs to.
///
/// **What makes the record maintain itself, and it used to only ask.** A compatibility
/// database nobody updates becomes a graveyard of stale entries within a month, and the
/// moment anybody notices is the moment they stop trusting all of it (D182). This printed
/// the command and waited: `compat/` then sat untouched for four days while the loop kept
/// finding things, so the repository's record of a title disagreed with every run anybody
/// had done. **A prompt nobody acts on measured nothing.**
///
/// The run already knows both numbers, so writing costs no more than asking did - and it is
/// safe unattended only because of the slot routing, which stopped a helped run being able
/// to overwrite the honest one (D312, D323).
fn record_compat(path: &std::path::Path, trace: &orbistoun_report::trace::CallTrace) {
    let dir = std::path::Path::new("compat");
    let Some(title) = title_id(path) else {
        return;
    };
    // **A title id, not whatever the containing directory was called.** Recording is automatic
    // now, and a binary sitting in a scratch folder produced `compat/New folder (2).toml` -
    // a tracked record named after somebody's Explorer default, describing a payload rather
    // than a title. A title id is an identifier: letters, digits, dot, dash, underscore.
    //
    // Said rather than skipped, because a run that quietly declines to record is
    // indistinguishable from one that had nothing to record (D347).
    if !title
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    {
        println!();
        println!("  not recorded: {title:?} is a directory name, not a title id");
        println!("  move the module under a directory named for the title to record it");
        return;
    }
    // **An intervened run is not a result about the emulator.** A diagnostic that maps memory
    // the guest never asked for, plants a value or forces an answer has changed the program
    // being measured, so what it reached says nothing about what the title reaches (D227).
    //
    // Found by adding recording to `turn`: the last boot of a turn is a diagnostic, so the
    // first turn to record filed `25 imports, ran to the time limit` for a title that reaches
    // 13 - a number bought by a reserved region, filed as a compatibility claim. The hazard
    // was already there for `run` under any diagnostic; nothing had exercised it (D355).
    if trace.conditions.intervened {
        println!();
        println!("  not recorded: this run was under a diagnostic, so what it reached is");
        println!("  a fact about the intervention rather than about the title");
        return;
    }
    let status = orbistoun_report::trace::status_of(trace, orbistoun_nid::today());

    // **Only into a directory that already exists.** Creating one in whatever directory
    // somebody happened to run from is a surprise, and a surprise that writes files. A
    // checkout always has it; anybody else opts in once by making it.
    if !dir.is_dir() {
        println!();
        println!("  {title} reached {} imports, unrecorded", status.imports);
        println!("  `mkdir compat` and every run after this one records itself");
        return;
    }

    // **Written rather than suggested.** This printed the command to run and waited for
    // somebody to type it, and `compat/` then went four days without being touched while the
    // loop kept finding things - so the repository's own record of a title disagreed with
    // every run anybody had done. A prompt nobody acts on is a prompt that measured nothing.
    //
    // Safe to do unattended only because of the slot routing: a helped run can no longer
    // overwrite the honest number, and nothing here can move an entry backwards (D312, D323).
    match keep_status(dir, &title, &status, false) {
        Ok(Kept::Written { slot, path }) => {
            println!();
            println!(
                "  recorded [{slot}] {} - {} imports, {} calls, {}% standing",
                status.reach.label(),
                status.imports,
                status.calls,
                status.standing
            );
            println!("  {}", path.display());
        }
        // Nothing to say. The record already holds something as good, which is the ordinary
        // outcome of a run that changed nothing.
        Ok(Kept::NotBetter { .. }) => {}
        // **Reported, not swallowed.** A run that could not write its own result is a run
        // whose finding exists only on this screen, and silence there is how four days of
        // them went missing in the first place.
        Err(e) => {
            println!();
            println!("  could not record {title}: {e:#}");
        }
    }
}

/// `env` - every variable this build reads, and what is set right now.
///
/// # Why this is a command and not a paragraph in a document
///
/// It was a paragraph in a document, hand-copied from three decision entries, and that is a
/// second list. The registry is the first one, so this prints from it - a variable added
/// anywhere appears here without anybody remembering to write it down (D221).
///
/// Settings and diagnostics are separated because they are different kinds of thing: one
/// configures the emulator, the other changes the program in order to learn something and
/// is meant to go away afterwards.
/// Prints the firmware skeleton's libkernel layout: every export's stub kind and any overrun.
///
/// Uses the same pure planner the worker places from, so what this prints is what a run lays
/// down. The implemented set comes from the service's declared symbols, so an export shows as a
/// trampoline exactly when a run would give it one.
fn cmd_firmware_layout(service: &Service, all: bool) {
    use orbistoun_firmware::SlotKind;

    let implemented: std::collections::BTreeSet<String> = service
        .declared_symbols()
        .into_iter()
        .filter(|d| d.implemented)
        .map(|d| d.symbol)
        .collect();

    let exports: Vec<(String, u64)> = orbistoun_firmware::libkernel_exports()
        .iter()
        .map(|(n, v)| ((*n).to_owned(), *v))
        .collect();
    let plan = orbistoun_firmware::plan_layout(&exports, |name| implemented.contains(name));

    let (mut trampolines, mut unimplemented, mut collisions, mut confirmed) =
        (0_usize, 0_usize, 0_usize, 0_usize);
    println!(
        "libkernel laid out at {:#x}, {} exports",
        orbistoun_firmware::LIBKERNEL_BASE,
        plan.len()
    );
    println!(
        "{:>10}  {:<12}  {:<10}  {:<40}  NOTE",
        "VADDR", "KIND", "PROV", "EXPORT"
    );
    for p in &plan {
        match p.kind {
            SlotKind::Anchor => {}
            SlotKind::Trampoline => trampolines += 1,
            SlotKind::Unimplemented => unimplemented += 1,
        }
        let kind = match p.kind {
            SlotKind::Anchor => "anchor",
            SlotKind::Trampoline => "trampoline",
            SlotKind::Unimplemented => "unimplemented",
        };
        let prov = match orbistoun_firmware::libkernel_provenance(&p.name) {
            orbistoun_firmware::Provenance::Confirmed => {
                confirmed += 1;
                "confirmed"
            }
            orbistoun_firmware::Provenance::Candidate => "candidate",
        };
        let note = match &p.collides_with {
            Some((next, at)) => {
                collisions += 1;
                format!("OVERRUNS {next} at {at:#x}")
            }
            None => String::new(),
        };
        // By default a full 1,867-line dump is noise; show the anchor, the unimplemented ones
        // (the work list) and any collision unless asked for everything.
        let worth_showing = all
            || p.kind == SlotKind::Anchor
            || p.kind == SlotKind::Unimplemented
            || p.collides_with.is_some();
        if worth_showing {
            println!(
                "{:>10x}  {kind:<12}  {prov:<10}  {:<40}  {note}",
                p.vaddr, p.name
            );
        }
    }
    println!(
        "\n1 anchor, {trampolines} implemented, {unimplemented} unimplemented, {collisions} collisions"
    );
    println!(
        "{confirmed} vaddrs behaviourally confirmed, {} still candidate",
        plan.len().saturating_sub(confirmed)
    );
    if !all && unimplemented + collisions > 0 {
        println!(
            "(showing the anchor, unimplemented exports and collisions; --all for every export)"
        );
    }
}

fn cmd_env() {
    use orbistoun_env::Kind;

    let width = orbistoun_env::REGISTRY
        .iter()
        .map(|v| v.name.len())
        .max()
        .unwrap_or(24);

    for (kind, heading) in [
        (Kind::Setting, "settings - configure how orbistoun behaves"),
        (
            Kind::Diagnostic,
            "diagnostics - change a run to find something out, then go away",
        ),
    ] {
        println!();
        println!("{heading}");
        for var in orbistoun_env::REGISTRY.iter().filter(|v| v.kind == kind) {
            // The current value, because "what are the names" and "what is set" are the
            // same question in practice - somebody reads this when a run did not do what
            // they expected, and a stale variable from an earlier shell is a real cause.
            let state = var
                .get()
                .map_or_else(|| "-".to_owned(), |value| format!("= {value}"));
            println!("  {:<width$}  {state}", var.name);
            println!("  {:<width$}  {} ({})", "", var.summary, var.read_by);
            println!("  {:<width$}  e.g. {}={}", "", var.name, var.example);
        }
    }

    // The other half of "what configures a run", because somebody reading this list is
    // asking that question and the environment is the smaller half of the answer.
    println!();
    println!("most settings live in config.toml, not here - see `orbistoun-cli paths`");

    // **The reason the registry exists**, printed where somebody will see it rather than
    // only inside a run. A misspelled variable is not an error - it is an absence, so the
    // run reports an ordinary result and is believed.
    let unknown = orbistoun_env::unknown();
    if !unknown.is_empty() {
        println!();
        println!(
            "{} variable(s) set that look like orbistoun's and are not:",
            unknown.len()
        );
        for name in &unknown {
            println!("  {name}   - misspelled? nothing reads this");
        }
    }
}

/// `serve` - answer the conformance probe's command protocol.
///
/// # Why the key is printed rather than configured
///
/// It is generated per start and shown once, which is the same shape obSCEne uses and for
/// the same reason: a secret compiled in is shared by everyone holding the binary, and a
/// secret read from a file is one that outlives the reason it was created.
///
/// # Errors
///
/// When the address cannot be bound, or when a wide bind is asked for without a key.
fn cmd_serve(service: &Service, bind: &str, no_key: bool, once: bool) -> Result<()> {
    use std::net::TcpListener;

    // Refused rather than warned about. The two decisions - "no password" and "anything on
    // this network may invoke this" - are separate, and only the first one was made here.
    if no_key && !is_loopback(bind) {
        anyhow::bail!(
            "--no-key on {bind} would leave this open to anything on the network - bind to loopback, or drop --no-key"
        );
    }

    let listener = TcpListener::bind(bind)?;
    let shown = listener
        .local_addr()
        .map_or_else(|_| bind.to_owned(), |a| a.to_string());

    // Once, before anything can connect. A secret minted per connection is one no driver
    // could have presented, which would make the check unpassable rather than secure.
    let secret = (!no_key).then(orbistoun_service::respond::ServiceAnswers::generate_secret);

    println!("listening  {shown}");
    match &secret {
        Some(key) => println!("key {key}"),
        None => println!("key none - unauthenticated, loopback only"),
    }
    println!("serving report");
    println!("declining  call, read - no guest is loaded, so neither is announced");

    for incoming in listener.incoming() {
        let stream = incoming?;
        let peer = stream
            .peer_addr()
            .map_or_else(|_| "unknown".to_owned(), |a| a.to_string());
        println!("session {peer}");
        // A fresh backend per connection so the *session identifier* is new, carrying the
        // same secret so the key printed above stays the one that works.
        let per_session =
            orbistoun_service::respond::ServiceAnswers::with_secret(service, secret.clone());
        let mut responder = orbistoun_probe::respond::Responder::new(stream, per_session);
        if let Err(e) = responder.serve() {
            // Not fatal. A driver that disconnects mid-command is ordinary, and taking the
            // listener down with it would make every dropped connection look like a crash.
            println!("session ended: {e}");
        }
        if once {
            break;
        }
    }
    Ok(())
}

/// Whether an address is one only this machine can reach.
fn is_loopback(bind: &str) -> bool {
    use std::net::ToSocketAddrs as _;

    bind.to_socket_addrs()
        .is_ok_and(|mut addresses| addresses.all(|address| address.ip().is_loopback()))
}

/// `paths` - show where orbistoun reads and writes.
/// Where a generated block starts, and what regenerates it.
const BLOCK_OPEN: &str = "<!-- generated by `orbistoun-cli status` - do not edit by hand -->";
/// Where a generated block ends.
const BLOCK_CLOSE: &str = "<!-- end generated -->";

/// The files carrying a generated numbers block.
const GENERATED_IN: &[&str] = &["README.md", "docs/PROJECT_STATUS.md"];

/// Where the per-title table starts.
const TITLES_OPEN: &str = "<!-- generated by `orbistoun-cli status` - read from compat/ -->";
/// Where it ends.
const TITLES_CLOSE: &str = "<!-- end titles -->";
/// The one file carrying it. The README says how far the project is, not title by title.
const TITLES_IN: &str = "docs/PROJECT_STATUS.md";

/// `status` - the numbers block, generated rather than typed.
///
/// # What is in it and what deliberately is not
///
/// Only numbers this tool can recompute **anywhere**, from what the repository ships. A
/// count that needs a title cannot be one of them: the corpus is not tracked and never will
/// be, so a generated block containing "6 of 6 titles execute guest code" would be
/// unverifiable in CI and would fail for every contributor who has no titles.
///
/// Those numbers still belong in the documentation - they are the most interesting ones -
/// so they live in the prose around the block, where they read as what they are: measured
/// from a run, on one machine, on material not in this repository (D240).
fn cmd_status(service: &Service, write: bool, check: bool) -> Result<()> {
    let block = numbers_block(service);

    if !write && !check {
        println!("{block}");
        return Ok(());
    }

    let mut drifted = Vec::new();
    for file in GENERATED_IN {
        let text = std::fs::read_to_string(file).with_context(|| format!("reading {file}"))?;
        let Some(updated) = splice_block(&text, &block) else {
            // Said out loud rather than skipped. A file that lost its markers would
            // otherwise be silently left behind, holding whatever numbers it last had -
            // which is the drift this exists to stop, with a check that reports success.
            anyhow::bail!("{file} has no generated block - the markers are missing");
        };
        if updated == text {
            continue;
        }
        if write {
            std::fs::write(file, &updated).with_context(|| format!("writing {file}"))?;
            println!("updated {file}");
        } else {
            drifted.push((*file).to_owned());
        }
    }

    // The per-title table, in the one file that carries it. Read from `compat/` rather than
    // typed, because the copy that was typed had been wrong for four days (D329).
    let titles = titles_block(std::path::Path::new("compat"))?;
    let text =
        std::fs::read_to_string(TITLES_IN).with_context(|| format!("reading {TITLES_IN}"))?;
    match splice_between(&text, &titles, TITLES_OPEN, TITLES_CLOSE) {
        None => anyhow::bail!("{TITLES_IN} has no title block - the markers are missing"),
        Some(updated) if updated == text => {}
        Some(updated) => {
            if write {
                std::fs::write(TITLES_IN, &updated)
                    .with_context(|| format!("writing {TITLES_IN}"))?;
                println!("updated {TITLES_IN} (titles)");
            } else {
                drifted.push(format!("{TITLES_IN} (titles)"));
            }
        }
    }

    if check && !drifted.is_empty() {
        anyhow::bail!(
            "the numbers in {} are not what the tool reports - run `orbistoun-cli status --write`",
            drifted.join(", ")
        );
    }
    if write && drifted.is_empty() {
        println!("every generated block is current");
    }
    Ok(())
}

/// A count with thousands separated, because one title makes ninety-nine million calls.
///
/// The hand-written table had these and the first generated one did not, which is a small
/// thing and exactly the kind of small thing that makes a generated table read as a
/// regression rather than a repair.
fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (seen, ch) in digits.chars().rev().enumerate() {
        if seen > 0 && seen % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

/// The per-title table, read from the records rather than typed.
///
/// # Why this one may be generated when the counts around it may not
///
/// D240 keeps a number out of a generated block when computing it needs a **title**, because
/// the corpus is not tracked and never will be - a block claiming "6 of 6 execute guest code"
/// would fail for every contributor who has none. That reasoning is about needing a *run*.
///
/// `compat/` is committed. Reading it recomputes from what the repository ships, works in CI,
/// and works for somebody with no titles at all - so the rule permits it, and the table it
/// replaces had been wrong for four days: it still said 23 imports and `image+0xafc959` for a
/// title the records put at 25 and `image+0xafcc08` (D329).
///
/// **The honest slot only.** `[experiment]` is a real result about a different question, and a
/// table that mixed the two would be the propped-up number wearing the honest one's clothes.
/// Titles that have one are named underneath, saying what they reached and under what.
fn titles_block(dir: &std::path::Path) -> Result<String> {
    use core::fmt::Write as _;

    let mut honest: Vec<(String, orbistoun_overrides::Status)> = Vec::new();
    let mut helped: Vec<(String, orbistoun_overrides::Status)> = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(format!(
                "{TITLES_OPEN}\n\nNo records yet.\n\n{TITLES_CLOSE}"
            ));
        }
        Err(e) => return Err(e).with_context(|| format!("reading {}", dir.display())),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "toml") {
            continue;
        }
        let Some(title) = path.file_stem().map(|n| n.to_string_lossy().into_owned()) else {
            continue;
        };
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let file = orbistoun_overrides::OverrideFile::from_toml(&text)
            .with_context(|| format!("parsing {}", path.display()))?;
        if let Some(status) = file.status {
            honest.push((title.clone(), status));
        }
        if let Some(experiment) = file.experiment {
            helped.push((title, experiment));
        }
    }

    let mut out = String::new();
    let _ = writeln!(out, "{TITLES_OPEN}\n");
    if honest.is_empty() {
        let _ = writeln!(out, "No title has an unassisted record yet.\n");
        let _ = write!(out, "{TITLES_CLOSE}");
        return Ok(out);
    }

    let _ = writeln!(out, "| Title | Reach | Imports | Calls | Standing | Ends |");
    let _ = writeln!(out, "|---|---|---|---|---|---|");
    // Ranked one layer down, so this table and `compat list` cannot disagree about which
    // title is closest to running (D160).
    for (title, status) in orbistoun_overrides::frontier(honest) {
        let _ = writeln!(
            out,
            "| {title} | {} | {} | {} | {}% | `{}` |",
            status.reach.label(),
            status.imports,
            grouped(status.calls),
            status.standing,
            status.outcome
        );
    }

    if !helped.is_empty() {
        helped.sort_by(|a, b| a.0.cmp(&b.0));
        let _ = writeln!(
            out,
            "\nUnder a measured policy - stubs answering by name, so these reach further by\nconstruction and are not comparable with the table above:\n"
        );
        for (title, status) in helped {
            let _ = writeln!(
                out,
                "- **{title}** reached {} imports, ending at `{}`, with {} function{} answered by name",
                status.imports,
                status.outcome,
                status.overrides,
                if status.overrides == 1 { "" } else { "s" }
            );
        }
    }
    let _ = write!(out, "\n{TITLES_CLOSE}");
    Ok(out)
}

/// The block itself, as markdown.
fn numbers_block(service: &Service) -> String {
    use orbistoun_hle::knowledge::Oracle;

    let mut declared = 0_usize;
    let mut implemented = 0_usize;
    for d in service.declared_symbols() {
        declared += 1;
        implemented += usize::from(d.implemented);
    }

    let knowledge = orbistoun_hle::knowledge::Knowledge::builtin();
    let resting: Vec<String> = [
        Oracle::Published,
        Oracle::Measured,
        Oracle::GuestObserved,
        Oracle::Assumed,
    ]
    .iter()
    .map(|o| (o, knowledge.resting_on(*o)))
    .filter(|(_, n)| *n > 0)
    .map(|(o, n)| format!("{n} {}", o.label()))
    .collect();

    let db = orbistoun_nid::SymbolDbFile::builtin();
    // Strongest claim first, in the enum's own order rather than alphabetically by label -
    // the same rule `print_by_tier` follows, so a listing cannot disagree with the type
    // about which claim is stronger.
    let mut split: Vec<String> = [
        orbistoun_nid::Reproducible::FromRepository,
        orbistoun_nid::Reproducible::FromModule,
        orbistoun_nid::Reproducible::FromRun,
        orbistoun_nid::Reproducible::FromHardware,
        orbistoun_nid::Reproducible::OnlyFromItsSource,
    ]
    .iter()
    .map(|tier| {
        let n = db
            .derivations
            .values()
            .filter(|d| d.method.reproducible() == *tier)
            .count();
        (tier, n)
    })
    .filter(|(_, n)| *n > 0)
    .map(|(tier, n)| format!("{n} {}", tier.label()))
    .collect();
    let unaccounted = db.names.len().saturating_sub(db.derivations.len());
    // Always stated, including when it is zero. "Nothing unaccounted for" is the claim the
    // audit exists to support, and a line that appears only when the news is bad reads as
    // absence of the check rather than absence of the problem.
    split.push(format!("{unaccounted} unaccounted"));

    format!(
        "{BLOCK_OPEN}

| | |
|---|---|
| Functions declared / implemented | {declared} / {implemented} |
| Recorded behaviours | {} - {} |
| Open questions a hardware probe could settle | {} |
| Symbol database | {} names - {} |

{BLOCK_CLOSE}",
        knowledge.len(),
        resting.join(", "),
        knowledge.open_questions(),
        db.names.len(),
        split.join(", "),
    )
}

/// Replaces whatever sits between the markers, or `None` when they are not both there.
fn splice_block(text: &str, block: &str) -> Option<String> {
    splice_between(text, block, BLOCK_OPEN, BLOCK_CLOSE)
}

/// The same, between whichever markers a block uses.
///
/// Extracted when a second block appeared, rather than copied - two spliceers is two places
/// for "what if the markers are the wrong way round" to be answered differently.
fn splice_between(text: &str, block: &str, open: &str, close: &str) -> Option<String> {
    let start = text.find(open)?;
    let end = text.find(close)? + close.len();
    if end < start {
        return None;
    }
    let mut out = String::with_capacity(text.len());
    out.push_str(&text[..start]);
    out.push_str(block);
    out.push_str(&text[end..]);
    Some(out)
}

fn cmd_paths() {
    let paths = orbistoun_paths::Paths::resolve();
    // Read from the one list rather than re-typed here. This was a second hand-written
    // enumeration of the same directories, which meant a new writable location could pass
    // the containment test and still never appear in the answer to "where did it go?"
    // (D215).
    let named = paths.named_dirs();
    // Measured, not typed. A ten-wide column was right until `screenshots` arrived and
    // pushed its own path out of line - the same class of thing as the list above, one
    // step smaller.
    let width = named
        .iter()
        .map(|(name, _)| name.len())
        .chain(["build", "mode", "data", "config", "library"].map(str::len))
        .max()
        .unwrap_or(10);
    // Which build this is, first. A path listing is what someone reads when an answer
    // surprised them, and the second question is always "which binary said that".
    println!("{:<width$} {}", "build", orbistoun_env::build::line());
    println!(
        "{:<width$} {}",
        "mode",
        if paths.is_portable() {
            "portable - everything lives beside the binary"
        } else {
            "installed - the platform data directory"
        }
    );
    println!("{:<width$} {}", "data", paths.data_root().display());
    for (name, dir) in &named {
        println!("{name:<width$} {}", dir.display());
    }
    println!("{:<width$} {}", "config", paths.config_file().display());
    // The one location here that is *read* rather than written, and the one nobody could
    // find out. A relative library root is joined to the data root above rather than to
    // the working directory, so "where does it look for titles" has a single answer -
    // which is exactly what it did not have (D228).
    let library = orbistoun_service::FileConfig::load(&paths.config_file())
        .unwrap_or_default()
        .library
        .resolve(paths.data_root());
    println!(
        "{:<width$} {}{}",
        "library",
        library.display(),
        if library.is_dir() {
            ""
        } else {
            "   (not a folder - nothing will be found)"
        }
    );
}

/// Prints how this corpus compares with the last time it was looked at, and records it.
///
/// # Why the shader work needs this
///
/// The import side ends every run with `FURTHER`, `same` or `BACK`, and that is what makes
/// it iterable: a change either moved something or it did not, and nobody carries two
/// numbers between runs in their head. The shader side has had the same loop all along -
/// rank what blocks, implement the top entry, run again - with no way to say whether it
/// worked except reading figures off consecutive screens.
///
/// Deliberately the same vocabulary as the import side. They are one loop pointed at
/// different material, and giving them different words would suggest otherwise.
///
/// Keyed by corpus path, so several corpora do not overwrite each other's history - the
/// same reason traces are keyed by module.
fn report_shader_movement(
    coverage: &orbistoun_shader::coverage::CorpusCoverage,
    encodings: &orbistoun_shader::EncodingTable,
    mnemonics: &orbistoun_shader::MnemonicTable,
    corpus: &std::path::Path,
) {
    use orbistoun_shader::coverage::{Summary, Verdict};

    let describe = |key: orbistoun_shader::coverage::OpcodeKey| {
        let family = key
            .encoding
            .and_then(|i| encodings.encodings().get(usize::from(i)))
            .map(|e| e.name.as_str());
        match family {
            Some(family) => mnemonics
                .name(family, key.opcode)
                .map_or_else(|| format!("{family}:{:#x}", key.opcode), str::to_owned),
            None => format!("unrecognised:{:#x}", key.opcode),
        }
    };

    let summary = Summary::of(coverage, describe);
    let previous = read_shader_summary(corpus);
    let movement = summary.movement(previous.as_ref());

    println!();
    println!("progress");
    if movement.verdict == Verdict::FirstRun {
        println!("  first look at this corpus - nothing to compare against yet");
    } else {
        println!(
            "  {:<8} {} of {} shaders complete ({:+}), {} of {} instructions ({:+})",
            movement.verdict.label(),
            summary.complete,
            summary.shaders,
            movement.complete_delta,
            summary.translatable,
            summary.instructions,
            movement.translatable_delta,
        );
        for name in &movement.cleared {
            println!("  cleared {name}");
        }
        // Reported apart from a regression: implementing one blocker routinely uncovers
        // the next instruction in a shader that could not be reached past it, and that is
        // progress rather than breakage.
        for name in &movement.uncovered {
            println!("  uncovered  {name}");
        }
    }

    write_shader_summary(corpus, &summary);
}

/// Where a corpus's history lives.
///
/// Named from the path so two corpora do not share one file. Hashed rather than escaped
/// because a path is not a filename and making one into the other legibly is a problem
/// nobody needs solved here.
fn shader_summary_path(corpus: &std::path::Path) -> std::path::PathBuf {
    let paths = orbistoun_paths::Paths::resolve();
    // A small stable digest of the path. Not a security property - it only has to give
    // two different corpora two different filenames.
    let mut key: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in corpus.display().to_string().bytes() {
        key ^= u64::from(byte);
        key = key.wrapping_mul(0x0000_0100_0000_01B3);
    }
    paths.shaders_dir().join(format!("{key:016x}.json"))
}

fn read_shader_summary(corpus: &std::path::Path) -> Option<orbistoun_shader::coverage::Summary> {
    let text = std::fs::read_to_string(shader_summary_path(corpus)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Records this run, so the next one has something to compare against.
///
/// A failure here is reported and not fatal. The comparison is a convenience; refusing to
/// print a worklist because a history file could not be written would trade the useful
/// output for the optional one.
fn write_shader_summary(corpus: &std::path::Path, summary: &orbistoun_shader::coverage::Summary) {
    let path = shader_summary_path(corpus);
    let written = path
        .parent()
        .map(std::fs::create_dir_all)
        .transpose()
        .and_then(|_| serde_json::to_string_pretty(summary).map_err(std::io::Error::other))
        .and_then(|text| std::fs::write(&path, text));
    if let Err(error) = written {
        eprintln!(
            "note: this run was not recorded ({error}), so the next one has nothing to compare against"
        );
    }
}

/// `worklist` - rank what to implement next, across every run so far.
/// One thing this project has written down that it does not know.
#[derive(serde::Serialize)]
struct OpenQuestion {
    /// The function it is about - a name, or a hash where there is no name yet.
    function: String,
    /// Which library it belongs to.
    library: String,
    /// The question itself, as recorded.
    question: String,
    /// How many times guests have called it across every run so far.
    ///
    /// **The ranking.** A question about a function called nine hundred times is worth
    /// more than one about a function nothing has reached, and without this the queue is
    /// alphabetical - which is the same as unordered.
    calls: u64,
    /// How many titles called it.
    modules: usize,
    /// What the function hands back, where that is established.
    ///
    /// Carried because it is the dispatch key for a property: everything returning a
    /// handle can be asked the same questions, and so can everything returning a count.
    /// A probe can generate tests from the shape without knowing the function.
    #[serde(skip_serializing_if = "Option::is_none")]
    returns: Option<String>,
    /// How many integer arguments it takes, where that is established.
    #[serde(skip_serializing_if = "Option::is_none")]
    arity: Option<u8>,
    /// What it currently rests on, so an answer can be seen to upgrade it.
    #[serde(skip_serializing_if = "Option::is_none")]
    known_by: Option<String>,
}

/// `questions` - every open question, ranked by how often a guest calls the function.
fn cmd_questions(top: Option<usize>, json: bool) {
    let knowledge = orbistoun_hle::knowledge::Knowledge::builtin();
    let called = calls_by_function();

    let mut queue: Vec<OpenQuestion> = Vec::new();
    for f in knowledge.functions() {
        // Asked of the entry rather than assembled here. This shim used to apply the rule
        // itself - items, plus one for a silent guess - while `open_questions` applied a
        // different one, so `knows` reported 80 and this reported 70 of the same knowledge
        // base and neither said which it meant (D239).
        let asked = f.open_questions_asked();
        let (calls, modules) = called.get(&f.name).copied().unwrap_or((0, 0));
        for question in asked {
            queue.push(OpenQuestion {
                function: f.name.clone(),
                library: knowledge.library_of(&f.name).unwrap_or("?").to_owned(),
                question,
                calls,
                modules,
                returns: f.returns.map(|r| format!("{r:?}").to_lowercase()),
                arity: f.arity,
                known_by: f.known_by.map(|k| k.label().to_owned()),
            });
        }
    }
    // Most-called first; then by name so the order is total and a diff means something.
    queue.sort_by(|a, b| {
        b.calls
            .cmp(&a.calls)
            .then_with(|| a.function.cmp(&b.function))
            .then_with(|| a.question.cmp(&b.question))
    });
    if let Some(n) = top {
        queue.truncate(n);
    }

    if json {
        match serde_json::to_string_pretty(&queue) {
            Ok(text) => println!("{text}"),
            Err(e) => eprintln!("could not render the queue: {e}"),
        }
        return;
    }

    println!(
        "{} open questions, ranked by how often a guest calls the function",
        queue.len()
    );
    println!();
    let mut last = String::new();
    for q in &queue {
        if q.function != last {
            let shape = match (q.returns.as_deref(), q.arity) {
                (Some(r), Some(a)) => format!("returns {r}, {a} args"),
                (Some(r), None) => format!("returns {r}"),
                (None, Some(a)) => format!("{a} args"),
                (None, None) => "shape unrecorded".to_owned(),
            };
            println!(
                "  {:>10} calls in {} module(s)   {}::{}   [{shape}]",
                q.calls, q.modules, q.library, q.function
            );
            last.clone_from(&q.function);
        }
        println!("      ? {}", q.question);
    }
}

/// How often each function has been called, across every trace on disk.
///
/// Keyed by the bare function name so it joins against the knowledge base, which does not
/// know about libraries the way a trace label does.
fn calls_by_function() -> std::collections::BTreeMap<String, (u64, usize)> {
    let mut totals: std::collections::BTreeMap<String, (u64, usize)> =
        std::collections::BTreeMap::new();
    let paths = orbistoun_paths::Paths::resolve();
    let Ok(entries) = std::fs::read_dir(paths.traces_dir()) else {
        return totals;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(trace) = serde_json::from_str::<orbistoun_report::trace::CallTrace>(&text) else {
            continue;
        };
        for call in &trace.calls {
            // The label is `library::name`; the knowledge base keys on the name alone.
            let name = call.label.rsplit("::").next().unwrap_or(&call.label);
            let slot = totals.entry(name.to_owned()).or_insert((0, 0));
            slot.0 += call.calls;
            slot.1 += 1;
        }
    }
    totals
}

/// Ranks the calls a guest made to the kernel directly, which no import list can show.
///
/// # Why this is a section of its own
///
/// A guest reaching the kernel by number touches no stub, so it contributes nothing to the
/// ranked imports above. That is not an edge case: it is how every open-toolchain payload
/// works - resolve one function to build a gadget, then go straight to the kernel - so a run
/// that stopped dead on an unimplemented call could report nothing of interest and be telling
/// the truth (D401).
///
/// Ranked by **how many runs asked**, not by how often. The recorder is a bitmap and knows only
/// that a number came up; a call count would be a number nobody measured.
fn report_kernel_calls(
    kernel: &std::collections::BTreeMap<u64, (usize, Option<String>, Option<u64>)>,
) {
    let unserved: Vec<_> = kernel
        .iter()
        .filter(|(_, (_, name, _))| name.is_none())
        .collect();
    if unserved.is_empty() {
        return;
    }

    let mut ranked = unserved;
    ranked.sort_unstable_by_key(|(number, (runs, _, _))| (std::cmp::Reverse(*runs), **number));

    println!(
        "
{} system call(s) asked for directly that nothing here implements",
        ranked.len()
    );
    println!("{:>6}  {:>5}  FIRST ARGUMENT", "CALL", "RUNS");
    for (number, (runs, _, argument)) in &ranked {
        // The argument is shown because for a call nobody can name it is most of what there is
        // to go on - a number alone says which entry to write, and the argument starts to say
        // what it is for.
        let argument = argument.map_or_else(|| "-".to_owned(), |a| format!("{a:#x}"));
        println!("{number:>6}  {runs:>5}  {argument}");
    }
}

fn cmd_worklist(top: usize) {
    let paths = orbistoun_paths::Paths::resolve();
    let dir = paths.traces_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        println!("no traces yet at {}", dir.display());
        println!("run a guest first:  ./bin/orbistoun crunch");
        return;
    };

    // Totalled by label rather than by index. A stub index is per-module - index 260 is
    // a different function in every title - so summing by index would produce confident
    // nonsense the moment a second module was involved.
    let mut totals: std::collections::BTreeMap<String, (u64, usize)> =
        std::collections::BTreeMap::new();
    // Kept apart from the imports above, and counted differently on purpose. The recorder is a
    // bitmap: it knows a number was asked for, not how many times. Ranking these by call volume
    // would mean inventing the volume, so they rank by **how many runs wanted them** - which is
    // a fact, and is the right question anyway for something that blocks a payload outright.
    let mut kernel: std::collections::BTreeMap<u64, (usize, Option<String>, Option<u64>)> =
        std::collections::BTreeMap::new();
    let mut modules = 0;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(trace) = serde_json::from_str::<orbistoun_report::trace::CallTrace>(&text) else {
            eprintln!("note: {} is not a call trace, skipping", path.display());
            continue;
        };
        modules += 1;
        for call in &trace.calls {
            let entry = totals.entry(call.label.clone()).or_insert((0, 0));
            entry.0 = entry.0.saturating_add(call.calls);
            entry.1 += 1;
        }
        for asked in &trace.syscalls {
            let seen = kernel.entry(asked.number).or_insert((0_usize, None, None));
            seen.0 += 1;
            if seen.1.is_none() {
                seen.1.clone_from(&asked.name);
            }
            if seen.2.is_none() {
                seen.2 = asked.first_argument;
            }
        }
    }

    if totals.is_empty() && kernel.is_empty() {
        println!("no imports recorded across {modules} traces");
        return;
    }

    let mut ranked: Vec<(&String, u64, usize)> =
        totals.iter().map(|(l, (c, m))| (l, *c, *m)).collect();
    ranked.sort_unstable_by_key(|(label, calls, _)| (std::cmp::Reverse(*calls), (*label).clone()));

    let grand: u64 = ranked.iter().map(|(_, c, _)| *c).sum();
    println!(
        "{} distinct imports called across {modules} runs, {grand} calls total
",
        ranked.len()
    );
    println!("{:>14}  {:>5}  {:>7}  IMPORT", "CALLS", "SHARE", "MODULES");
    for (label, calls, in_modules) in ranked.iter().take(top) {
        let tenths = calls.saturating_mul(1000).checked_div(grand).unwrap_or(0);
        let share = format!("{}.{}", tenths / 10, tenths % 10);
        println!("{calls:>14}  {share:>4}%  {in_modules:>7}  {label}");
    }
    if ranked.len() > top {
        println!(
            "
... and {} more (--top to see further)",
            ranked.len() - top
        );
    }

    report_kernel_calls(&kernel);

    // The two questions this list answers are different, and worth separating.
    let unnamed = ranked.iter().filter(|(l, _, _)| l.contains("::0x")).count();
    println!(
        "
{} of {} still have no name - extend crates/orbistoun-names/data/vendor.toml",
        unnamed,
        ranked.len()
    );
}

/// Collects every `Symbol.map` beneath a directory.
///
/// Hand-rolled rather than pulling in a directory-walking crate: the whole job is
/// "recurse and match one filename", and a dependency that does it would be more code
/// to audit than the code it replaces.
fn find_symbol_maps(root: &std::path::Path, found: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        // An unreadable directory costs its own symbols, not the run. A harvest that
        // fails wholesale because one path is inaccessible is worse than a partial one
        // that says how many it read.
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            find_symbol_maps(&path, found);
        } else if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(orbistoun_names::harvest::is_version_script)
        {
            // **Asked rather than re-decided.** This tested the file name against
            // `"Symbol.map"` while `is_version_script` - written to fix exactly that, after
            // it cost every `pthread_*` name (D127) - sat unused in the crate below.
            //
            // Two implementations of one rule, one of them fixed. `lib/libthr` calls its
            // file `pthread.map` and `lib/libsys` calls its `Symbol.sys.map`, so this
            // walker silently skipped both and reported success either way (D191).
            found.push(path);
        }
    }
}

/// `harvest` - rebuild the standard-library word list from a FreeBSD source tree.
fn cmd_harvest(
    source: &std::path::Path,
    out: &std::path::Path,
    revision: Option<&str>,
) -> Result<()> {
    anyhow::ensure!(
        source.is_dir(),
        "{} is not a directory - point this at a FreeBSD source checkout",
        source.display()
    );

    let mut maps = Vec::new();
    for library in orbistoun_names::harvest::FREEBSD_LIBRARY_PATHS {
        let path = source.join(library);
        if path.is_dir() {
            find_symbol_maps(&path, &mut maps);
        } else {
            // Named rather than silently skipped: a sparse checkout missing one library
            // yields a smaller list, and the reader should know which.
            eprintln!("note: {} is not present, skipping", path.display());
        }
    }
    anyhow::ensure!(
        !maps.is_empty(),
        "no Symbol.map files found under {} - is this a FreeBSD source tree?",
        source.display()
    );
    maps.sort();

    let mut names = std::collections::BTreeSet::new();
    for map in &maps {
        let text =
            std::fs::read_to_string(map).with_context(|| format!("reading {}", map.display()))?;
        for symbol in orbistoun_names::harvest::parse_symbol_map(&text) {
            names.insert(symbol.name);
        }
    }

    // A revision is a citation; a local path is not. When both exist the revision leads
    // and the path is dropped entirely - nobody re-deriving this has the same directory,
    // and a temporary one actively misleads.
    let described = match revision {
        Some(revision) => revision.to_owned(),
        None => format!("{} (no revision given)", source.display()),
    };
    let names: Vec<String> = names.into_iter().collect();
    let text = orbistoun_names::harvest::render(&names, &described, &orbistoun_nid::today());
    std::fs::write(out, text).with_context(|| format!("writing {}", out.display()))?;

    println!(
        "harvested {} names from {} symbol maps into {}",
        names.len(),
        maps.len(),
        out.display()
    );
    if revision.is_none() {
        // The point of harvesting is citability, and a path with no revision is only
        // half a citation.
        eprintln!("note: pass --revision to record which revision this came from");
    }
    Ok(())
}

/// Re-derives generated records that the current grammar no longer confirms.
///
/// # The record shape that fights the loop it belongs to
///
/// A generated record is `pattern` plus `index`, and that is what makes checking it a
/// microsecond rather than a full sweep. It is also what makes it fragile: an index is a
/// position in a mixed-radix enumeration over the vocabularies, so **adding one word to a
/// vocabulary renumbers every candidate built from it**.
///
/// Which is a problem, because adding words is the loop. Every confirmed name is split
/// into parts and fed back into the grammar so the next search reaches further (D195) - and
/// each time that happens, some already-recorded name stops being at the index its record
/// names. Nothing was wrong with the name and nothing was wrong with the claim; the
/// coordinates moved underneath it.
///
/// Before this, those names fell off the verified count onto the unaccounted ceiling - a
/// file whose whole rule is that it may only shrink. One learning pass of thirty-seven
/// words moved twenty names onto it. Left alone the ceiling would have grown on every
/// successful search, which is the exact opposite of what it is for (D213).
///
/// **A repair knows the name, so it never searches.** `Pattern::index_of` inverts the
/// mixed-radix encoding that produced it, so new coordinates are arithmetic per name. This
/// used to hand the hashes to the generative sweep and hunt for names it already had - five
/// hours on thirty-three records, unfinished; the same work is now fourteen seconds (D304).
///
/// The date is never touched. A record says when a name was *first* worked out, and that
/// did not change; only where the current grammar produces it did.
fn repair_generated_records(
    file: &mut orbistoun_nid::SymbolDbFile,
    patterns: &[orbistoun_names::Pattern],
    standard: &[String],
) -> usize {
    let stale: Vec<String> = file
        .names
        .iter()
        .filter(|name| {
            file.derivations.get(*name).is_some_and(|d| {
                matches!(d.method, orbistoun_nid::Method::Generated { .. })
                    && !orbistoun_names::solve::verify(name, d, patterns, standard)
            })
        })
        .cloned()
        .collect();
    if stale.is_empty() {
        return 0;
    }

    println!();
    println!(
        concat!(
            "{} generated record(s) no longer hold - the grammar moved underneath them. ",
            "Re-deriving:"
        ),
        stale.len()
    );

    // **Derived from the name, not searched for by hash.** This ran `solve_patterns` - the
    // full generative sweep, hashing every candidate across a space measured in trillions,
    // looking for NIDs it already had names for. Five hours on thirty-three names, and it did
    // not finish.
    //
    // A repair knows the name. `Pattern::index_of` inverts the mixed-radix encoding that
    // produced it, so the new coordinates are arithmetic rather than a search (D304).
    //
    // It also removes a hazard the hash route had and had to guard against by hand: a target
    // set holds hashes, the first candidate hashing to one is not necessarily the name being
    // repaired, and rewriting a record from a collision would forge coordinates using the tool
    // meant to prevent forged records. Searching for the name cannot collide.
    let mut repaired = 0;
    for name in &stale {
        let Some(derivation) = orbistoun_names::solve::derive(name, patterns, standard) else {
            continue;
        };
        if let Some(existing) = file.derivations.get_mut(name) {
            existing.method = derivation.method;
            repaired += 1;
        }
    }
    let missed = stale.len() - repaired;
    if missed > 0 {
        // Not an error. A name the grammar genuinely cannot reach any more belongs on the
        // ceiling, which is what the ceiling is for - and saying so beats a silent partial
        // repair that leaves somebody wondering why the count still does not add up.
        println!("  {repaired} re-derived, {missed} the current grammar cannot produce at all");
    } else {
        println!("  {repaired} re-derived");
    }
    repaired
}

/// Re-runs every static harvest against the module its record names.
///
/// # Why this is the point of splitting `observed` in two
///
/// A static record says a name was read out of one named file, at rest. That is not a
/// claim anybody has to take on trust - the file either contains the string or it does
/// not, and re-reading it settles the question exactly as an array lookup settles a
/// generated one. The only difference is that the file is not in this repository, so CI
/// cannot do it and a person holding the title can (D213).
///
/// The old vocabulary made this impossible to even ask. `observed` covered both "read out
/// of a file" and "worked out by watching a guest run", and there is no single check that
/// applies to both, so neither got one.
///
/// Absent modules are counted and named, never passed. A check that reports success for
/// material it could not read is worse than no check.
fn verify_static_records(file: &orbistoun_nid::SymbolDbFile) -> Result<()> {
    use std::collections::BTreeMap;

    // Grouped by module so each is read and scanned once. A record-at-a-time loop would
    // re-scan a thirty-megabyte executable for every one of the hundred names it carries.
    let mut by_module: BTreeMap<&str, Vec<&String>> = BTreeMap::new();
    for name in &file.names {
        if let Some(orbistoun_nid::Method::Static { from, .. }) =
            file.derivations.get(name).map(|d| &d.method)
        {
            by_module.entry(from.as_str()).or_default().push(name);
        }
    }
    if by_module.is_empty() {
        return Ok(());
    }

    let mut checked = 0_usize;
    let mut unchecked = 0_usize;
    let mut absent: Vec<&str> = Vec::new();
    let mut failed: Vec<(&str, &str)> = Vec::new();

    for (module, names) in &by_module {
        let path = std::path::Path::new(module);
        let Ok(bytes) = std::fs::read(path) else {
            unchecked += names.len();
            absent.push(module);
            continue;
        };
        let candidates: std::collections::HashSet<String> =
            orbistoun_names::strings::candidates(&bytes)
                .into_iter()
                .collect();
        for name in names {
            if candidates.contains(name.as_str()) {
                checked += 1;
            } else {
                failed.push((module, name.as_str()));
            }
        }
    }

    println!();
    println!("{checked} static record(s) re-harvested from the module each one names");
    if unchecked > 0 {
        // `concat!` defeats implicit `{name}` capture, so both arguments go positional.
        println!(
            concat!(
                "  {} not checked - {} module(s) are not here, which is the normal state ",
                "of this repository and of CI:"
            ),
            unchecked,
            absent.len()
        );
        for module in &absent {
            println!("      {module}");
        }
    }
    if failed.is_empty() {
        return Ok(());
    }
    // A record naming a module that does not contain the string is a false provenance
    // claim, which is precisely what this whole mechanism exists to make impossible.
    println!();
    println!(
        "{} record(s) claim a string the named module does not contain:",
        failed.len()
    );
    for (module, name) in &failed {
        println!("  {name}  is not in  {module}");
    }
    anyhow::bail!("a static provenance record does not hold against its own module")
}

/// What a record says was done, in one line, for the audit's per-tier listing.
///
/// The subtype comes first because it is the closed vocabulary - it is what can be
/// counted and grepped - and the free text after it is the part only a person reads.
fn how_it_was_found(method: &orbistoun_nid::Method) -> String {
    use orbistoun_nid::{Method, RuntimeSource, StaticSource};
    match method {
        Method::Static { by, from } => {
            let by = match by {
                StaticSource::ModuleStrings => "module-strings",
                StaticSource::CrossModule => "cross-module",
            };
            format!("{by}  {from}")
        }
        Method::Runtime { by, how } => {
            let by = match by {
                RuntimeSource::CallTrace => "call-trace",
                RuntimeSource::ArgumentDump => "argument-dump",
                RuntimeSource::ProbeTranscript => "probe-transcript",
            };
            format!("{by}  {how}")
        }
        Method::Supplied { source } => source.clone(),
        Method::PublishedStandard { list } => list.clone(),
        Method::Generated { pattern, index } => format!("{pattern}[{index}]"),
    }
}

/// One line saying what kind of material the database was built out of.
///
/// **The headline the split was for.** The tier listing below answers "what would it take
/// to check this?", which is the audit's own question; this answers the one a person asks
/// first - how much of what we know came from running things, and how much from reading
/// them. Under the old vocabulary the answer was unobtainable, because the value that
/// would have carried it covered both (D213).
///
/// Zero classes are printed too. "0 external" is the most reassuring number in the line
/// and would be the most conspicuous one to omit.
fn print_by_evidence(file: &orbistoun_nid::SymbolDbFile) {
    use orbistoun_nid::Evidence;

    let mut counts = [0_usize; 4];
    for derivation in file.derivations.values() {
        let slot = match derivation.method.evidence() {
            Evidence::Derived => 0,
            Evidence::Static => 1,
            Evidence::Runtime => 2,
            Evidence::External => 3,
        };
        counts[slot] += 1;
    }
    let unrecorded = file.names.len().saturating_sub(file.derivations.len());
    let classes = [
        Evidence::Derived,
        Evidence::Static,
        Evidence::Runtime,
        Evidence::External,
    ];
    let line: Vec<String> = classes
        .iter()
        .zip(counts)
        .map(|(class, n)| format!("{n} {}", class.label()))
        .collect();
    if unrecorded > 0 {
        println!(
            "  by evidence: {} ({unrecorded} with no record)",
            line.join(", ")
        );
    } else {
        println!("  by evidence: {}", line.join(", "));
    }
}

/// Names this project worked out but this repository cannot re-derive on its own, grouped
/// by what somebody else would need in order to arrive at them.
///
/// **This is the half of the audit that used to be a shrug.** One bucket labelled
/// "documented, not verified" held a string read deterministically out of a file next to a
/// conclusion a person drew from a trace, and it described both as unverifiable. Neither
/// is: one needs the module, one needs a run of it, and saying which is both truer and a
/// stronger claim than declining to classify them (D213).
fn print_by_tier(entries: &[(&String, &orbistoun_nid::Derivation)]) {
    use orbistoun_nid::Reproducible;

    // Tier order is the enum's own order, cheapest to check first, so the listing cannot
    // disagree with the type about which claim is stronger.
    for tier in [
        Reproducible::FromModule,
        Reproducible::FromRun,
        Reproducible::FromHardware,
    ] {
        let in_tier: Vec<_> = entries
            .iter()
            .filter(|(_, d)| d.method.reproducible() == tier)
            .collect();
        if in_tier.is_empty() {
            continue;
        }
        println!();
        println!(
            "{} reproducible {} - ours, but not from this repository alone:",
            in_tier.len(),
            tier.label()
        );
        for (name, d) in in_tier {
            println!("  {name}  [{}]  {}", d.on, how_it_was_found(&d.method));
            if let Some(note) = &d.note {
                println!("      {note}");
            }
        }
    }
}

/// `audit` - re-derive every name in a database from this repository's own inputs.
fn cmd_audit(
    database: &std::path::Path,
    grammar: Option<&std::path::Path>,
    ceiling: Option<&std::path::Path>,
    deep: bool,
    verify_harvest: bool,
    repair: bool,
) -> Result<()> {
    let text = std::fs::read_to_string(database)
        .with_context(|| format!("reading {}", database.display()))?;
    let mut file = orbistoun_nid::SymbolDbFile::from_json(&text).context("parsing the database")?;

    let grammar = match grammar {
        Some(path) => {
            let text = std::fs::read_to_string(path)
                .with_context(|| format!("reading {}", path.display()))?;
            orbistoun_names::Grammar::parse(&text)?
        }
        None => orbistoun_names::Grammar::builtin()?,
    };
    let patterns = grammar.patterns()?;
    let standard = orbistoun_names::standard_names();

    // Before anything is classified, so a repaired record is reported as what it now is
    // rather than as a failure that was quietly fixed on the way past.
    if repair {
        // No thread count: a repair is arithmetic per name now rather than a sweep, so there
        // is nothing to spread across cores (D304). `--threads` still governs `--deep`, which
        // genuinely searches because it has no pattern to invert against.
        if repair_generated_records(&mut file, &patterns, &standard) > 0 {
            let text =
                serde_json::to_string_pretty(&file).context("serialising the symbol database")?;
            std::fs::write(database, text)
                .with_context(|| format!("writing {}", database.display()))?;
        }
    }
    let file = file;

    let mut verified = 0_usize;
    // Everything that is ours but needs something this repository does not hold, kept in
    // tier order so the report reads from cheapest to check to most expensive. Sorting
    // into one "documented, not verified" bucket was the thing that let a static harvest
    // and a runtime observation look like the same claim (D213).
    let mut elsewhere: Vec<(&String, &orbistoun_nid::Derivation)> = Vec::new();
    let mut imported: Vec<(&String, &orbistoun_nid::Derivation)> = Vec::new();
    let mut unaccounted: Vec<&String> = Vec::new();

    for name in &file.names {
        match file.derivations.get(name) {
            Some(d) if orbistoun_names::solve::verify(name, d, &patterns, &standard) => {
                verified += 1;
            }
            // Recorded, but not from material this repository holds. Split by whether
            // this project worked it out or took it from elsewhere - the whole question
            // this file exists to answer.
            Some(d) if !d.method.is_mechanically_checkable() => {
                if d.method.is_our_own_work() {
                    elsewhere.push((name, d));
                } else {
                    imported.push((name, d));
                }
            }
            // A record that claims to be checkable and is not, or no record at all.
            // `--deep` gives the second kind a chance before giving up on it.
            _ if deep && orbistoun_names::solve::derive(name, &patterns, &standard).is_some() => {
                verified += 1;
            }
            _ => unaccounted.push(name),
        }
    }

    let total = file.names.len();
    println!("{verified} of {total} names re-derived from this repository");
    print_by_evidence(&file);
    if !deep && !unaccounted.is_empty() {
        println!("  (pass --deep to search the whole space for names with no record)");
    }

    if !elsewhere.is_empty() {
        print_by_tier(&elsewhere);
    }

    // Before the unaccounted list, because a false static record is a different and worse
    // finding than an unaccounted name, and it should not be read after two hundred lines
    // of known ceiling.
    if verify_harvest {
        verify_static_records(&file)?;
    }

    if !imported.is_empty() {
        // Listed loudly and separately. Not an error - taking a name from a public
        // database is lawful and sometimes sensible - but it is the one category that
        // changes the answer to "did you derive all of this yourselves?", so it must
        // never be quiet.
        println!();
        println!("{} came from outside this project:", imported.len());
        for (name, d) in &imported {
            let source = match &d.method {
                orbistoun_nid::Method::Supplied { source } => source.as_str(),
                _ => "",
            };
            println!("  {name}  [{}]  {source}", d.on);
        }
    }

    if unaccounted.is_empty() {
        println!(
            "
every name is accounted for"
        );
        // **The ceiling is still checked.** Returning here skipped it whenever nothing was
        // unaccounted, which is exactly the state in which the ceiling has gone stale - so
        // the half of its rule that says "an entry that stopped applying must leave" was
        // unenforceable in the only case that could trigger it. A guard that passes because
        // it stopped looking is worse than no guard (D199).
        if let Some(path) = ceiling {
            return against_ceiling(path, &unaccounted);
        }
        return Ok(());
    }
    let plural = if unaccounted.len() == 1 {
        "name"
    } else {
        "names"
    };
    println!(
        "
{} {plural} this repository cannot account for:",
        unaccounted.len()
    );
    for name in &unaccounted {
        println!("  {name}");
    }

    if let Some(path) = ceiling {
        return against_ceiling(path, &unaccounted);
    }

    // A non-zero status so this can gate a commit. An unaccounted name is not
    // necessarily wrong - a vocabulary shrinks, a name is added by hand - but it is
    // always something a person should have decided about deliberately, and recorded.
    anyhow::bail!("{} {plural} unaccounted for", unaccounted.len())
}

/// Judges the unaccounted set against a written-down ceiling.
///
/// Two failure directions, both load-bearing. A name unaccounted and **unlisted** is the
/// thing worth stopping for - somebody added a name nobody can explain. A name listed that
/// is **no longer** unaccounted has to leave the file, or the ceiling stops describing
/// anything and becomes a list nobody prunes (D208).
fn against_ceiling(path: &std::path::Path, unaccounted: &[&String]) -> Result<()> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading the ceiling at {}", path.display()))?;
    let listed: std::collections::BTreeSet<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    let now: std::collections::BTreeSet<&str> = unaccounted.iter().map(|n| n.as_str()).collect();

    let added: Vec<&&str> = now.difference(&listed).collect();
    let retired: Vec<&&str> = listed.difference(&now).collect();

    println!();
    if !added.is_empty() {
        println!("{} of those are NOT on the ceiling:", added.len());
        for name in &added {
            println!("  {name}");
        }
        // **Try `--repair` before reaching for the ceiling.** Most arrivals here are not
        // names the grammar cannot spell; they are names whose recorded index a learned
        // word renumbered, and the ceiling reached 202 entries before anybody checked
        // which (D213). Adding one without trying the repair records a fact that is not
        // true, in a file that is supposed to be evidence.
        println!("  most of these are records the grammar moved underneath, not names it");
        println!("  cannot produce. Try `audit --repair` first; it re-derives them in one");
        println!("  sweep. What survives that is a real gap - add it to");
        println!("  {} with a reason.", path.display());
    }
    if !retired.is_empty() {
        println!(
            "{} on the ceiling are now accounted for and must be removed from {}:",
            retired.len(),
            path.display()
        );
        for name in &retired {
            println!("  {name}");
        }
    }
    if added.is_empty() && retired.is_empty() {
        println!(
            "unaccounted set matches the ceiling exactly ({} names, and it may only shrink)",
            listed.len()
        );
        return Ok(());
    }
    anyhow::bail!("the unaccounted set does not match {}", path.display())
}

/// Enters worker mode: host the crates, speak the protocol over stdio, hold no logic.
///
/// Not a clap subcommand on purpose. It is an implementation detail of how the shims
/// execute guests, not a user-facing verb, and putting it in `--help` would invite
/// people to drive it by hand.
fn run_as_worker() -> Result<()> {
    orbistoun_worker::serve_as_worker_process().map_err(|e| anyhow::anyhow!(e))
}

/// Runs whichever command was asked for.
///
/// Split from `main` so the two jobs stay separate: `main` decides how the process is
/// configured, this decides what it does. A single function doing both grows a branch
/// every time a verb is added and is the first thing to become unreadable.
// One match arm per verb, so it grows past the line limit by design - the same reason the
// probe's responder allows it. Each arm is a one-liner to a `cmd_*`; the length is the count
// of verbs, not complexity in any one of them.
#[allow(clippy::too_many_lines)]
fn dispatch(cli: Cli, service: &Service) -> Result<()> {
    match cli.command {
        Command::Symbols { filter } => cmd_symbols(service, filter.as_deref()),
        Command::Policy => println!("{}", service.default_policy_toml()?),
        Command::Inspect { path } => cmd_inspect(service, &path)?,
        Command::Imports { path } => cmd_imports(service, &path)?,
        Command::Ask { .. } | Command::Session { .. } => dispatch_probe(cli.command)?,
        Command::Probe {
            path,
            device,
            firmware,
            is_target,
            as_knowledge,
        } => cmd_probe(&path, device, firmware, is_target, as_knowledge)?,
        Command::Shaders { path, top } => cmd_shaders(&path, top)?,
        Command::Serve { bind, no_key, once } => cmd_serve(service, &bind, no_key, once)?,
        Command::Verify { path } => cmd_verify(service, &path)?,
        Command::Report { path } => cmd_report(service, &path)?,
        Command::Load { path, base } => cmd_load(service, &path, base)?,
        Command::Turn {
            ref path,
            record,
            apply,
            ref verify,
        } => {
            cmd_turn(path, record, apply, verify.as_deref())?;
        }
        Command::Run {
            path,
            limit,
            calls,
            profile,
        } => cmd_run(
            &path,
            limit,
            calls,
            profile.as_deref(),
            cli.symbols_db.as_deref(),
        )?,
        Command::Handoff {
            ref path,
            fields,
            limit,
        } => cmd_handoff(path, fields, limit)?,
        Command::Names {
            ref path,
            threads,
            ref grammar,
            ref words,
            words_from,
            ref out,
            ref wanted,
            from_trace,
        } => cmd_names(
            service,
            &cli,
            &NameSearch {
                path,
                threads,
                grammar: grammar.as_deref(),
                words: words.as_deref(),
                words_from,
                out: out.as_deref(),
                wanted: wanted.as_deref(),
                from_trace,
            },
        )?,
        Command::Learn(ref learned) => cmd_learn(learned)?,
        Command::Compat { ref action } => match action {
            CompatAction::List { dir } => cmd_compat_list(dir)?,
            CompatAction::Markdown { dir, out, shots } => cmd_compat_markdown(dir, out, shots)?,
            CompatAction::Record {
                path,
                dir,
                note,
                force,
            } => cmd_compat_record(path, dir, note.as_deref(), *force)?,
        },
        Command::Corpus { ref action } => match action {
            CorpusAction::List { manifest } => cmd_corpus_list(manifest)?,
            CorpusAction::Sync {
                source,
                manifest,
                titles,
            } => cmd_corpus_sync(manifest, titles, source.as_deref())?,
            CorpusAction::Run {
                source,
                manifest,
                titles,
                limit,
                calls,
                profile,
            } => cmd_corpus_run(
                manifest,
                titles,
                source.as_deref(),
                *limit,
                *calls,
                profile.as_deref(),
            )?,
        },
        Command::Submit { ref action } => dispatch_submit(action)?,
        Command::Knows { ref pattern } => cmd_knows(pattern.as_deref()),
        Command::Status { write, check } => cmd_status(service, write, check)?,
        Command::Paths => cmd_paths(),
        Command::Env => cmd_env(),
        Command::Firmware { all } => cmd_firmware_layout(service, all),
        Command::Questions { top, json } => cmd_questions(top, json),
        Command::Worklist { top } => cmd_worklist(top),
        Command::Harvest {
            ref source,
            ref out,
            ref revision,
        } => cmd_harvest(source, out, revision.as_deref())?,
        Command::Audit {
            ref database,
            ref grammar,
            ref ceiling,
            deep,
            verify_harvest,
            repair,
        } => cmd_audit(
            database,
            grammar.as_deref(),
            ceiling.as_deref(),
            deep,
            verify_harvest,
            repair,
        )?,
    }

    Ok(())
}

fn main() -> Result<()> {
    // Was five lines of `tracing_subscriber` here, reading `RUST_LOG` only. The shared version
    // answers `OOPS_LOG` as well, can write a file and can export, and - the reason it is worth
    // replacing rather than leaving - the other two binaries in this workspace had no logging
    // at all, because each would have had to repeat those five lines to get any.
    //
    // The guard is held for the whole of `main`; `let _` would drop it here.
    let _logging = oops_log::Logging::new("orbistoun")
        .build(orbistoun_env::build::line_static())
        .init();

    // Checked before clap sees the arguments: worker mode is not a user-facing verb.
    if std::env::args().any(|a| a == orbistoun_worker::WORKER_FLAG) {
        return run_as_worker();
    }

    let cli = Cli::parse();
    let symbol_db = match cli.symbols_db.as_ref() {
        // **Loaded unless told otherwise.** It used to be `None`, so every run reported
        // hashes the shipped database could already name and then told the reader to go
        // and extend the vocabulary - work already done, in a file already committed, that
        // nothing loaded. The findings are what this project is for, and they were
        // confidently recommending the wrong next action (D188).
        None => Some(orbistoun_service::SymbolDbFile::builtin()),
        Some(path) => {
            let text = std::fs::read_to_string(path)
                .with_context(|| format!("reading symbol database {}", path.display()))?;
            Some(
                orbistoun_service::SymbolDbFile::from_json(&text)
                    .with_context(|| format!("parsing symbol database {}", path.display()))?,
            )
        }
    };

    // Portable-first resolution decides where reports land, and purging old artifacts
    // on startup is what keeps a long agent run from filling a disk (D047).
    let paths = orbistoun_paths::Paths::resolve();
    paths.ensure_dirs().ok();
    if let Ok(purged) = orbistoun_report::retention::purge(
        &paths.reports_dir(),
        orbistoun_report::retention::Policy::default(),
        std::time::SystemTime::now(),
    ) {
        if purged.removed > 0 {
            tracing::info!(
                removed = purged.removed,
                bytes = purged.bytes_freed,
                "purged expired run artifacts"
            );
        }
    }

    let service = Service::new(ServiceConfig {
        nid_suffix: suffix_for(&cli)?,
        symbol_db,
        paths: Some(paths),
        ..ServiceConfig::default()
    });

    if let Some(known) = service.symbol_db_len() {
        tracing::info!(names = known, "symbol database loaded");
    }

    if !service.nids_are_real() {
        tracing::warn!(
            reason = "no --suffix-hex given",
            "symbol names are correct; NIDs shown are not real import hashes"
        );
    }

    dispatch(cli, &service)
}

/// The commands that talk to a live probe.
///
/// Split out because `dispatch` outgrew its line limit, and this is the natural seam: every
/// other command reads a file or a title, and these two open a socket. Keeping them
/// together makes the one part of this tool that touches hardware findable.
fn dispatch_probe(command: Command) -> Result<()> {
    match command {
        Command::Ask {
            address,
            key,
            command,
            budget,
            as_knowledge,
            device,
            is_target,
        } => {
            let origin = match device {
                Some(device) => {
                    let target = is_target && !orbistoun_probe::Origin::is_known_stand_in(&device);
                    orbistoun_probe::Origin::asserted(device, "", target)
                }
                None => orbistoun_probe::Origin::unasserted(),
            };
            cmd_ask(
                &address,
                key.as_deref(),
                &command,
                budget,
                as_knowledge,
                &origin,
            )
        }
        Command::Session {
            address,
            key,
            out,
            device,
            firmware,
            is_target,
            budget,
        } => cmd_session(
            &address,
            key.as_deref(),
            &out,
            device.as_deref(),
            firmware.as_deref(),
            is_target,
            budget,
        ),
        _ => unreachable!("dispatch_probe is only reached for probe commands"),
    }
}

/// Asks a probe one question.
///
/// # Why the answer is printed rather than interpreted
///
/// This is the rawest surface onto the protocol and it stays that way deliberately. It
/// prints what came back and does not decide what it means - no grading, no knowledge
/// entry, no judgement about whether the value is usable. Those are decisions with rules
/// attached, and a command whose whole job is "ask the console" should not quietly make
/// them.
///
/// What it will not do is flatter a non-answer. `died`, `timeout` and `lost` print as
/// themselves and carry no value, because the probe dying is the normal case here and a
/// tool that rendered it as a result would be the one thing this whole effort is against.
fn cmd_ask(
    address: &str,
    key: Option<&str>,
    command: &[String],
    budget: u64,
    as_knowledge: bool,
    origin: &orbistoun_probe::Origin,
) -> Result<()> {
    let address = if address.contains(':') {
        address.to_owned()
    } else {
        format!("{address}:{}", orbistoun_probe::client::DEFAULT_PORT)
    };
    let budget = std::time::Duration::from_secs(budget);

    let mut client = orbistoun_probe::client::connect(&address, budget)
        .with_context(|| format!("connecting to {address}"))?;
    let session = client
        .hello(orbistoun_probe::VERSION, key)
        .map_err(|e| anyhow::anyhow!("negotiating with {address}: {e}"))?;

    let (verb, arguments) = command.split_first().expect("clap requires one");
    let borrowed: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let answer = client.command(verb, &borrowed);
    let _ = client.bye();

    // Rendering the answer as knowledge is the whole point of asking: a value that stays in
    // a terminal has to be asked for again tomorrow. Only a `call` produces one - `read` and
    // `report` establish other things, and pretending otherwise would file a byte count as a
    // function's return value.
    if as_knowledge {
        return render_asked(&answer, verb, arguments, origin);
    }

    match answer {
        Ok(answer) => {
            // Records that arrived before the answer - `bytes` from a read, or a report's
            // stream - are shown, because they are frequently the point of the question.
            for record in &answer.records {
                println!("{record:?}");
            }
            println!("{}", answer.outcome);
            if !answer.detail.is_empty() {
                println!("  {}", answer.detail);
            }
            // A memory read is worth rendering as memory rather than as records.
            let transcript = orbistoun_probe::Transcript::read(&client.transcript().join("\n"))
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            let memory = transcript.memory();
            if !memory.bytes.is_empty() {
                println!("\n{} byte(s):", memory.bytes.len());
                for (offset, chunk) in memory.bytes.chunks(16).enumerate() {
                    let hex: Vec<String> = chunk.iter().map(|byte| format!("{byte:02x}")).collect();
                    println!("  {:04x}  {}", offset * 16, hex.join(" "));
                }
            }
            if memory.undecodable > 0 {
                println!(
                    "  {} run(s) could not be decoded and were not guessed at",
                    memory.undecodable
                );
            }
        }
        Err(e) => println!("refused: {e}"),
    }
    println!("\nsession {session}");
    Ok(())
}

/// Renders a live answer as the knowledge entry it would become.
///
/// # Why only a `call`
///
/// The rule is about what a *function returns*. A `read` establishes what some memory holds
/// and a `report` establishes a suite's results; filing either as a return value would put a
/// byte count where a function's answer belongs. So anything else says so rather than
/// producing a plausible entry.
fn render_asked(
    answer: &std::result::Result<
        orbistoun_probe::client::Answer,
        orbistoun_probe::client::ClientError,
    >,
    verb: &str,
    arguments: &[String],
    origin: &orbistoun_probe::Origin,
) -> Result<()> {
    if verb != "call" {
        println!("`{verb}` establishes something, but not what a function returns");
        println!("  only `call` produces a knowledge entry");
        return Ok(());
    }
    let Ok(answer) = answer else {
        println!("nothing to record: the command was refused before it ran");
        return Ok(());
    };

    let parsed: Vec<u64> = arguments
        .iter()
        .filter_map(|argument| {
            let body = argument.strip_prefix("0x").unwrap_or(argument);
            u64::from_str_radix(body, 16).ok()
        })
        .collect();
    let symbol = parsed
        .first()
        .map_or_else(|| "<unknown>".to_owned(), |address| format!("{address:#x}"));

    // The return kind decides whether the guest may use it, and nothing here knows it: this
    // command was given an address, not a name. So it records only, which is the safe
    // reading - not knowing what a function returns is exactly when handing its value over
    // is most dangerous.
    let asked = orbistoun_probe::Asked {
        symbol,
        arguments: parsed.iter().skip(1).copied().collect(),
        outcome: answer.outcome.clone(),
        usable: orbistoun_probe::usable(None),
    };

    let entry = asked.knowledge(origin);
    let file = orbistoun_hle::knowledge::KnowledgeFile {
        library: "<unknown - asked by address>".to_owned(),
        functions: vec![entry],
    };
    print!("{}", file.render().context("rendering knowledge")?);
    println!("# recorded only: this was asked by address, so the return kind is unknown");
    Ok(())
}

/// Drives a live session and writes the transcript out.
///
/// # Why the file is the point
///
/// The session is the interface; the corpus is the product. A run that answers questions
/// and leaves nothing on disk has produced nothing, and everything downstream - grading,
/// findings, knowledge entries - reads files rather than sockets. So this connects, runs
/// the probe's suite, and writes what it saw.
///
/// The operator's assertion about the machine is written into the file as a comment,
/// because a transcript that has to be joined against a memory of who ran it is a
/// transcript nobody can grade later.
fn cmd_session(
    address: &str,
    key: Option<&str>,
    out: &std::path::Path,
    device: Option<&str>,
    firmware: Option<&str>,
    is_target: bool,
    budget: u64,
) -> Result<()> {
    use std::io::Write as _;

    let address = if address.contains(':') {
        address.to_owned()
    } else {
        format!("{address}:{}", orbistoun_probe::client::DEFAULT_PORT)
    };
    let budget = std::time::Duration::from_secs(budget);

    let mut client = orbistoun_probe::client::connect(&address, budget)
        .with_context(|| format!("connecting to {address}"))?;
    let session = client
        .hello(orbistoun_probe::VERSION, key)
        .map_err(|e| anyhow::anyhow!("negotiating with {address}: {e}"))?;
    println!("session {session}");

    // Only what it announced. Sending a reserved verb and waiting to be refused puts a
    // command on the wire that this probe does not implement, and on a target that faults
    // easily that is not free.
    if client.can(&orbistoun_probe::Capability::Report) {
        match client.report() {
            Ok(answer) => println!("report {}", answer.outcome),
            // A command that did not answer is not an error in the client - it is the
            // finding. It is recorded and the session continues to a clean close.
            Err(e) => println!("report failed: {e}"),
        }
    } else {
        println!("report not announced by this probe");
    }
    let _ = client.bye();

    let mut file =
        std::fs::File::create(out).with_context(|| format!("creating {}", out.display()))?;
    writeln!(file, "# session {session} against {address}")?;
    writeln!(
        file,
        "# operator asserts: device={} firmware={} is-target={}",
        device.unwrap_or("unasserted"),
        firmware.unwrap_or(""),
        is_target
    )?;
    for line in client.transcript() {
        writeln!(file, "{line}")?;
    }
    println!("written {}", out.display());
    println!(
        "\nread it back with: orbistoun probe {} {}",
        out.display(),
        if is_target {
            "--device <name> --is-target"
        } else {
            "--device <name>"
        }
    );
    Ok(())
}

/// Reads a probe transcript and reports what it establishes.
///
/// Deliberately read-only and deliberately file-based. A gate that needs a console plugged
/// in is a gate that fails for everyone else, so the corpus is the interface and the socket
/// is somebody else's problem (D207).
fn cmd_probe(
    path: &std::path::Path,
    device: Option<String>,
    firmware: Option<String>,
    is_target: bool,
    as_knowledge: bool,
) -> Result<()> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let transcript = orbistoun_probe::Transcript::read(&text)
        .map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;

    // The operator's assertion, or the absence of one. Nothing here reads the machine
    // identity off the records: a probe running inside an emulator reports that emulator's
    // version as the platform's, so `target|console` on the wire is a claim somebody typed
    // and not evidence of anything.
    let origin = if let Some(device) = device {
        {
            // One question, not two. A name this project knows to be a stand-in stays one
            // without the operator saying so twice; anything else is taken at its word only
            // when `--is-target` says so.
            //
            // The list is of stand-ins rather than targets on purpose: an unrecognised name
            // defaults to *not the target*, so an emulator nobody has listed is demoted
            // rather than promoted. A wrong demotion is recoverable; the other direction is
            // the one that corrupts a knowledge base.
            let target = is_target && !orbistoun_probe::Origin::is_known_stand_in(&device);
            if is_target && !target {
                println!("note      `{device}` is a known stand-in, so --is-target was ignored");
            }
            orbistoun_probe::Origin::asserted(device, firmware.unwrap_or_default(), target)
        }
    } else {
        {
            // Nothing typed, nothing claimed. This is the safe default and the common case:
            // the session is recorded in full and every result grades as an assumption,
            // which is exactly what "nobody said what this ran on" means.
            if is_target {
                println!("note      --is-target names nothing without --device, so it was ignored");
            }
            orbistoun_probe::Origin::unasserted()
        }
    };
    let origin = transcript.sessions.first().map_or_else(
        || origin.clone(),
        |session| origin.clone().with_claims(session),
    );

    print_origin(&transcript, &origin);
    print!("{}", transcript.established(&origin));

    if as_knowledge {
        return print_knowledge(&transcript, &origin);
    }

    // Facts first, and named. A count says how much was learned; this says what, and a
    // function with a measured return value is the only form this project can act on.
    let findings = transcript.findings(&origin);
    let facts: Vec<_> = findings.iter().filter(|f| f.is_fact()).collect();
    if !facts.is_empty() {
        println!("\nestablished");
        for finding in facts {
            let grade = finding.known_by.map_or("?", |oracle| oracle.label());
            println!(
                "  {:<9} {}::{} = {} [{grade}]",
                format!("{:?}", finding.status).to_lowercase(),
                finding.library,
                finding.symbol,
                if finding.value.is_empty() {
                    "(no value)"
                } else {
                    &finding.value
                }
            );
        }
    }

    print_self_report(&transcript);
    print_sections(&transcript);

    // Symbols, separately from results, but graded the same way and by the same origin.
    // They used to be printed ungraded on the reasoning that a name resolving on a stand-in
    // is still spelled correctly - which is the stand-in's mined name list speaking, not the
    // platform (D246).
    let symbols = transcript.symbols(&origin);
    if !symbols.is_empty() {
        let absent = symbols.iter().filter(|s| !s.present).count();
        println!(
            "\nsymbols {} resolved, {absent} absent",
            symbols.len() - absent
        );
        for symbol in &symbols {
            // Whichever the record carried, named for what it is. A `sym` record says how
            // the symbol is reached and a `resolve` record says where it landed; printing
            // either under one unlabelled bracket would make them look like one field with
            // inconsistent contents (D245).
            let detail = match (&symbol.availability, &symbol.address) {
                (Some(how), _) => format!("via {how}"),
                (None, Some(at)) => format!("at {at}"),
                (None, None) => "no detail recorded".to_owned(),
            };
            // Said on the line, not inferred from the header. A fact that may source a
            // name and one that may not look identical otherwise, and the whole naming
            // rule turns on the difference (D242, D246).
            let source = if symbol.may_source_a_name() {
                ""
            } else {
                "  - not a naming source"
            };
            println!(
                "  {:<8} {}::{} ({detail}){source}",
                if symbol.present { "present" } else { "ABSENT" },
                symbol.library,
                symbol.symbol,
            );
        }
    }

    // A call that was announced and never concluded. Listed separately and never counted
    // as a failure: the probe said what it was about to do and did not come back, so
    // nothing was concluded about it. Reporting that as a failing check would be recording
    // an outcome nobody observed.
    let unfinished = transcript.attempted_without_result();
    if !unfinished.is_empty() {
        println!("\nannounced and never concluded - each one ended the probe");
        for (check, library, symbol) in unfinished {
            println!("  {library}::{symbol}  ({check})");
        }
    }
    Ok(())
}

/// Prints what the target says about itself, marked as self-reported.
///
/// # Why the state is shown and not just the value
///
/// All three states can read `unknown` and they are three different findings: the platform
/// has no such query, the probe has not wired one up yet, or here is a real number. A
/// display that collapsed them would show one blank where there are three, and only one of
/// them is anybody's bug.
///
/// Marked as the target's own account throughout. Inside an emulator every field answers as
/// that emulator chooses, so none of this is machine identity - that is asserted by the
/// operator and appears above, separately and labelled.
fn print_self_report(transcript: &orbistoun_probe::Transcript) {
    use orbistoun_probe::Confidence;

    let report = transcript.self_report();
    if report.is_empty() {
        return;
    }
    println!(
        "
self-reported by the target (not evidence of what it is)"
    );
    for field in &report {
        let state = match &field.confidence {
            Confidence::Known => "",
            Confidence::Unconfirmed => "  [the probe cannot read this yet]",
            Confidence::Absent => "  [this platform has no such query]",
            Confidence::Unrecognised(other) => &format!("  [state {other:?} - unrecognised]"),
        };
        // `generation` carries two readings a display can get wrong, so both are named
        // here rather than left in a document the reader does not have open.
        //
        // `both` is a positive observation - two driver stacks present - and it
        // deliberately names no console, because presence is not implementation.
        //
        // The parenthetical is **evidence, not recency**: `agc` and `gnm` are the graphics
        // drivers the inference keyed on. It used to read `(current)` / `(previous)`, which
        // stops being true the day a sixth generation ships and cannot be corrected in an
        // archived report (obSCEne D147). Nothing here parsed those words - the value is
        // rendered verbatim - so the change needed no code; the note is so a reader does
        // not take a driver name for a version.
        let note = if field.field == "generation" {
            match field.value.as_str() {
                "both" => "  [two driver stacks present; this names no console]",
                v if v.contains("(agc)") || v.contains("(gnm)") => {
                    "  [the parenthetical is the driver this was inferred from, not a version]"
                }
                _ => state,
            }
        } else {
            state
        };
        println!("  {:<11} {}{note}", field.field, field.value);
    }
}

/// Prints each area of the platform and how much of it came out green.
///
/// A single total says how much was checked and nothing about what is *understood*. The
/// same count spread thinly across every area and concentrated in one are completely
/// different situations, and only the second means a subsystem can be relied on.
fn print_sections(transcript: &orbistoun_probe::Transcript) {
    let sections = transcript.sections();
    if sections.is_empty() {
        return;
    }
    let green = sections
        .iter()
        .filter(|section| section.is_wholly_green())
        .count();
    println!("\nareas {green} of {} wholly green", sections.len());
    for section in &sections {
        // A skip is shown rather than folded into the total, because it is a check that
        // did not run - the section did not establish what it claims to, and rounding a
        // skip up is how a subsystem gets relied on for something nobody tested.
        let counts = [
            ("pass", section.pass),
            ("partial", section.partial),
            ("fail", section.fail),
            ("skip", section.skip),
        ]
        .into_iter()
        // `pass` is shown even at zero: a section reporting no passes is the interesting
        // case, and omitting the number would leave it looking like a section with no
        // checks rather than one where nothing worked.
        .filter(|(label, count)| *count > 0 || *label == "pass")
        .map(|(label, count)| format!("{count} {label}"))
        .collect::<Vec<_>>()
        .join(", ");
        let mark = if section.is_wholly_green() { "+" } else { " " };
        println!(
            "  {mark} {:<20} {:<34} {counts}",
            section.id,
            if section.title.is_empty() {
                "(no section record)"
            } else {
                &section.title
            }
        );
    }
}

/// Prints what produced the answers, before any of the answers.
///
/// First and not as a footnote. A number read without knowing which machine produced it is
/// the failure this project has already paid for once.
fn print_origin(transcript: &orbistoun_probe::Transcript, origin: &orbistoun_probe::Origin) {
    println!("machine {} (operator-asserted)", origin.describe());
    if origin.is_target {
        println!("          asserted as the target platform - results may grade as measured");
    } else {
        println!("          not asserted as the real target - nothing grades above `assumed`");
    }
    if let Some((build, kind)) = transcript.build() {
        println!("build {build} ({kind})");
    }
    println!();

    for session in &transcript.sessions {
        println!("session {}", session.session);
        // What produced the answers, printed first and not as a footnote. A number read
        // without knowing which device produced it is the failure this project has already
        // paid for once.
        for (key, value) in &session.parts {
            println!("  {key:<9} {value}");
        }
        let mut capabilities: Vec<String> = session
            .capabilities
            .iter()
            .map(|capability| format!("{capability:?}").to_lowercase())
            .collect();
        capabilities.sort();
        println!("  can {}", capabilities.join(", "));
        // What the session claimed, printed as a claim. It is worth seeing next to the
        // operator's assertion precisely when the two disagree - an emulator announcing
        // `console` under an operator who said otherwise is the case this whole
        // distinction exists for.
        if let Some(claimed) = session.claimed_target() {
            println!("  claimed {claimed} (the probe's own word, not evidence)");
        }
        println!();
    }
}

/// Renders what was established as knowledge entries.
fn print_knowledge(
    transcript: &orbistoun_probe::Transcript,
    origin: &orbistoun_probe::Origin,
) -> Result<()> {
    let findings = transcript.findings(origin);
    // Grouped by library, because that is how the knowledge base is filed, and
    // rendered through its own serialiser so what is printed is what would be written.
    let mut by_library: std::collections::BTreeMap<
        String,
        orbistoun_hle::knowledge::KnowledgeFile,
    > = std::collections::BTreeMap::new();
    for finding in &findings {
        let file = by_library
            .entry(finding.library.clone())
            .or_insert_with(|| orbistoun_hle::knowledge::KnowledgeFile {
                library: finding.library.clone(),
                functions: Vec::new(),
            });
        file.functions.push(finding.knowledge(origin));
    }
    for (library, file) in by_library {
        println!(
            "
# {library}"
        );
        print!("{}", file.render().context("rendering knowledge")?);
    }
    Ok(())
}

/// Analyses every shader binary in a directory.
///
/// A thin shim over `orbistoun_shader::report`, per principle 13: what the report says
/// is a property of the analysis, so this command and the run report cannot disagree
/// about it.
fn cmd_shaders(path: &std::path::Path, top: Option<usize>) -> Result<()> {
    use orbistoun_shader::corpus::is_shader;
    use orbistoun_shader::{
        CorpusCoverage, EncodingTable, MnemonicTable, OperandTable, decode, report,
    };

    let encodings = EncodingTable::builtin()?;
    let operands = OperandTable::builtin()?;
    let mnemonics = MnemonicTable::builtin()?;

    let files: Vec<std::path::PathBuf> = std::fs::read_dir(path)
        .map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();

    // Only the corpus's own extension. Reading everything in the directory decoded the
    // reference-text files alongside the binaries and reported eighteen shaders where
    // there were nine - plausible output, entirely wrong, and exactly the failure this
    // crate exists to make visible in guest code.
    let mut entries: Vec<std::path::PathBuf> = files
        .iter()
        // Both kinds: a shader dumped from a title, and one generated here. They
        // decode identically and differ only in provenance - which decides whether they
        // may be committed, not whether this can read them.
        .filter(|p| is_shader(p))
        .cloned()
        .collect();
    // Sorted so two runs over an unchanged directory produce identical output, which
    // is what makes the report diffable at all.
    entries.sort();

    let skipped = files.len() - entries.len();

    if entries.is_empty() {
        // Said plainly rather than reported as a clean sweep. An empty corpus produces
        // "0 of 0 complete", which reads like success.
        println!(
            "no shader files in {} ({skipped} other file(s) present)",
            path.display()
        );
        return Ok(());
    }

    let mut coverage = CorpusCoverage::new();
    for entry in &entries {
        let bytes =
            std::fs::read(entry).map_err(|e| anyhow::anyhow!("{}: {e}", entry.display()))?;
        let name = entry
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("<unnamed>");
        // The worklist ranks what is *not* translatable, so it has to ask the
        // translator rather than assume. Wiring these together is what makes the
        // report move as instructions are implemented instead of staying a fixed
        // picture of an empty translator.
        let supported = |key: orbistoun_shader::OpcodeKey| {
            key.encoding
                .and_then(|i| encodings.encodings().get(usize::from(i)))
                .is_some_and(|e| {
                    orbistoun_translate::model::supports_named(&encodings, &e.name, key.opcode)
                })
        };
        coverage.observe(name, &decode(&bytes, &encodings, &operands), &supported);
    }

    // The tier comes from the translator, which is the only layer that knows *why* an
    // instruction is refused. The shader crate can see what blocks a shader and not what
    // it would cost to fix, so the two are joined here rather than either guessing.
    let effort_of = |key: orbistoun_shader::coverage::OpcodeKey| {
        let named = key
            .encoding
            .and_then(|i| encodings.encodings().get(usize::from(i)))
            .and_then(|e| encodings.mnemonic_for(&e.name, key.opcode));
        match named {
            Some(name) if orbistoun_translate::model::blocked(name).is_some() => {
                orbistoun_shader::coverage::Effort::Subsystem
            }
            _ => orbistoun_shader::coverage::Effort::Ordinary,
        }
    };
    print!(
        "{}",
        report::render(&coverage, &encodings, &mnemonics, top, effort_of)
    );

    report_shader_movement(&coverage, &encodings, &mnemonics, path);
    if skipped > 0 {
        // Reported rather than assumed irrelevant: a corpus whose files carry a
        // different extension would otherwise look empty for no stated reason.
        println!(
            "
{skipped} file(s) skipped - not a shader"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    /// **A hash the database can already name does not stay on the work list.**
    ///
    /// The failing case, written first. The old rule removed only what a given run had
    /// just solved, and a named hash is never searched for again - so it could never be
    /// solved again, so it was never removed. 116 of 3829 committed entries were exactly
    /// this, against a file header promising they disappear as the vocabulary grows.
    #[test]
    fn a_hash_something_can_already_name_leaves_the_work_list() {
        let carried: std::collections::BTreeSet<u64> = [0x1111, 0x2222, 0x3333].into();
        // Nothing new is unnamed and nothing was solved this run - the only thing that has
        // changed since the list was written is that the vocabulary explains 0x2222.
        let now = super::wanted_now(carried, &[], &[], |nid| nid.as_raw() == 0x2222);
        assert_eq!(now, [0x1111, 0x3333].into());
    }

    /// A hash nothing can name stays, however many runs it survives.
    ///
    /// The other half, and the one that keeps the list a work list rather than an empty
    /// file: purging by "can be named" must not purge what merely was not solved today.
    #[test]
    fn a_hash_nothing_can_name_stays_on_the_work_list() {
        let carried: std::collections::BTreeSet<u64> = [0x1111].into();
        let now = super::wanted_now(carried, &[], &[], |_| false);
        assert_eq!(now, [0x1111].into());
    }

    /// What a run just solved goes, before it reaches the database.
    ///
    /// `is_named` answers from a service built before the search, and the database is
    /// written from `found` afterwards - so for exactly one run these names are known to
    /// nothing that this can ask. Dropping the second rule leaves each new name on the
    /// work list until the run after it.
    #[test]
    fn a_name_found_this_run_goes_before_it_reaches_the_database() {
        let nid = orbistoun_nid::Nid::from_raw(0x4444);
        let found = vec![orbistoun_names::solve::Solved {
            nid,
            name: "sceExample".to_owned(),
            derivation: orbistoun_nid::Derivation::new(
                orbistoun_nid::Method::Generated {
                    pattern: "sce{Verb}".to_owned(),
                    index: 0,
                },
                "2026-08-25",
            ),
        }];
        let now = super::wanted_now([0x4444].into(), &[], &found, |_| false);
        assert!(
            now.is_empty(),
            "a name found this run stayed on the work list"
        );
    }

    /// New unnamed imports join, so the list is the union across every module ever seen.
    #[test]
    fn newly_unnamed_imports_join_the_work_list() {
        let now = super::wanted_now(
            [0x1111].into(),
            &[orbistoun_nid::Nid::from_raw(0x5555)],
            &[],
            |_| false,
        );
        assert_eq!(now, [0x1111, 0x5555].into());
    }

    use super::{BLOCK_CLOSE, BLOCK_OPEN, parse_address, percent, suffix_for};

    #[test]
    fn addresses_parse_in_hex_and_decimal() {
        assert_eq!(parse_address("0x1000").expect("hex"), 0x1000);
        assert_eq!(parse_address("4096").expect("decimal"), 4096);
        assert_eq!(
            parse_address(" 0x800000000 ").expect("trimmed"),
            0x8_0000_0000
        );
        assert!(parse_address("nonsense").is_err());
        assert!(parse_address("0xzz").is_err());
    }

    /// Builds a `Cli` carrying just the suffix, which is all `suffix_for` reads.
    fn cli_with(suffix_hex: &str) -> super::Cli {
        use clap::Parser as _;
        super::Cli::parse_from(["orbistoun-cli", "--suffix-hex", suffix_hex, "symbols"])
    }

    #[test]
    fn no_suffix_given_means_the_one_orbistoun_ships_with() {
        // A user should never have to supply this. Resolving imports is the central act
        // of high-level emulation, so the value is not optional equipment (D071).
        let shipped = suffix_for(&cli_with("")).expect("the shipped suffix must load");
        assert_eq!(shipped, orbistoun_nid::default_suffix());
        assert!(!shipped.is_empty());
    }

    #[test]
    fn an_explicit_suffix_overrides_the_shipped_one() {
        let given = suffix_for(&cli_with("00ff10")).expect("valid hex");
        assert_eq!(given, vec![0x00, 0xff, 0x10]);
    }

    #[test]
    fn a_malformed_suffix_is_refused_rather_than_falling_back() {
        // Falling back would silently ignore what the user asked for and produce hashes
        // they did not request, which is worse than stopping.
        assert!(suffix_for(&cli_with("abc")).is_err(), "odd length");
        assert!(suffix_for(&cli_with("zz")).is_err(), "not hex");
    }

    #[test]
    fn percent_handles_the_empty_case_without_dividing_by_zero() {
        assert!((percent(0, 0) - 0.0).abs() < f64::EPSILON);
        assert!((percent(1, 4) - 25.0).abs() < f64::EPSILON);
        assert!((percent(1410, 1410) - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn every_module_extension_a_corpus_uses_is_recognised() {
        // **The regression this exists for.** Four hand-written globs in `bin/orbistoun`
        // covered `eboot.bin` and `.prx` and silently omitted `.sprx`, so eleven modules
        // in the local corpus were never searched even once - and a glob that matches
        // nothing is not an error, so nothing ever said so (D213).
        for name in ["eboot.bin", "libc.prx", "libSceAmpr.sprx", "EBOOT.BIN"] {
            assert!(super::is_guest_module(name), "{name} is a guest module");
        }
        for name in ["icon0.png", "param.json", "eboot.bin.bak", "notes.prx.txt"] {
            assert!(!super::is_guest_module(name), "{name} is not");
        }
    }

    #[test]
    fn a_module_reached_two_ways_records_as_one_module() {
        // A tab-completed directory argument left `titles/PPSA21564-app0//eboot.bin` on a
        // hundred provenance records, which reads as a different module from the same
        // file reached without the trailing slash.
        use std::path::Path;
        assert_eq!(
            super::record_path(Path::new("titles/X//eboot.bin")),
            "titles/X/eboot.bin"
        );
        assert_eq!(
            super::record_path(Path::new(r"titles\X\eboot.bin")),
            "titles/X/eboot.bin"
        );
    }

    #[test]
    fn a_dump_decodes_to_the_bytes_it_recorded() {
        assert_eq!(super::decode_hex_bytes("48656c6c6f"), b"Hello");
        // Whitespace and separators are how a dump is actually rendered.
        assert_eq!(super::decode_hex_bytes("48 65 6c 6c 6f"), b"Hello");
        // A trailing half-byte is dropped rather than guessed at: inventing the low
        // nibble would invent an identifier boundary that was never in guest memory.
        assert_eq!(super::decode_hex_bytes("48656c6c6f7"), b"Hello");
        assert!(super::decode_hex_bytes("").is_empty());
    }

    /// A generated block is replaced whole, and its surroundings are left alone.
    #[test]
    fn a_generated_block_is_spliced_between_its_markers() {
        let text = format!(
            "before
{BLOCK_OPEN}
stale
{BLOCK_CLOSE}
after
"
        );
        let fresh = format!(
            "{BLOCK_OPEN}
fresh
{BLOCK_CLOSE}"
        );
        let out = super::splice_block(&text, &fresh).expect("markers are present");
        assert_eq!(
            out,
            format!(
                "before
{fresh}
after
"
            )
        );
        assert!(!out.contains("stale"));
    }

    /// A file that lost its markers is refused rather than silently left behind.
    ///
    /// The failure that matters: a check reporting success over a file it never looked at
    /// is worse than no check, because it is believed (D240).
    #[test]
    fn a_file_without_markers_is_not_quietly_skipped() {
        assert!(super::splice_block("no markers here", "block").is_none());
        assert!(
            super::splice_block(
                &format!(
                    "{BLOCK_OPEN}
unclosed"
                ),
                "block"
            )
            .is_none(),
            "an unterminated block is not a block"
        );
    }
}
