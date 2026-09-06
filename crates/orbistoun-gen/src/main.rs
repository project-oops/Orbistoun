//! Offline generators for the shader data tables.
//!
//! # What this is, and what it is not
//!
//! **Not part of the emulator.** Nothing here is on the build, test, or run path of
//! anything that executes a guest. It exists to produce the `.toml` files under
//! `crates/orbistoun-shader/data/`, which are committed, and which the decoder reads.
//!
//! **No machine in this project's normal setup can run these against a live assembler.**
//! They need `llvm-mc` with the AMDGPU target, which `tools/toolchain/setup.sh` builds a VM
//! for. That constraint is the reason for the seam below, and for everything about how this
//! crate is tested.
//!
//! # The seam that makes it checkable
//!
//! Every generator gets its bytes through [`assembler`], which either shells out to
//! `llvm-mc` or replays a committed recording of having done so. The second mode needs
//! nothing installed, which is what lets the solvers be tested at all - and the solvers are
//! where the difficulty lives. The subprocess call is a dozen lines; the bit arithmetic is
//! two thousand.
//!
//! # Fidelity over improvement
//!
//! The correctness argument for the port is that it produces byte-identical output. So the
//! translation is deliberately literal, including where the original reads oddly. Improving
//! a solver and porting it at the same time makes any difference in the result impossible
//! to attribute.

mod assembler;
mod buffer_formats;
mod constants;
mod ctype;
mod encodings;
mod fixtures;
mod hardware;
mod knowledge;
mod operands;
mod patterns;
mod solve;
mod table;
mod target;

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

/// Offline generators for the shader data tables.
#[derive(Parser)]
#[command(name = "orbistoun-gen", about, long_about = None)]
struct Cli {
    /// Replay a recorded assembler transcript instead of invoking `llvm-mc`.
    ///
    /// **The mode that needs no toolchain.** A recording is taken once, on a machine with
    /// an AMDGPU-enabled LLVM, and committed - after which the solver can be re-run and
    /// its output diffed anywhere, including in CI.
    #[arg(long, global = true, value_name = "DIR")]
    transcript: Option<PathBuf>,

    /// Record every assembler invocation into this directory, for later replay.
    #[arg(long, global = true, value_name = "DIR")]
    record: Option<PathBuf>,

