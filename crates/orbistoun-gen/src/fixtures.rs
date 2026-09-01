//! Generating differential-test fixtures for the shader decoder.
//!
//! `crates/orbistoun-shader/tests/fixtures/` and `data/mnemonics.toml`
//!
//! # Why this exists
//!
//! The instruction encoding table is transcribed from a published specification (D085), and
//! a wrong row in it does not fail to build - it silently mis-decodes. Until real shaders
//! are captured from a real title, which is a long way off, there is nothing to check it
//! against.
//!
//! LLVM ships a code generator for this GPU architecture, so shaders whose contents we
//! specified can be produced on demand, and its disassembler then says exactly where each
//! instruction begins. That is a reference decoder to diff against, available today, with no
//! console and no title involved.
//!
//! # The provenance line, drawn deliberately
//!
//! The disassembler is used to **detect** that an entry is wrong. Correcting it is done from
//! the published AMD document, not by reading LLVM's tables.
//!
//! Differential testing against another implementation is ordinary engineering. Reading that
//! implementation's source to source the right value is deriving from it, and this project
//! draws that line everywhere else too. Worth holding here because the temptation is
//! strongest exactly where the answer is hardest to look up.
//!
//! # Fixtures are committed, so LLVM is not a test dependency
//!
//! Output goes into the repository and the differential test reads it there. This runs when
//! someone wants new coverage; the test runs everywhere, forever, including on machines with
//! no GPU toolchain at all.
//!
//! The binaries are compiled from source in `tools/shader-fixtures/` that this project
//! wrote. Generated, never extracted - the same rule every other fixture here follows.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::assembler::Source;
use crate::table::{self, Encoding};
use crate::target::{MATTR, MCPU, TRIPLE};

/// One instruction, as the reference disassembler described it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Instruction {
    /// Byte offset into the shader.
    pub(crate) offset: u64,
    /// Its encoding words.
    pub(crate) words: Vec<u32>,
    /// The mnemonic the reference printed.
    pub(crate) mnemonic: String,
    /// Operands **as the reference printed them, verbatim**.
    ///
    /// Normalisation belongs on the reading side. Storing a cleaned-up version would bake
    /// one comparison strategy into the fixture and lose whatever the next one needs.
    pub(crate) operands: String,
}

impl Instruction {
    /// Length in bytes.
    #[must_use]
    pub(crate) fn length(&self) -> usize {
        self.words.len() * 4
    }
}

/// The target a source declares, or the default.
///
/// Read from the source rather than fixed by the generator so a fixture is
/// self-describing - the file that needs a different environment is the file that says so.
/// Compute shaders use the HSA environment; graphics shaders must declare the graphics one,
/// because HSA refuses them outright, which surfaced as an LLVM crash rather than a
/// diagnostic and cost a run to find.
#[must_use]
pub(crate) fn triple_for(text: &str) -> String {
    crate::patterns::declared_triple(text).unwrap_or_else(|| TRIPLE.to_owned())
}

/// Reads `llvm-objdump` output into instructions.
///
/// The encoding words are taken from the trailing comment rather than from a separate binary
/// dump, so the bytes and the expectations are guaranteed to describe the same instructions -
/// two files produced from one parse cannot disagree with each other.
#[must_use]
pub(crate) fn parse_disassembly(text: &str) -> Vec<Instruction> {
    text.lines()
        .filter_map(crate::patterns::objdump_line)
        .collect()
}

/// Every instruction must begin where the previous one ended.
///
/// If this fails, the **reference** has been misparsed - a line missed, most likely - and the
/// fixture would encode a gap as though it were real. A fixture with a hole in it teaches
/// the decoder to be wrong.
pub(crate) fn check_contiguous(name: &str, instructions: &[Instruction]) -> Result<()> {
    let mut expected = 0_u64;
    for instruction in instructions {
        anyhow::ensure!(
            instruction.offset == expected,
            "{name}: gap in parsed disassembly at {:#x}, expected {expected:#x} - the parser missed a line",
            instruction.offset
        );
        expected += instruction.length() as u64;
    }
    Ok(())
}

/// The instruction stream, exactly as it sits in a shader binary.
#[must_use]
pub(crate) fn binary_of(instructions: &[Instruction]) -> Vec<u8> {
    instructions
        .iter()
        .flat_map(|i| i.words.iter().flat_map(|w| w.to_le_bytes()))
        .collect()
}

