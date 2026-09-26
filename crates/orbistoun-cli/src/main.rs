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

mod audit;
mod common;
mod compat;
mod corpus;
mod learn;
mod module;
mod names;
mod probe;
mod progress;
mod questions;
mod run;
mod shaders;
mod status;
mod submit;
mod system;
mod turn;
mod worklist;

use crate::audit::{cmd_audit, cmd_harvest};
use crate::common::library_or;
use crate::compat::{cmd_compat_list, cmd_compat_markdown, cmd_compat_record};
use crate::corpus::{cmd_corpus_list, cmd_corpus_run, cmd_corpus_sync};
use crate::learn::{cmd_knows, cmd_learn};
use crate::module::{
    cmd_exports, cmd_imports, cmd_inspect, cmd_load, cmd_module_tables, cmd_report, cmd_symbols,
    cmd_title_modules, cmd_verify,
};
use crate::names::{NameSearch, cmd_names};
use crate::probe::{cmd_ask, cmd_probe, cmd_session};
use crate::questions::cmd_questions;
use crate::run::{cmd_handoff, cmd_run};
use crate::shaders::cmd_shaders;
use crate::status::cmd_status;
use crate::submit::dispatch_submit;
use crate::system::{cmd_env, cmd_firmware_layout, cmd_paths, cmd_serve};
use crate::turn::cmd_turn;
use crate::worklist::{cmd_worklist, cmd_worklist_static};
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
    /// Rarely needed. The shipped value is `selfish-nid`'s, documented in selfish's
    /// `data/hash-suffix.toml`. See docs/SYMBOLS.md.
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
        /// e.g. `prospero-cex-12.40`, the measured reference target. Omit to use `shell.toml`.
        #[arg(long)]
        profile: Option<String>,
        /// Play this pad script on player 1 instead of any the configuration names (D721) - a
        /// test's own input, or a recording of an earlier run replayed.
        #[arg(long)]
        input: Option<std::path::PathBuf>,
        /// Run a loose build as a staged title, as though it lay under the library's
        /// `data/homebrew` tree: its `/app0` is writable through the title's overlay (D722). A
        /// module that does lie there is staged without asking.
        #[arg(long, alias = "homebrew")]
        staged: bool,
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
        /// Also look for names for hashes a conformance probe reported the platform exports.
        ///
        /// A hash from an import table is one a title asked for. A hash from a console's own
        /// export table is one the platform *offers*, whether or not anything has ever
        /// imported it - a census a collision search cannot reach by any other route (D245).
        ///
        /// **Grading does not enter into it.** These hashes are targets, not facts: the name
        /// that comes back is proved by the hash agreeing, and would be equally proved if the
        /// report were fabricated. What a report cannot do here is put a name in.
        #[arg(long, value_name = "REPORT")]
        from_report: Vec<std::path::PathBuf>,
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
        /// Group by the premise entries share, rather than one line per function.
        ///
        /// **Because the list is shorter than its own length.** Most of these questions are
        /// one sentence repeated across every entry resting on it, so a reader is shown
        /// hundreds of asks where there are fewer things to establish - and a probe planning
        /// a sweep cannot see that one sample would speak for a whole group (D538).
        #[arg(long)]
        premises: bool,
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
        /// Rank the *static* import lists instead of the call traces.
        ///
        /// What a guest might call, grouped by where an answer can come from. The trace
        /// ranking answers "what is this guest leaning on"; this answers "what should
        /// somebody write next", which for published interfaces needs no run at all.
        #[arg(long)]
        static_gap: bool,
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
    /// Compute the import hash for one or more names.
    ///
    /// The other half of `exports`: that lists a module's symbols as hashes, and a vendor
    /// module's are **all** encoded, so asking "does this module export `module_start`" means
    /// hashing the name and looking for the number. Doing that by hand needs the suffix, which
    /// is a run input rather than a constant (principle 5).
    Nid {
        /// Names to hash.
        #[arg(required = true)]
        names: Vec<String>,
    },
    /// Report what a guest module imports, without executing it.
    Imports {
        /// Path to a guest executable.
        path: std::path::PathBuf,
        /// Show the modules the title ships that answer its own imports.
        #[arg(long)]
        own: bool,
        /// With `--own`, place them and report which imports would bind into them.
        #[arg(long)]
        placed: bool,
        /// With `--own`, place **and relocate** them against one shared stub table.
        #[arg(long)]
        linked: bool,
        /// Show the vendor library and module tables instead of the imports.
        ///
        /// These are what an encoded import name's ids index, and neither is `DT_NEEDED`.
        /// Whether their strings are bare names or paths is what decides how a loader finds
        /// a module an executable imports from.
        #[arg(long)]
        libraries: bool,
    },
    /// Report what a guest module **provides**, without executing it.
    ///
    /// The other half of `imports`. A title ships its own modules and the executable imports
    /// from them by NID, so what those modules export is what decides whether those imports
    /// can ever bind.
    Exports {
        /// Path to a guest module.
        path: std::path::PathBuf,
        /// Only show symbols whose name or NID contains this.
        #[arg(long)]
        matching: Option<String>,
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
        /// Compare against another transcript and report the checks that disagree.
        ///
        /// **The strongest tool this project has, and it needed one command.** The conformance
        /// probe is the only guest whose source is available, it passes on a console, and it
        /// runs under orbistoun - so the same binary runs in both places and its own verdicts
        /// can be compared. A check that passes in the reference and fails here is a named,
        /// sourced defect with the probe's own sentence attached (D622).
        ///
        /// Give the **hardware** transcript here and the local one as `path`: the reference is
        /// what a console did, the subject is what orbistoun did.
        #[arg(long, value_name = "REFERENCE")]
        against: Option<std::path::PathBuf>,
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
        /// Instead of writing, fail if what is on disk is not what the records render to.
        ///
        /// The guard `status --check` is for `docs/PROJECT_STATUS.md`; this is the
        /// same guard over the two things it does not cover - `COMPATIBILITY.md` and `docs/titles/`.
        /// The `docs` step runs it.
        #[arg(long)]
        check: bool,
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
        /// Where guest bytes land. Omit for the shared title library (`orbistoun-cli paths`).
        #[arg(long)]
        titles: Option<std::path::PathBuf>,
    },
    /// Sync, then run every guest and record what it reached to `compat/`.
    Run {
        /// Only this source, by name. Omit for all.
        #[arg(long)]
        source: Option<String>,
        /// The manifest to read.
        #[arg(long, default_value = "corpus/sources.toml")]
        manifest: std::path::PathBuf,
        /// Where guest bytes land. Omit for the shared title library (`orbistoun-cli paths`).
        #[arg(long)]
        titles: Option<std::path::PathBuf>,
        /// Seconds each guest may run before it is stopped and reported.
        #[arg(long, default_value_t = DEFAULT_GUEST_LIMIT_SECONDS)]
        limit: u64,
        /// Imports each guest may call before it is stopped and reported.
        #[arg(long, default_value_t = DEFAULT_GUEST_CALL_BUDGET)]
        calls: u64,
        /// Present a named console profile for the runs, e.g. `prospero-cex-12.40`.
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
fn dispatch(cli: Cli, service: &Service) -> Result<()> {
    match cli.command {
        Command::Symbols { .. }
        | Command::Policy
        | Command::Inspect { .. }
        | Command::Nid { .. }
        | Command::Imports { .. }
        | Command::Exports { .. }
        | Command::Shaders { .. }
        | Command::Verify { .. }
        | Command::Report { .. }
        | Command::Load { .. } => dispatch_module(cli.command, service)?,
        Command::Ask { .. } | Command::Session { .. } => dispatch_probe(cli.command)?,
        Command::Probe {
            path,
            device,
            firmware,
            is_target,
            as_knowledge,
            against,
        } => cmd_probe(
            &path,
            device,
            firmware,
            is_target,
            as_knowledge,
            against.as_deref(),
            service,
        )?,
        Command::Serve { bind, no_key, once } => cmd_serve(service, &bind, no_key, once)?,
        Command::Turn { .. }
        | Command::Run { .. }
        | Command::Handoff { .. }
        | Command::Names { .. }
        | Command::Learn(..) => dispatch_guest(cli, service)?,
        Command::Compat { .. } | Command::Corpus { .. } | Command::Submit { .. } => {
            dispatch_records(&cli.command)?;
        }
        Command::Knows { .. }
        | Command::Status { .. }
        | Command::Paths
        | Command::Env
        | Command::Firmware { .. }
        | Command::Questions { .. }
        | Command::Worklist { .. }
        | Command::Harvest { .. }
        | Command::Audit { .. } => dispatch_reports(&cli.command, service)?,
    }

    Ok(())
}

/// The commands that read one guest module or a file beside it.
fn dispatch_module(command: Command, service: &Service) -> Result<()> {
    match command {
        Command::Symbols { filter } => cmd_symbols(service, filter.as_deref()),
        Command::Policy => println!("{}", service.default_policy_toml()?),
        Command::Inspect { path } => cmd_inspect(service, &path)?,
        Command::Nid { names } => {
            for name in &names {
                println!("{:#018x}  {name}", service.hash_name(name).as_raw());
            }
        }
        Command::Imports {
            path,
            own,
            placed,
            linked,
            libraries,
        } => {
            if own {
                cmd_title_modules(service, &path, placed, linked)?;
            } else if libraries {
                cmd_module_tables(service, &path)?;
            } else {
                cmd_imports(service, &path)?;
            }
        }
        Command::Exports { path, matching } => {
            cmd_exports(service, &path, matching.as_deref())?;
        }
        Command::Shaders { path, top } => cmd_shaders(&path, top)?,
        Command::Verify { path } => cmd_verify(service, &path)?,
        Command::Report { path } => cmd_report(service, &path)?,
        Command::Load { path, base } => cmd_load(service, &path, base)?,
        _ => unreachable!("dispatch_module is only reached for its own commands"),
    }

    Ok(())
}

/// The commands that run a guest or learn from what a run found.
fn dispatch_guest(cli: Cli, service: &Service) -> Result<()> {
    match cli.command {
        Command::Turn {
            ref path,
            record,
            apply,
            ref verify,
        } => {
            cmd_turn(
                path,
                record,
                apply,
                verify.as_deref(),
                cli.symbols_db.as_deref(),
            )?;
        }
        Command::Run {
            path,
            limit,
            calls,
            profile,
            input,
            staged,
        } => cmd_run(
            &path,
            limit,
            calls,
            profile.as_deref(),
            (cli.symbols_db.as_deref(), input.as_deref()),
            staged,
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
            ref from_report,
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
                from_report,
            },
        )?,
        Command::Learn(ref learned) => cmd_learn(learned)?,
        _ => unreachable!("dispatch_guest is only reached for its own commands"),
    }

    Ok(())
}