    /// Print what would be written instead of writing it.
    ///
    /// The honest way to check a port: generate, diff against what is committed, and only
    /// then overwrite.
    #[arg(long, global = true)]
    dry_run: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Print one field of the target the generators assemble for.
    ///
    /// Exists so a shell script reads the same source the generators do, rather than
    /// hardcoding a target that then outlives its correctness (D139).
    Target {
        /// One of `mcpu`, `mattr`, `triple`, `graphics-triple`.
        field: String,
    },
    /// Solve each encoding family's identifying bits from assembled samples.
    ///
    /// Reports rather than writes: `data/encodings.toml` carries reasoning and citations a
    /// person maintains, and this solves the numbers in it. Overwriting the file would throw
    /// the prose away.
    Encodings {
        /// Directory of per-family probe files.
        #[arg(long, default_value = "tools/shader-fixtures/families")]
        families: PathBuf,
    },
    /// Regenerate the differential-test fixtures and the mnemonic table.
    ///
    /// Writes `.gcn` and `.txt` per source, plus `data/mnemonics.toml` from what the
    /// reference disassembler actually named. Both are committed, so LLVM is not a test
    /// dependency - this runs when somebody wants new coverage, the tests run everywhere.
    Fixtures {
        /// Directory of shader sources to compile.
        #[arg(long, default_value = "tools/shader-fixtures")]
        sources: PathBuf,
        /// Where the fixtures go.
        #[arg(long, default_value = "crates/orbistoun-shader/tests/fixtures")]
        out: PathBuf,
        /// The encoding table to classify against.
        #[arg(long, default_value = "crates/orbistoun-shader/data/encodings.toml")]
        encodings: PathBuf,
        /// Where the mnemonic table goes.
        #[arg(long, default_value = "crates/orbistoun-shader/data/mnemonics.toml")]
        mnemonics: PathBuf,
    },
    /// Solve per-opcode operand layouts from assembled probes.
    Operands {
        /// Directory of probe files.
        #[arg(long, default_value = "tools/shader-fixtures/probes")]
        probes: PathBuf,
        /// The encoding table, so a probe can be attributed to a family and opcode.
        #[arg(long, default_value = "crates/orbistoun-shader/data/encodings.toml")]
        encodings: PathBuf,
        /// The operand-code table, for the names a register cannot reach.
        #[arg(long, default_value = "crates/orbistoun-shader/data/operands.toml")]
        codes: PathBuf,
        /// Where to write the solved layouts.
        #[arg(
            long,
            default_value = "crates/orbistoun-shader/data/opcode-operands.toml"
        )]
        out: PathBuf,
    },
    /// Harvest ABI constants from a FreeBSD source checkout.
    ///
    /// The naming harvest takes symbol *names* and deliberately no constants, which was
    /// right while the work was naming. Implementing needs numbers - `AF_INET` cannot be
    /// mapped onto a host socket by guessing it (D352).
    ///
    /// Only headers are read, so a sparse checkout is plenty:
    ///
    ///     git clone --filter=blob:none --sparse https://github.com/freebsd/freebsd-src
    ///     cd freebsd-src
    ///     git sparse-checkout set lib/libc lib/libsys lib/libthr lib/libutil lib/msun
    ///     git sparse-checkout add sys/sys sys/netinet include
    /// Write the committed table of what a conformance run measured.
    ///
    /// The `measure` records carry subject, condition, observation and kind, which is enough
    /// for an assertion to name one and be checked against it. A measurement every run agreed
    /// on is marked constant; one they disagreed on is kept and marked not.
    Measurements {
        /// The directory of capture files, in the sibling conformance-probe repository.
        #[arg(long, default_value = "../obscene/data/hardware")]
        records: PathBuf,
        /// Where the generated table goes.
        #[arg(long, default_value = "crates/orbistoun-hle/data/hardware.toml")]
        out: PathBuf,
    },
    /// Attach conformance-run observations to the functions they exercised.
    ///
    /// Joins `try` and `res` on their check id and records each outcome as an edge case on
    /// the function it names - **quoted, not interpreted**. The value's meaning lives in the
    /// check rather than the record, so reading every one as a return value would write
    /// timestamps and loop counters into the knowledge base as behaviour.
    Hardware {
        /// The directory of capture files, in the sibling conformance-probe repository.
        #[arg(long, default_value = "../obscene/data/hardware")]
        records: PathBuf,
        /// Where the per-library knowledge files live.
        #[arg(long, default_value = "crates/orbistoun-hle/data/knowledge")]
        out: PathBuf,
    },
    /// Derive knowledge entries from the implementations' own documentation.
    ///
    /// For every implemented symbol with no entry, reads the doc comment on the function
    /// that implements it and records what it says - purpose, the specification it cites,
    /// and the caveats it emphasised. **Nothing is invented**: documentation citing no
    /// standard lands as `assumed`, and every entry goes through the format's own
    /// provenance rules, which stop the write rather than warning (D180).
    Knowledge {
        /// The crate sources to read implementations and declarations from.
        #[arg(long, default_value = "crates")]
        crates: PathBuf,
        /// Where the per-library knowledge files live.
        #[arg(long, default_value = "crates/orbistoun-hle/data/knowledge")]
        out: PathBuf,
    },
    /// Read the character-classification tables from an obSCEne hardware capture.
    ///
    /// The values are the platform C library's own, so nothing lawful states them and the
    /// capture is the oracle. It carries its own spot-checks, and the generator refuses a
    /// table that disagrees with them.
    Ctype {
        /// Path to an obSCEne capture holding the `035-libc/getpctype` probe.
        source: PathBuf,
        /// Where to write the table.
        #[arg(long, default_value = "crates/orbistoun-libc/data/ctype.toml")]
        out: PathBuf,
    },
    Constants {
        /// Path to a FreeBSD source checkout.
        source: PathBuf,
        /// Where to write the table.
        #[arg(long, default_value = "crates/orbistoun-hle/data/abi-constants.toml")]
        out: PathBuf,
    },
    /// Solve the typed-buffer format table.
    BufferFormats {
        /// Where to write it.
        #[arg(
            long,
            default_value = "crates/orbistoun-shader/data/buffer-formats.toml"
        )]
        out: PathBuf,
    },
}