/// What the reference says is in it.
///
/// One line per instruction so a mismatch in the test can name an offset rather than say the
/// file differs.
#[must_use]
pub(crate) fn render_expectations(
    source_name: &str,
    triple: &str,
    instructions: &[Instruction],
) -> String {
    let mut lines = vec![
        "# offset  length  mnemonic  operands-as-the-reference-printed-them".to_owned(),
        format!("# generated by `orbistoun-gen fixtures` from {source_name}"),
        format!("# target {triple} {MCPU} {MATTR}"),
    ];
    for i in instructions {
        lines.push(
            format!(
                "{:#08x} {} {} {}",
                i.offset,
                i.length(),
                i.mnemonic,
                i.operands
            )
            .trim_end()
            .to_owned(),
        );
    }
    lines.join("\n") + "\n"
}

/// Emits the opcode-name table from what the reference actually said.
///
/// Every entry here was observed: a real compiler emitted the instruction and a real
/// disassembler named it. That makes this table verified by construction, and it grows only
/// as the fixture set grows - which is the right constraint, because an unobserved name is a
/// guess and this project does not ship those.
#[must_use]
pub(crate) fn render_mnemonics(observed: &BTreeMap<(String, u32), String>) -> String {
    let mut lines: Vec<String> = [
        "# Instruction names, for reports.",
        "#",
        "# Generated by `orbistoun-gen fixtures` - do not edit by hand.",
        "#",
        "# Every entry was observed: a compiler emitted the instruction and a reference",
        "# disassembler named it, so this table is verified by construction. It covers",
        "# only what the fixtures exercise, which is deliberate - an unobserved name is a",
        "# guess, and a wrong name in a report sends someone to the wrong instruction.",
        "#",
        "# The translator dispatches on these. It used to dispatch on opcode numbers,",
        "# which bind silently to a different instruction on a different architecture",
        "# generation; a name that moves is a name the table cannot find, which is a",
        "# refusal rather than a wrong answer (D139). An instruction with no entry has",
        "# no translation and reports as its family and opcode number.",
        "#",
        "# The target is declared so the loader can refuse a half-retargeted table set.",
        "",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    lines.push(format!("target = \"{MCPU}\""));
    lines.push(String::new());

    for ((family, opcode), mnemonic) in observed {
        lines.push("[[mnemonic]]".to_owned());
        lines.push(format!("family = \"{family}\""));
        lines.push(format!("opcode = {opcode}"));
        lines.push(format!("name = \"{mnemonic}\""));
        lines.push(String::new());
    }
    lines.join("\n")
}

/// What a whole run established.
#[derive(Debug, Default)]
pub(crate) struct Report {
    /// Per source: how many instructions and how many bytes.
    pub(crate) built: Vec<(String, usize, usize)>,
    /// Sources the toolchain would not build, and why.
    ///
    /// **Named rather than merely counted**: each one is an encoding family left unverified,
    /// which is a gap in what the differential test can prove.
    pub(crate) skipped: Vec<(String, String)>,
    /// Classification disagreements - the same opcode named two ways.
    pub(crate) conflicts: Vec<String>,
    /// Every opcode a fixture named.
    pub(crate) observed: BTreeMap<(String, u32), String>,
}

/// Records one instruction's name against its classification.
///
/// **First observation wins.** A later fixture naming the same opcode differently would mean
/// the classification is wrong, and quietly overwriting would hide it - so it is reported.
pub(crate) fn observe(report: &mut Report, instruction: &Instruction, encodings: &[Encoding]) {
    let Some(key) = table::classify(&instruction.words, encodings) else {
        return;
    };
    match report.observed.get(&key) {
        Some(previous) if previous != &instruction.mnemonic => {
            report.conflicts.push(format!(
                "{}:{} named both {previous} and {} - classification is wrong",
                key.0, key.1, instruction.mnemonic
            ));
        }
        Some(_) => {}
        None => {
            report.observed.insert(key, instruction.mnemonic.clone());
        }
    }
}

/// Builds one source and disassembles it, or replays a recording of having done so.
fn disassembly_of(
    source: &Source,
    stem: &str,
    path: &Path,
    triple: &str,
    record: Option<&Path>,
) -> Result<String> {
    let key = format!("fixtures-{stem}");
    let text = match source {
        Source::Transcript(dir) => std::fs::read_to_string(dir.join(format!("{key}.out")))
            .with_context(|| format!("reading the recording for {stem}"))?,
        Source::Llvm { .. } => build_and_disassemble(path, triple)?,
    };
    if let Some(dir) = record {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        std::fs::write(dir.join(format!("{key}.out")), &text)
            .with_context(|| format!("recording {stem}"))?;
    }
    Ok(text)
}

/// Two routes to the same object file, then one disassembly.
///
/// LLVM IR goes through `llc`, which is how every fixture that came from real shader source
/// is built. Assembly goes through `llvm-mc`, which exists because three encoding families -
/// SOPK, MTBUF and VINTRP - could not be reached any other way: nothing the compiler emits
/// from the IR anyone can write produces them, so they stayed transcribed-only and unverified
/// while every other family had been checked against a reference (D085).
///
/// Writing the instruction by hand is a weaker fixture than compiling one, because the
/// instruction chosen is one somebody thought of rather than one a compiler reached for. It
/// is still an enormous step up from nothing: the reference decides the bytes and the
/// boundaries, so a wrong mask, value, opcode field or length in the table fails here exactly
/// as it would for a compiled fixture.
fn build_and_disassemble(path: &Path, triple: &str) -> Result<String> {
    use std::process::Command;

    let work = std::env::temp_dir().join("orbistoun-gen-fixtures");
    std::fs::create_dir_all(&work).with_context(|| format!("creating {}", work.display()))?;
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("fixture");
    let object = work.join(format!("{stem}.o"));

    let assembly = path.extension().is_some_and(|e| e == "s");
    let built = if assembly {
        Command::new("llvm-mc")
            .args([
                format!("--triple={triple}"),
                format!("-mcpu={MCPU}"),
                format!("-mattr={MATTR}"),
            ])
            .args(["-filetype=obj"])
            .arg(path)
            .arg("-o")
            .arg(&object)
            .output()
            .context("could not run `llvm-mc`")?
    } else {
        Command::new("llc")
            .arg("-mtriple")
            .arg(triple)
            .arg("-mcpu")
            .arg(MCPU)
            .arg(format!("-mattr={MATTR}"))
            .arg("-filetype=obj")
            .arg(path)
            .arg("-o")
            .arg(&object)
            .output()
            .context("could not run `llc`")?
    };

    if !built.status.success() {
        anyhow::bail!(
            "{}",
            unbuildable_reason(&String::from_utf8_lossy(&built.stderr))
        );
    }

    let dumped = Command::new("llvm-objdump")
        .arg("-d")
        .arg(format!("--triple={triple}"))
        .arg(format!("--mcpu={MCPU}"))
        .arg(format!("--mattr={MATTR}"))
        .arg(&object)
        .output()
        .context("could not run `llvm-objdump`")?;
    anyhow::ensure!(
        dumped.status.success(),
        "llvm-objdump failed: {}",
        String::from_utf8_lossy(&dumped.stderr).trim()
    );
    Ok(String::from_utf8_lossy(&dumped.stdout).into_owned())
}

/// The most useful line of a build failure.
///
/// **On a crash the last line is a stack frame and the diagnosis is near the top**, so a real
/// `error:` line is preferred. Reporting the frame instead sent the first investigation of
/// this straight past the actual message.
#[must_use]
pub(crate) fn unbuildable_reason(stderr: &str) -> String {
    let lines: Vec<&str> = stderr.trim().lines().collect();
    lines
        .iter()
        .find(|l| l.trim_start().starts_with("error:"))
        .or_else(|| lines.first())
        .map_or_else(
            || "the toolchain refused it with no diagnostic".to_owned(),
            |l| (*l).to_owned(),
        )
}

/// Every source to build, IR first then assembly, each group sorted.
fn sources_in(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut ir = Vec::new();
    let mut asm = Vec::new();
    let read = std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))?;
    for entry in read.flatten() {
        let path = entry.path();
        match path.extension().and_then(|e| e.to_str()) {
            Some("ll") => ir.push(path),
            Some("s") => asm.push(path),
            _ => {}
        }
    }
    ir.sort();
    asm.sort();
    ir.extend(asm);
    anyhow::ensure!(!ir.is_empty(), "no sources in {}", dir.display());
    Ok(ir)
}

