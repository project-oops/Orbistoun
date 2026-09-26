//! `orbistoun-cli` - the command-line interaction shim.
//!
//! Parses arguments, calls `orbistoun-service` and the other crates, and prints what comes back. It
//! holds no logic (D034): behaviour lives one layer down, where the GUI and worker mode reach it
//! too. Each command's implementation is in the module named after it.

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
    /// Supplies readable names for import hashes. Independent of the unresolved-import count, which
    /// is about what orbistoun implements. The shipped database is used when omitted.
    #[arg(long, global = true)]
    symbols_db: Option<std::path::PathBuf>,

    #[command(subcommand)]
    command: Command,
}

/// Where a supplied word list came from.
///
/// Separates work this project did from work taken from elsewhere. Defaults to `supplied`, the less
/// generous label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum WordSource {
    /// Names this project's own conformance probe reported, running on the hardware.
    ///
    /// Ours, but not reproducible by this machine or CI, because producing it took the hardware
    /// (D213).
    Probe,
    /// Came from outside this project.
    Supplied,
}

/// Seconds of guest execution allowed before a run is stopped and reported.
///
/// Long enough for a guest to get past its startup path, short enough for an unattended sweep over
/// a directory of titles. Overridable per run.
const DEFAULT_GUEST_LIMIT_SECONDS: u64 = 20;