fn main() -> Result<()> {
    // A generator writes committed artefacts from data files, so what it decided on the way is
    // worth being able to ask about after the fact.
    let _logging = oops_log::Logging::new("orbistoun-gen")
        .build(orbistoun_env::build::line_static())
        .init();

    let cli = Cli::parse();

    let source = match &cli.transcript {
        Some(dir) => assembler::Source::Transcript(dir.clone()),
        None => assembler::Source::compute(),
    };

    match &cli.command {
        Command::Target { field } => {
            let value = target::field(field).with_context(|| {
                format!(
                    "unknown field `{field}`; one of {}",
                    target::FIELDS.join(", ")
                )
            })?;
            println!("{value}");
            Ok(())
        }
        Command::Knowledge { crates, out } => run_knowledge(crates, out, cli.dry_run),
        Command::Hardware { records, out } => run_hardware(records, out, cli.dry_run),
        Command::Measurements { records, out } => run_measurements(records, out, cli.dry_run),
        Command::Ctype { source, out } => {
            let rendered = ctype::run(source)?;
            emit(&rendered, out, cli.dry_run)
        }
        Command::Constants { source, out } => {
            let rendered = constants::run(source)?;
            emit(&rendered, out, cli.dry_run)
        }
        Command::BufferFormats { out } => {
            let (solved, rendered) = buffer_formats::run(&source, cli.record.as_deref())?;
            eprint!("{}", buffer_formats::render_report(&solved));
            emit(&rendered, out, cli.dry_run)
        }
        Command::Fixtures {
            sources,
            out,
            encodings,
            mnemonics,
        } => {
            let table = table::load(encodings)?;
            let report = fixtures::run(
                &source,
                sources,
                out,
                &table,
                cli.record.as_deref(),
                cli.dry_run,
            )?;
            let rendered = fixtures::render_mnemonics(&report.observed);
            if cli.dry_run {
                print!("{rendered}");
            } else {
                std::fs::write(mnemonics, &rendered)
                    .with_context(|| format!("writing {}", mnemonics.display()))?;
            }
            eprint!("{}", fixtures::render_report(&report));
            // A conflict means the classification is wrong, which makes every name in the
            // table suspect rather than one of them. Not something to exit zero on.
            anyhow::ensure!(
                report.conflicts.is_empty(),
                "{} classification conflict(s) - see above",
                report.conflicts.len()
            );
            Ok(())
        }
        Command::Operands {
            probes,
            encodings,
            codes,
            out,
        } => {
            let table = table::load(encodings)?;
            let named = operands::load_named_codes(codes)?;
            let report = operands::run(&source, probes, &table, &named, cli.record.as_deref())?;
            eprint!("{}", operands::render_report(&report));
            emit(&operands::render(&report.solved), out, cli.dry_run)
        }
        Command::Encodings { families } => {
            let report = encodings::run(&source, families, cli.record.as_deref())?;
            print!("{}", encodings::render(&report));
            // Non-zero when anything went unsolved. A generator that reports a problem and
            // exits successfully is one a script will happily ignore.
            anyhow::ensure!(
                report.problems.is_empty(),
                "{} family problem(s) - see above",
                report.problems.len()
            );
            Ok(())
        }
    }
}

/// Derives the missing knowledge entries and writes them, or shows what it would write.
///
/// **Refuses on a provenance fault rather than warning.** The format's rules are the reason
/// this is safe to run unattended: an entry claiming an outside source without citing one is
/// rejected by [`orbistoun_hle::knowledge::KnowledgeFile::merge`], and one rejected entry
/// stops the whole write. Reporting and carrying on would leave the file in a state nobody
/// chose (D180).
fn run_knowledge(crates: &std::path::Path, out: &std::path::Path, dry_run: bool) -> Result<()> {
    let sources = rust_sources(crates)?;
    anyhow::ensure!(!sources.is_empty(), "no sources under {}", crates.display());

    let existing = load_knowledge(out)?;
    let derived = knowledge::derive(&sources, &existing, &orbistoun_nid::today());

    for symbol in &derived.undeclared {
        eprintln!("  {symbol}: implemented but declared by no module, so nothing reaches it");
    }
    for symbol in &derived.undocumented {
        eprintln!("  {symbol}: implemented with no doc comment, so there is nothing to derive");
    }
    anyhow::ensure!(
        derived.faults.is_empty(),
        "{} entr(ies) would not be admissible, so nothing was written:\n  {}",
        derived.faults.len(),
        derived.faults.join("\n  ")
    );

    eprintln!("derived {} entr(ies)", derived.added);
    write_knowledge(&derived.files, out, dry_run)
}