/// Runs the whole generator.
pub(crate) fn run(
    source: &Source,
    sources_dir: &Path,
    out_dir: &Path,
    encodings: &[Encoding],
    record: Option<&Path>,
    dry_run: bool,
) -> Result<Report> {
    let mut report = Report::default();
    std::fs::create_dir_all(out_dir).with_context(|| format!("creating {}", out_dir.display()))?;

    for path in sources_in(sources_dir)? {
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("fixture")
            .to_owned();
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let triple = triple_for(&text);

        let disassembly = match disassembly_of(source, &stem, &path, &triple, record) {
            Ok(text) => text,
            Err(reason) => {
                // **Delete whatever was there.** A skip used to leave the *previous* run's
                // fixture on disk, and after a retarget that is a file full of another
                // architecture generation's bytes with nothing marking it - the differential
                // test then compares this target's decoder against the last one's output and
                // reports the decoder as broken. Cost two real-looking failures to find.
                let stale = out_dir.join(format!("{stem}.txt"));
                let had = stale.exists();
                if !dry_run {
                    let _ = std::fs::remove_file(&stale);
                }
                report.skipped.push((
                    stem,
                    format!(
                        "{reason}{}",
                        if had {
                            " (removed a stale fixture)"
                        } else {
                            ""
                        }
                    ),
                ));
                continue;
            }
        };

        let instructions = parse_disassembly(&disassembly);
        anyhow::ensure!(
            !instructions.is_empty(),
            "{stem}: no instructions parsed - has the output format changed?"
        );
        check_contiguous(&stem, &instructions)?;

        let binary = binary_of(&instructions);
        let file_name = path.file_name().and_then(|s| s.to_str()).unwrap_or(&stem);
        let expectations = render_expectations(file_name, &triple, &instructions);
        if !dry_run {
            std::fs::write(out_dir.join(format!("{stem}.gcn")), &binary)
                .with_context(|| format!("writing {stem}.gcn"))?;
            std::fs::write(out_dir.join(format!("{stem}.txt")), &expectations)
                .with_context(|| format!("writing {stem}.txt"))?;
        }

        for instruction in &instructions {
            observe(&mut report, instruction, encodings);
        }
        report.built.push((stem, instructions.len(), binary.len()));
    }

    // **A run that built nothing is a failure, not an empty success.**
    //
    // Without a toolchain every source is skipped, and the skip path has already deleted
    // each `.txt`. Remove this and the run goes on to write a `mnemonics.toml` containing no
    // mnemonics over the committed one, print "0 fixtures", and exit zero - destroying the
    // reference output the differential suite exists to compare against, on any machine
    // without an AMDGPU-enabled LLVM, which is most of them.
    anyhow::ensure!(
        !report.built.is_empty(),
        concat!(
            "no source built - refusing to write an empty table over the committed one. ",
            "This usually means llc/llvm-mc/llvm-objdump are missing or lack the AMDGPU ",
            "target; `tools/toolchain/setup.sh` builds a VM that has them."
        )
    );
    Ok(report)
}