/// Imports a guest may call before it is stopped (D238).
///
/// Orders of magnitude above what a title doing real work makes, and far below what a guest
/// spinning on one import reaches, so it stops a runaway without truncating a real run.
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
        /// Modules link at zero and need one; executables carry absolute addresses and want zero.
        #[arg(long, default_value = "0", value_parser = parse_address)]
        base: u64,
    },
    /// Execute a guest, in a worker process.
    Run {
        /// Path to a guest executable.
        path: std::path::PathBuf,
        /// Seconds of guest execution to allow before stopping and reporting.
        ///
        /// A guest can settle into waiting on something that never happens. Zero removes the limit.
        #[arg(long, default_value_t = DEFAULT_GUEST_LIMIT_SECONDS)]
        limit: u64,
        /// Imports the guest may call before stopping and reporting.
        ///
        /// The deterministic limit: two runs of one build stop at the same call, so a verdict
        /// between them measures the change rather than the machine. The clock is a backstop for a
        /// guest that stops calling imports. Zero removes the budget.
        #[arg(long, default_value_t = DEFAULT_GUEST_CALL_BUDGET)]
        calls: u64,
        /// Present a named machine profile instead of the configured machine, e.g.
        /// `prospero-cex-12.40`. Omit to use `shell.toml`.
        #[arg(long)]
        profile: Option<String>,
        /// Play this pad script on player 1 instead of any the configuration names.
        ///
        /// A test's own input, or a recording of an earlier run replayed (D721).
        #[arg(long)]
        input: Option<std::path::PathBuf>,
        /// Run a loose build as a staged title, as though it lay under the library's
        /// `data/homebrew`.
        ///
        /// Its `/app0` is then writable through the title's overlay (D722). A module that does lie
        /// there is staged without asking.
        #[arg(long, alias = "homebrew")]
        staged: bool,
    },
    /// Find out which handoff fields a guest's runtime uses.
    ///
    /// The structure a payload's runtime is handed is not published, so each field is poisoned with
    /// an address nothing maps, one run per field, and a fault on it shows the field is used
    /// (D390). Needs no symbols and no source.
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
    /// Runs the guest, ranks what went wrong, and takes every mechanical step - sweeping a call's
    /// arguments, asking the other diagnostics, arming a watchpoint, giving the guest a region when
    /// a sweep says it lacked one. Stops at the steps that are a person's, each with a reason.
    Turn {
        /// Path to a guest executable.
        path: std::path::PathBuf,
        /// Print what the turn established as a `learn` command, not only the steps.
        ///
        /// Printed rather than written: changing a tracked file stays a deliberate act with a diff
        /// (D291).
        #[arg(long)]
        record: bool,
        /// Write what the turn measured into the learned policy, so the next run carries it.
        ///
        /// `learned.toml` is untracked, sits beside `config.toml`, is folded in underneath it and
        /// loses to every entry a person wrote. Deleting it is a complete undo (D296).
        #[arg(long)]
        apply: bool,
        /// Check a submitted learned file against what this machine measures.
        ///
        /// A measurement is checked by measuring again, not trusted: the claim is falsifiable by a
        /// command (D297).
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
    /// A name list and suffix are correct exactly to the extent they explain hashes a real module
    /// imports; a hash match is the proof.
    Verify {
        /// Path to a guest executable or module.
        path: std::path::PathBuf,
    },
    /// Search generated names for ones that hash to a module's unnamed imports.
    ///
    /// Clean-room: names are proposed and the hash confirms or rejects each, so a reported name is
    /// proved. A miss proves only that the name was not among those tried; extending the vocabulary
    /// is the method.
    Names {
        /// A guest executable or module, or a directory to search every module beneath.
        ///
        /// A directory is one search: the unnamed imports of every module are unioned first, so the
        /// sweep runs once and every module's strings are tried against every module's imports.
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
        /// `probe` for names this project's conformance probe reported; `supplied` for anything
        /// from outside, which never verifies and is listed separately by an audit (D213).
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
        /// Needs a previous run with forced argument dumps, so it is off unless asked for.
        #[arg(long)]
        from_trace: bool,
        /// Also look for names for hashes a conformance probe reported the platform exports.
        ///
        /// An export table lists what the platform offers, whether or not anything imported it.
        /// These hashes are targets, not facts: a name that comes back is proved by the hash, so a
        /// report cannot put a name in.
        #[arg(long, value_name = "REPORT")]
        from_report: Vec<std::path::PathBuf>,
    },
    /// Record something learned about a guest function.
    ///
    /// Appends to the knowledge file, so a finding is recorded with a command rather than
    /// hand-formatted TOML (D122).
    Learn(Learned),
    /// Read and update the per-title compatibility record.
    ///
    /// The measured half of a title file, written from a trace rather than by hand (D182).
    Compat {
        #[command(subcommand)]
        action: CompatAction,
    },
    /// The test corpus: fetch pinned guests from configured sources, run them, record results.
    ///
    /// `corpus/sources.toml` names where guests come from (D042); `sync` downloads them into the
    /// title library and pins each by hash; `run` runs every one and records to `compat/` exactly
    /// as a hand `run` does.
    Corpus {
        #[command(subcommand)]
        action: CorpusAction,
    },
    /// Gather what this machine has to contribute, or check what somebody sent.
    ///
    /// Lets someone running a binary against their own titles send back what they found.
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
    /// A driver can point at a probe or at this and compare the records live. Opens a socket, so it
    /// runs only when asked and never in an automated path. Only `report` is served: a `call` needs
    /// a running guest, so that capability is not announced.
    Serve {
        /// Address to listen on. Loopback by default.
        ///
        /// A responder reachable from a network can be driven by anything on it.
        #[arg(long, default_value = "127.0.0.1:9599")]
        bind: String,

        /// Serve without requiring a session secret.
        ///
        /// Sound on loopback. Refused when `--bind` is not a loopback address: "no password" and
        /// "reachable from the network" are separate choices.
        #[arg(long)]
        no_key: bool,

        /// Serve one session and exit, rather than accepting until interrupted.
        #[arg(long)]
        once: bool,
    },
    /// Emit the generated numbers block for the documentation, or check it for drift.
    ///
    /// The numbers are printed by the tool rather than counted by hand.
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
    /// Portable mode moves every location at once.
    Paths,
    /// List every environment variable orbistoun reads, and what is set right now.
    ///
    /// A misspelled variable is an absence, not an error: the run behaves normally. This prints the
    /// registry (D221).
    Env,
    /// Emit every open question, ranked by how often a guest calls the function.
    ///
    /// Every `assumptions` line in the knowledge base is something unknown and measurable; ranked,
    /// they are a work queue for a hardware probe.
    Questions {
        /// How many to show. Omit for all of them.
        #[arg(long)]
        top: Option<usize>,
        /// Emit JSON, for a probe or an agent to consume.
        #[arg(long)]
        json: bool,
        /// Group by the premise entries share, rather than one line per function.
        ///
        /// Many questions are one sentence repeated across the entries resting on it; grouped, one
        /// sample can speak for the group (D538).
        #[arg(long)]
        premises: bool,
    },
    /// Rank what to implement next, across every guest run so far.
    ///
    /// Totals the call traces every run persists: what guests actually called, and how often.
    Worklist {
        /// How many entries to show.
        #[arg(long, default_value_t = 25)]
        top: usize,
        /// Rank the static import lists instead of the call traces.
        ///
        /// What a guest might call, grouped by where an answer can come from. For a published
        /// interface that needs no run at all.
        #[arg(long)]
        static_gap: bool,
    },
    /// Rebuild the standard-library word list from a FreeBSD source tree.
    ///
    /// The target C library is FreeBSD-derived, and FreeBSD publishes what its libraries export,
    /// citable at a named revision. Only the version scripts are read, so a sparse checkout is
    /// enough:
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
    /// The provenance check: a name this repository cannot produce is the one that needs
    /// explaining. Cheap enough to run on every commit.
    Audit {
        /// Path to a symbol database.
        database: std::path::PathBuf,
        /// Grammar file to check against instead of the built-in vocabulary.
        #[arg(long)]
        grammar: Option<std::path::PathBuf>,
        /// Compare the unaccounted set against a written-down ceiling instead of failing on any.
        ///
        /// Names the current grammar cannot regenerate are a recorded, shrinking set; the ceiling
        /// keeps the gate green for them while a new unaccounted name still fails. The file may
        /// only shrink: a listed name that has since been accounted for also fails.
        #[arg(long)]
        ceiling: Option<std::path::PathBuf>,
        /// Search the whole space for names carrying no derivation record.
        ///
        /// Slow - it walks every candidate per unaccounted name - but the only way to ask whether a
        /// name without a record could have been generated.
        #[arg(long)]
        deep: bool,
        /// Re-read every module a static record names, and confirm it contains the string.
        ///
        /// Needs the guest material, so CI cannot run it (D213). Off by default: it scans every
        /// module in the corpus, too slow for the per-commit gate.
        #[arg(long)]
        verify_harvest: bool,
        /// Re-derive generated records the current grammar no longer confirms, and write them back.
        ///
        /// A learned word renumbers the candidates built from its vocabulary (D195), so verified
        /// records drift onto the unaccounted ceiling. One pass repairs every stale record.
        #[arg(long)]
        repair: bool,
    },
    /// Compute the import hash for one or more names.
    ///
    /// The counterpart of `exports`, which lists a module's symbols as hashes: finding a name there
    /// means hashing it with the run's suffix and looking for the number.
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
        /// With `--own`, place and relocate them against one shared stub table.
        #[arg(long)]
        linked: bool,
        /// Show the vendor library and module tables instead of the imports.
        ///
        /// These are what an encoded import name's ids index, and neither is `DT_NEEDED`. Whether
        /// their strings are bare names or paths decides how a loader finds an imported module.
        #[arg(long)]
        libraries: bool,
    },
    /// Report what a guest module provides, without executing it.
    ///
    /// The counterpart of `imports`: a title's executable imports from its own modules by NID, so
    /// their exports decide whether those imports can bind.
    Exports {
        /// Path to a guest module.
        path: std::path::PathBuf,
        /// Only show symbols whose name or NID contains this.
        #[arg(long)]
        matching: Option<String>,
    },
    /// Ask a live probe one question and print what it answers.
    ///
    /// Asks the hardware what a function does instead of guessing. Prints the answer as it is -
    /// `returned 0x2`, `died`, `refused unauthorised` - so a non-answer never reads as an answer.
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
        /// Shows the grade, the caveat and, for a handle or pointer, that the value was recorded
        /// rather than handed to a guest. Printed, never written.
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
    /// The probe listens and this connects, since the hardware has an address a person can read off
    /// a screen. The transcript file is the product.
    Session {
        /// `host:port` of the listening probe.
        address: String,
        /// Session secret, shown by the probe when it starts listening.
        ///
        /// Generated per start, so a restart invalidates the old one.
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
        /// Usually unnecessary: `--device` carries it. This asks whether the device is the emulated
        /// platform, not whether it is real hardware.
        #[arg(long)]
        is_target: bool,
        /// Seconds to wait for any one command before calling it a timeout.
        #[arg(long, default_value_t = 30)]
        budget: u64,
    },
    /// Read a probe transcript or corpus and report what it establishes.
    ///
    /// Says how many results are facts about the target rather than reasoning or measurements on a
    /// different device. Needs no hardware.
    Probe {
        /// A transcript or corpus file.
        path: std::path::PathBuf,
        /// What the operator asserts this ran on: the hardware, or a named emulator.
        ///
        /// Asked for rather than read off the records: inside an emulator a probe reports the
        /// emulator's version as the platform's, so a `target` on the wire is a claim, not
        /// evidence.
        #[arg(long)]
        device: Option<String>,
        /// Firmware or version, where the operator knows it.
        #[arg(long)]
        firmware: Option<String>,
        /// Assert that the device named is the target platform itself.
        ///
        /// Usually unnecessary: `--device` carries it, and a known stand-in (a host build, a named
        /// emulator, other x86-64 hardware) is treated as one without being told. Grading turns on
        /// whether the device was the platform being emulated, not on whether it was real hardware.
        #[arg(long)]
        is_target: bool,
        /// Render what was established as knowledge entries, to standard output.
        ///
        /// Printed rather than written: merging into the knowledge base is a separate, deliberate
        /// act.
        #[arg(long)]
        as_knowledge: bool,
        /// Compare against another transcript and report the checks that disagree.
        ///
        /// The conformance probe runs both on the hardware and under orbistoun, so its own verdicts
        /// can be compared: a check that passes in the reference and fails here is a defect with
        /// the probe's sentence attached. Give the hardware transcript here and the local one as
        /// `path`.
        #[arg(long, value_name = "REFERENCE")]
        against: Option<std::path::PathBuf>,
    },
    /// Analyse a directory of shader binaries and rank what blocks translation.
    ///
    /// Ranks which instruction, if supported, would unblock the most shaders. Needs no GPU, driver
    /// or running guest.
    Shaders {
        /// Directory of shader binaries, one per file.
        path: std::path::PathBuf,
        /// Show only the top N blockers. Omit for the whole list.
        #[arg(long)]
        top: Option<usize>,
    },
    /// Show how the firmware skeleton lays out libkernel.
    ///
    /// Prints the stub each export address gets, which exports are unimplemented, and where a stub
    /// overruns its neighbour.
    Firmware {
        /// Show every export, not just collisions and unimplemented ones a payload might reach.
        #[arg(long)]
        all: bool,
    },
}