/// Attaches conformance-run observations to the entries for the functions they exercised.
///
/// **Refuses on a provenance fault**, exactly as the derivation does: the knowledge format's
/// own rules decide admissibility, and one rejected entry stops the write rather than leaving
/// the files in a state nobody chose (D180).
fn run_hardware(records: &std::path::Path, out: &std::path::Path, dry_run: bool) -> Result<()> {
    let mut observations = Vec::new();
    let mut seen = 0_usize;
    for (name, text) in captures(records)? {
        let found = hardware::observations_in(&text, &name);
        if found.is_empty() {
            continue;
        }
        seen += 1;
        eprintln!("  {name}: {} observation(s)", found.len());
        observations.extend(found);
    }
    anyhow::ensure!(
        seen > 0,
        "no capture in {} carried a joinable record",
        records.display()
    );

    let observations = hardware::fold(observations);
    let existing = load_knowledge(out)?;
    let ingested = hardware::ingest(&observations, &existing, &orbistoun_nid::today());
    for symbol in &ingested.unknown {
        eprintln!("  {symbol}: observed on hardware, but nothing here records it");
    }
    anyhow::ensure!(
        ingested.faults.is_empty(),
        "{} entr(ies) would not be admissible, so nothing was written:\n  {}",
        ingested.faults.len(),
        ingested.faults.join("\n  ")
    );
    eprintln!(
        "attached {} observation(s), {} of which the two runs disagreed on",
        ingested.attached, ingested.varying
    );
    write_knowledge(&ingested.files, out, dry_run)
}

/// Writes the measurement table, or shows it.
fn run_measurements(records: &std::path::Path, out: &std::path::Path, dry_run: bool) -> Result<()> {
    let mut found = Vec::new();
    for (name, text) in captures(records)? {
        let rows = hardware::measurements_in(&text, &name);
        if rows.is_empty() {
            continue;
        }
        eprintln!("  {name}: {} measurement(s)", rows.len());
        found.extend(rows);
    }
    anyhow::ensure!(
        !found.is_empty(),
        "no capture in {} carried a measure record",
        records.display()
    );
    let table = hardware::table(&hardware::fold(found));
    let constant = table.constants().count();
    eprintln!(
        "{} distinct measurement(s), {constant} constant across every run that took them",
        table.measurements.len()
    );
    let rendered = table.render().context("rendering the measurement table")?;
    emit(&rendered, &out.to_path_buf(), dry_run)
}

/// Every capture file under a directory, as (name, contents).
fn captures(records: &std::path::Path) -> Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    for entry in
        std::fs::read_dir(records).with_context(|| format!("reading {}", records.display()))?
    {
        let path = entry?.path();
        if path.extension().is_none_or(|e| e != "txt") {
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let name = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        out.push((name, text));
    }
    Ok(out)
}

/// Reads every per-library knowledge file, keyed by the library it declares.
fn load_knowledge(
    out: &std::path::Path,
) -> Result<std::collections::BTreeMap<String, orbistoun_hle::knowledge::KnowledgeFile>> {
    let mut existing = std::collections::BTreeMap::new();
    for entry in std::fs::read_dir(out).with_context(|| format!("reading {}", out.display()))? {
        let path = entry?.path();
        if path.extension().is_none_or(|e| e != "toml") {
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let file = orbistoun_hle::knowledge::KnowledgeFile::parse(&text)
            .with_context(|| format!("parsing {}", path.display()))?;
        existing.insert(file.library.clone(), file);
    }
    Ok(existing)
}

/// Writes the knowledge files back, or shows what would be written.
fn write_knowledge(
    files: &std::collections::BTreeMap<String, orbistoun_hle::knowledge::KnowledgeFile>,
    out: &std::path::Path,
    dry_run: bool,
) -> Result<()> {
    for (library, file) in files {
        let rendered = file
            .render()
            .with_context(|| format!("rendering {library}"))?;
        let path = out.join(format!("{library}.toml"));
        if dry_run {
            println!("--- {}", path.display());
            print!("{rendered}");
            continue;
        }
        std::fs::write(&path, rendered).with_context(|| format!("writing {}", path.display()))?;
    }
    if !dry_run {
        eprintln!("wrote {}", out.display());
    }
    Ok(())
}

/// Every `.rs` file under a directory, read into memory.
///
/// Generated sources are skipped: a table emitted by another generator carries no
/// documentation worth deriving from, and reading it back would record a machine's output as
/// though somebody had established it.
fn rust_sources(root: &std::path::Path) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in
            std::fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))?
        {
            let path = entry?.path();
            if path.is_dir() {
                if path.file_name().is_some_and(|n| n == "target") {
                    continue;
                }
                stack.push(path);
                continue;
            }
            if path.extension().is_some_and(|e| e == "rs") {
                out.push(
                    std::fs::read_to_string(&path)
                        .with_context(|| format!("reading {}", path.display()))?,
                );
            }
        }
    }
    Ok(out)
}

/// Writes a generated table, or shows it.
fn emit(rendered: &str, out: &PathBuf, dry_run: bool) -> Result<()> {
    if dry_run {
        print!("{rendered}");
        return Ok(());
    }
    std::fs::write(out, rendered).with_context(|| format!("writing {}", out.display()))?;
    eprintln!("wrote {}", out.display());
    Ok(())
}