/// Renders the report a person reads.
#[must_use]
pub(crate) fn render_report(report: &Report) -> String {
    let mut out = String::new();
    for (name, count, size) in &report.built {
        let _ = writeln!(out, "  {name:<10} {count:>4} instructions  {size:>5} bytes");
    }
    for conflict in &report.conflicts {
        let _ = writeln!(out, "  ! {conflict}");
    }
    let _ = writeln!(
        out,
        "{} fixtures, {} instructions, {} distinct opcodes named",
        report.built.len(),
        report.built.iter().map(|b| b.1).sum::<usize>(),
        report.observed.len()
    );
    if !report.skipped.is_empty() {
        let _ = writeln!(
            out,
            "{} source(s) the toolchain would not build:",
            report.skipped.len()
        );
        for (name, reason) in &report.skipped {
            let _ = writeln!(out, "  {name}: {reason}");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{
        Instruction, binary_of, check_contiguous, parse_disassembly, render_expectations,
        triple_for, unbuildable_reason,
    };

    fn instruction(offset: u64, words: &[u32], mnemonic: &str, operands: &str) -> Instruction {
        Instruction {
            offset,
            words: words.to_vec(),
            mnemonic: mnemonic.to_owned(),
            operands: operands.to_owned(),
        }
    }

    /// A gap means the parser missed a line, and is refused rather than written out.
    ///
    /// **A fixture with a hole teaches the decoder to be wrong**: every instruction after
    /// the gap would be expected four bytes early, forever.
    #[test]
    fn a_gap_in_the_disassembly_is_refused() {
        let good = [
            instruction(0, &[1], "a", ""),
            instruction(4, &[2, 3], "b", ""),
            instruction(12, &[4], "c", ""),
        ];
        assert!(check_contiguous("t", &good).is_ok());

        let bad = [instruction(0, &[1], "a", ""), instruction(8, &[2], "b", "")];
        let error = check_contiguous("t", &bad).expect_err("must refuse");
        assert!(error.to_string().contains("gap"), "{error}");
    }

    /// A branch's symbol reference does not stop the line being read.
    ///
    /// Anchoring the pattern to end of line seemed tidier and silently dropped **every
    /// branch instruction** - which the contiguity check then caught as a gap, because a
    /// fixture missing its control flow teaches the decoder that what follows starts early.
    #[test]
    fn a_branch_with_a_symbol_reference_is_still_read() {
        let text = "\ts_cbranch_scc1 65535   // 000000000010: BF85FFFF <control+0x2c>\n";
        let parsed = parse_disassembly(text);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].mnemonic, "s_cbranch_scc1");
        assert_eq!(parsed[0].offset, 0x10);
        assert_eq!(parsed[0].words, vec![0xBF85_FFFF]);
    }

    /// A multi-word instruction keeps every word, in order.
    #[test]
    fn a_multi_word_instruction_keeps_all_its_words() {
        let text = "\tv_mul_f32_e64 v0, s2, s3   // 00000000000C: D5080000 00000602\n";
        let parsed = parse_disassembly(text);
        assert_eq!(parsed[0].words, vec![0xD508_0000, 0x0000_0602]);
        assert_eq!(parsed[0].length(), 8);
    }

    /// Words become little-endian bytes, which is how a shader binary holds them.
    #[test]
    fn words_become_little_endian_bytes() {
        let binary = binary_of(&[instruction(0, &[0x7E00_0301], "v_mov_b32_e32", "")]);
        assert_eq!(binary, vec![0x01, 0x03, 0x00, 0x7E]);
    }

    /// A source that declares a triple gets it; one that does not gets the default.
    #[test]
    fn a_source_declares_its_own_triple() {
        assert_eq!(
            triple_for("target triple = \"amdgcn-mesa-mesa3d\"\n"),
            "amdgcn-mesa-mesa3d"
        );
        assert_eq!(
            triple_for("// target triple: amdgcn-mesa-mesa3d\n"),
            "amdgcn-mesa-mesa3d"
        );
        assert_eq!(triple_for("define void @main() {\n"), "amdgcn-amd-amdhsa");
    }

    /// A real diagnostic beats a stack frame.
    ///
    /// On a crash the last line is a frame and the diagnosis is near the top. Reporting the
    /// frame sent the first investigation of this straight past the actual message.
    #[test]
    fn a_real_diagnostic_is_preferred_to_a_stack_frame() {
        let stderr = concat!(
            "PLEASE submit a bug report\n",
            "  error: unsupported instruction for this target\n",
            " #0 0x00007f llvm::sys::PrintStackTrace\n"
        );
        assert_eq!(
            unbuildable_reason(stderr),
            "  error: unsupported instruction for this target"
        );
        // With no `error:` line at all, the first line is better than nothing.
        assert_eq!(
            unbuildable_reason("something went wrong\nframe\n"),
            "something went wrong"
        );
    }

    /// The expectations file is shaped exactly as the differential test reads it.
    #[test]
    fn the_expectations_file_is_stable() {
        let text = render_expectations(
            "arith.ll",
            "amdgcn-amd-amdhsa",
            &[instruction(
                0,
                &[1, 2],
                "s_load_dwordx4",
                "s[0:3], s[4:5], null",
            )],
        );
        assert!(text.starts_with("# offset  length  mnemonic"));
        assert!(text.contains("0x000000 8 s_load_dwordx4 s[0:3], s[4:5], null"));
        assert!(text.ends_with('\n'));
    }

    /// An instruction with no operands leaves no trailing space.
    ///
    /// A trailing space is invisible in a diff and changes the bytes of a committed file.
    #[test]
    fn an_instruction_with_no_operands_has_no_trailing_space() {
        let text = render_expectations(
            "t.ll",
            "amdgcn-amd-amdhsa",
            &[instruction(0, &[1], "s_endpgm", "")],
        );
        assert!(text.contains("0x000000 4 s_endpgm\n"), "{text}");
    }
}