/// Everything `learn` records about one function.
///
/// A struct rather than variant fields, so the field list lives in one place.
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
    /// Required whenever anything beyond a name is recorded. No value means "already known": every
    /// option names something that could contradict it. See `Oracle`.
    #[arg(long, value_enum)]
    known: Option<KnownBy>,
    /// Where to check it: a standard clause, a source file and revision, a probe identifier.
    /// Required by `--known published` and `--known measured`.
    #[arg(long)]
    cites: Option<String>,
    /// A specific claim in this entry that `--known` does not cover. Repeatable.
    ///
    /// Each is a question the hardware could settle.
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
    /// unmeasured, not as a contradiction.
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
    /// Render every record as a ranked markdown table into a tracked file.
    ///
    /// The ranking `list` prints, as a document in the repository, with a per-title page each. A
    /// title with a screenshot beside its record gets the image embedded.
    Markdown {
        /// Where the records live.
        #[arg(long, default_value = "compat")]
        dir: std::path::PathBuf,
        /// Where to write the table.
        #[arg(long, default_value = "COMPATIBILITY.md")]
        out: std::path::PathBuf,
        /// Directory of `<title>.png` screenshots, relative to the repository root.
        #[arg(long, default_value = "compat/screenshots")]
        shots: std::path::PathBuf,
        /// Instead of writing, fail if what is on disk is not what the records render to.
        ///
        /// Checks `COMPATIBILITY.md` and `docs/titles/`, the files `status --check` does not cover.
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
        /// For a deliberate correction: an entry measured wrongly, or a regression recorded as the
        /// new truth. Never the default.
        #[arg(long)]
        force: bool,
    },
}