/// The commands that keep the compatibility, corpus and submission records.
fn dispatch_records(command: &Command) -> Result<()> {
    match *command {
        Command::Compat { ref action } => match action {
            CompatAction::List { dir } => cmd_compat_list(dir)?,
            CompatAction::Markdown {
                dir,
                out,
                shots,
                check,
            } => cmd_compat_markdown(dir, out, shots, *check)?,
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
            } => cmd_corpus_sync(manifest, &library_or(titles.as_deref()), source.as_deref())?,
            CorpusAction::Run {
                source,
                manifest,
                titles,
                limit,
                calls,
                profile,
            } => cmd_corpus_run(
                manifest,
                &library_or(titles.as_deref()),
                source.as_deref(),
                *limit,
                *calls,
                profile.as_deref(),
            )?,
        },
        Command::Submit { ref action } => dispatch_submit(action)?,
        _ => unreachable!("dispatch_records is only reached for its own commands"),
    }

    Ok(())
}

/// The commands that report on what orbistoun knows and what to do next.
fn dispatch_reports(command: &Command, service: &Service) -> Result<()> {
    match *command {
        Command::Knows { ref pattern } => cmd_knows(pattern.as_deref()),
        Command::Status { write, check } => cmd_status(service, write, check)?,
        Command::Paths => cmd_paths(),
        Command::Env => cmd_env(),
        Command::Firmware { all } => cmd_firmware_layout(service, all),
        Command::Questions {
            top,
            json,
            premises,
        } => cmd_questions(top, json, premises),
        Command::Worklist { top, static_gap } => {
            if static_gap {
                cmd_worklist_static(service, top);
            } else {
                cmd_worklist(top);
            }
        }
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
        _ => unreachable!("dispatch_reports is only reached for its own commands"),
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

#[cfg(test)]
mod tests {
    use super::{parse_address, suffix_for};

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
}
