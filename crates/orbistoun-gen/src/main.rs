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
mod encodings;
mod fixtures;
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