/// What to do with the test corpus.
#[derive(clap::Subcommand, Debug)]
enum CorpusAction {
    /// Show the manifest: every source, its assets, and whether each is pinned.
    List {
        /// The manifest to read.
        #[arg(long, default_value = "corpus/sources.toml")]
        manifest: std::path::PathBuf,
    },
    /// Fetch every source's assets into the title library, pinning or verifying each by hash.
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
        /// Present a named machine profile for the runs, e.g. `prospero-cex-12.40`.
        #[arg(long)]
        profile: Option<String>,
    },
}

/// How a behavioural claim was established, as the command line spells it.
///
/// A mirror of [`orbistoun_hle::knowledge::Oracle`], because clap's derive needs its own trait on
/// the type and the knowledge crate takes no command-line dependency. A test holds the two in step.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
enum KnownBy {
    /// A published standard or published source that specifies this function.
    Published,
    /// Measured on the hardware by a conformance probe.
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
fn parse_address(text: &str) -> Result<u64, String> {
    let t = text.trim();
    let parsed = t.strip_prefix("0x").map_or_else(
        || t.parse::<u64>().map_err(|e| e.to_string()),
        |hex| u64::from_str_radix(hex, 16).map_err(|e| e.to_string()),
    );
    parsed.map_err(|e| format!("{t:?} is not an address: {e}"))
}

/// The suffix to hash with: whatever was asked for, else the one orbistoun ships (D071).
fn suffix_for(cli: &Cli) -> Result<Vec<u8>> {
    if cli.suffix_hex.is_empty() {
        return Ok(orbistoun_nid::default_suffix());
    }
    orbistoun_nid::decode_hex(&cli.suffix_hex)
        .context("--suffix-hex must be an even number of hexadecimal digits")
}

/// Enters worker mode: host the crates and speak the protocol over stdio.
///
/// Not a clap subcommand: it is how the shims execute guests, not a user-facing verb (D033).
fn run_as_worker() -> Result<()> {
    orbistoun_worker::serve_as_worker_process().map_err(|e| anyhow::anyhow!(e))
}

/// Runs whichever command was asked for.
///
/// Split from `main`, which configures the process; this decides what it does.
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
    // The shared logging setup; the guard is held for the whole of `main`, and `let _` would drop
    // it here.
    let _logging = oops_log::Logging::new("orbistoun")
        .build(orbistoun_env::build::line_static())
        .init();

