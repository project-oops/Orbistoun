//! Offline generators for the committed data tables.
//!
//! Not on the build, test or run path of anything that executes a guest: these produce the
//! committed `.toml` files the decoder and other crates read. A live run needs `llvm-mc`
//! with the `AMDGPU` target, which `tools/toolchain/setup.sh` builds a VM for, so every
//! generator gets its bytes through [`assembler`], which either invokes `llvm-mc` or
//! replays a committed recording (D209). Replay needs nothing installed, so the solvers
//! are tested everywhere.

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
    /// Needs no toolchain: a recording taken once with an `AMDGPU`-enabled LLVM and
    /// committed lets the solver re-run and its output be diffed anywhere, including CI.
    #[arg(long, global = true, value_name = "DIR")]
    transcript: Option<PathBuf>,

    /// Record every assembler invocation into this directory, for later replay.
    #[arg(long, global = true, value_name = "DIR")]
    record: Option<PathBuf>,

    /// Print what would be written instead of writing it.
    ///
    /// Generate and diff against what is committed before overwriting.
    #[arg(long, global = true)]
    dry_run: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Print one field of the target the generators assemble for.
    ///
    /// So a shell script reads the same target the generators do, rather than hardcoding
    /// one (D139).
    Target {
        /// One of `mcpu`, `mattr`, `triple`, `graphics-triple`.
        field: String,
    },
    /// Solve each encoding family's identifying bits from assembled samples.
    ///
    /// Reports rather than writes, because `data/encodings.toml` carries hand-maintained
    /// reasoning and citations beside the numbers this solves.
    Encodings {
        /// Directory of per-family probe files.
        #[arg(long, default_value = "tools/shader-fixtures/families")]
        families: PathBuf,
    },
    /// Regenerate the differential-test fixtures and the mnemonic table.
    ///
    /// Writes `.gcn` and `.txt` per source, plus `data/mnemonics.toml` from what the
    /// reference disassembler named. Both are committed, so LLVM is not a test dependency.
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
    /// Write the committed table of what conformance runs measured.
    ///
    /// The `measure` records carry subject, condition, observation and kind, enough for an
    /// assertion to name one. A measurement every run agreed on is marked constant; one they
    /// disagreed on is kept and marked not.
    Measurements {
        /// A directory of capture files, in the sibling conformance-probe repository.
        ///
        /// Repeatable, all read into one table, because `constant` requires every run that
        /// took the measurement to agree. A missing directory is skipped with a warning,
        /// since the defaults name a sibling checkout.
        #[arg(
            long,
            default_values = [
                "../obscene/data/hardware",
                "../obscene/reports/hardware",
                "../obscene/reports/archive/hardware",
            ]
        )]
        records: Vec<PathBuf>,
        /// Where the generated table goes.
        #[arg(long, default_value = "crates/orbistoun-hle/data/hardware.toml")]
        out: PathBuf,
    },
    /// Attach conformance-run observations to the functions they exercised.
    ///
    /// Joins `try` and `res` on their check id and records each outcome as an edge case on
    /// the function it names, quoted, not interpreted: the value's meaning lives in the
    /// check, and not every value is a return value.
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
    /// that implements it and records its purpose, the specification it cites and its
    /// caveats. Documentation citing no standard lands as `assumed`, and the format's
    /// provenance rules stop the write rather than warning (D180).
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
    /// The values are the platform C library's own, so the capture is the oracle. The
    /// generator refuses a table that disagrees with the capture's own spot-checks.
    Ctype {
        /// Path to an obSCEne capture holding the `035-libc/getpctype` probe.
        source: PathBuf,
        /// Where to write the table.
        #[arg(long, default_value = "crates/orbistoun-libc/data/ctype.toml")]
        out: PathBuf,
    },
    /// Harvest ABI constants from a FreeBSD source checkout (D352).
    ///
    /// Implementing needs numbers: `AF_INET` cannot be mapped onto a host socket by
    /// guessing it. Only headers are read, so a sparse checkout is enough:
    ///
    ///     git clone --filter=blob:none --sparse https://github.com/freebsd/freebsd-src
    ///     cd freebsd-src
    ///     git sparse-checkout set lib/libc lib/libsys lib/libthr lib/libutil lib/msun
    ///     git sparse-checkout add sys/sys sys/netinet include
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
    // A generator writes committed artefacts, so its decisions are logged.
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
            // A conflict means the classification is wrong and every name is suspect.
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
            // Non-zero when anything went unsolved, so a script cannot ignore it.
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
/// Refuses on a provenance fault rather than warning: an entry claiming an outside source
/// without citing one is rejected by [`orbistoun_hle::knowledge::KnowledgeFile::merge`],
/// and one rejected entry stops the whole write (D180).
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
/// Refuses on a provenance fault, as the derivation does: one rejected entry stops the
/// write (D180).
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
fn run_measurements(records: &[PathBuf], out: &std::path::Path, dry_run: bool) -> Result<()> {
    let mut found = Vec::new();
    let mut read = 0_usize;
    for directory in records {
        if !directory.is_dir() {
            eprintln!("  {}: no such directory - skipped", directory.display());
            continue;
        }
        read += 1;
        for (name, text) in captures(directory)? {
            let rows = hardware::measurements_in(&text, &name);
            if rows.is_empty() {
                continue;
            }
            eprintln!("  {name}: {} measurement(s)", rows.len());
            found.extend(rows);
        }
    }
    anyhow::ensure!(
        read > 0,
        "none of the {} directory/ies named exist - nothing was read",
        records.len()
    );
    anyhow::ensure!(
        !found.is_empty(),
        "no capture in the {read} directory/ies read carried a measure record"
    );

    // What is already committed is folded in beside what was read: report directories are
    // overwritten, and dropping an earlier batch could turn a disputed measurement constant.
    let carried = match std::fs::read_to_string(out) {
        Ok(text) => {
            let table: orbistoun_hle::hardware::Measurements =
                toml::from_str(&text).with_context(|| format!("parsing {}", out.display()))?;
            let rows = hardware::observations_in_table(&table);
            eprintln!(
                "  {}: {} measurement(s) already recorded",
                out.display(),
                rows.len()
            );
            rows
        }
        // Absent is the ordinary first run; any other error is real and must not start the
        // table from nothing.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(e).with_context(|| format!("reading {}", out.display())),
    };
    found.extend(carried);
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
///
/// Two extensions, because a session transcript is `.txt` and a report is `.obs.log`.
/// Anything else (READMEs, module dumps, `.sprx` files) is skipped rather than read.
fn captures(records: &std::path::Path) -> Result<Vec<(String, String)>> {
    /// What a capture file is called.
    const CAPTURE_SUFFIXES: &[&str] = &[".txt", ".obs.log"];

    let mut out = Vec::new();
    for entry in
        std::fs::read_dir(records).with_context(|| format!("reading {}", records.display()))?
    {
        let path = entry?.path();
        let name = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        if !CAPTURE_SUFFIXES.iter().any(|suffix| name.ends_with(suffix)) {
            continue;
        }
        // A file that is not UTF-8 text is not a transcript and not an error: it is skipped
        // and named, so the rest are still read.
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) => {
                eprintln!("  {name}: not readable as text - skipped ({e})");
                continue;
            }
        };
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
/// Generated sources are skipped, so a generator's output is never recorded as though
/// somebody had established it.
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