    // Checked before clap sees the arguments: worker mode is not a user-facing verb.
    if std::env::args().any(|a| a == orbistoun_worker::WORKER_FLAG) {
        return run_as_worker();
    }

    let cli = Cli::parse();
    let symbol_db = match cli.symbols_db.as_ref() {
        // The shipped database loads unless a path is given (D188).
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

    // Portable-first resolution decides where reports land; expired artifacts are purged at startup
    // so long runs do not fill a disk.
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

/// The commands that talk to a live probe: the ones that open a socket.
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

    /// Addresses parse in hex and decimal, trimmed; anything else is refused.
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

    /// With no suffix given, the one orbistoun ships with is used (D071).
    #[test]
    fn no_suffix_given_means_the_one_orbistoun_ships_with() {
        let shipped = suffix_for(&cli_with("")).expect("the shipped suffix must load");
        assert_eq!(shipped, orbistoun_nid::default_suffix());
        assert!(!shipped.is_empty());
    }

    /// An explicit suffix overrides the shipped one.
    #[test]
    fn an_explicit_suffix_overrides_the_shipped_one() {
        let given = suffix_for(&cli_with("00ff10")).expect("valid hex");
        assert_eq!(given, vec![0x00, 0xff, 0x10]);
    }

    /// A malformed suffix is refused rather than falling back to the shipped one.
    #[test]
    fn a_malformed_suffix_is_refused_rather_than_falling_back() {
        assert!(suffix_for(&cli_with("abc")).is_err(), "odd length");
        assert!(suffix_for(&cli_with("zz")).is_err(), "not hex");
    }
}
