//! Getting encodings out of the reference assembler, live or from a recording (D209).
//!
//! The tables under `crates/orbistoun-shader/data/` are solved from assembled bytes
//! (D085), and a live run needs `llvm-mc` with the `AMDGPU` target. [`Source::Llvm`] shells
//! out; [`Source::Transcript`] replays a committed recording and needs nothing installed,
//! so the solvers and the committed tables are checked in CI. Recording is an explicit act
//! (`--record`), not a cache that decides for itself when it is stale.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::target::{MATTR, MCPU};

/// What an assembler invocation produced.
///
/// Both streams, because rejections are expected when a probe file is assembled against
/// another generation, and the list of them is part of the answer (see `probes` in the
/// operand solver).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Output {
    /// Standard output: the assembled listing, with `; encoding: [..]` per line.
    pub(crate) stdout: String,
    /// Standard error: one diagnostic per refused line.
    pub(crate) stderr: String,
}

/// Where encodings come from.
#[derive(Debug, Clone)]
pub(crate) enum Source {
    /// Invoke `llvm-mc`, which must be on `PATH` and built with the `AMDGPU` target.
    Llvm {
        /// Triple to assemble for. Compute and graphics need different ones.
        triple: String,
    },
    /// Replay recordings from a directory, keyed by the name the caller asks under.
    Transcript(PathBuf),
}

impl Source {
    /// The ordinary compute source: a live assembler on the compute triple.
    #[must_use]
    pub(crate) fn compute() -> Self {
        Self::Llvm {
            triple: crate::target::TRIPLE.to_owned(),
        }
    }
}

/// Assembles `input`, or replays what a recording says it produced.
///
/// `key` names the invocation. Under [`Source::Transcript`] it selects the recording; under
/// [`Source::Llvm`] with `record` set it names the file written. The caller keeps keys
/// stable, because a renamed key stops matching its recording.
pub(crate) fn assemble(
    source: &Source,
    key: &str,
    input: &str,
    record: Option<&Path>,
) -> Result<Output> {
    let output = match source {
        Source::Llvm { triple } => run_llvm(triple, input)?,
        Source::Transcript(dir) => read_recording(dir, key)?,
    };
    if let Some(dir) = record {
        write_recording(dir, key, input, &output)?;
    }
    Ok(output)
}

/// Shells out to the reference assembler.
///
/// A non-zero exit is not an error: `llvm-mc` reports rejected lines and still emits
/// encodings for the rest, and both are wanted. Failing to run it at all is an error, named
/// as a missing toolchain.
fn run_llvm(triple: &str, input: &str) -> Result<Output> {
    use std::io::Write as _;
    use std::process::{Command, Stdio};

    let mut child = Command::new("llvm-mc")
        .arg(format!("-triple={triple}"))
        .arg("-mcpu")
        .arg(MCPU)
        .arg(format!("-mattr={MATTR}"))
        .arg("-show-encoding")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context(concat!(
            "could not run `llvm-mc`. It must be on PATH and built with the AMDGPU ",
            "target; `tools/toolchain/setup.sh` builds a VM that has it. To work without ",
            "it, replay a recording with --transcript."
        ))?;

    child
        .stdin
        .take()
        .context("llvm-mc took no stdin")?
        .write_all(input.as_bytes())
        .context("writing probes to llvm-mc")?;

    let done = child.wait_with_output().context("waiting for llvm-mc")?;
    Ok(Output {
        stdout: String::from_utf8_lossy(&done.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&done.stderr).into_owned(),
    })
}

/// Asks the reference what a sequence of words decodes to, or replays a recording.
///
/// The encoding solver's sweep needs to know which candidate words are instructions, which
/// only the disassembler answers.
pub(crate) fn disassemble(
    source: &Source,
    key: &str,
    words: &[Vec<u32>],
    record: Option<&Path>,
) -> Result<Vec<Option<String>>> {
    let input = hex_lines(words);
    let output = match source {
        Source::Llvm { triple } => run_llvm_disassemble(triple, &input)?,
        Source::Transcript(dir) => read_recording(dir, key)?,
    };
    if let Some(dir) = record {
        write_recording(dir, key, &input, &output)?;
    }
    Ok(names_of(words.len(), &output))
}

/// One comma-separated hex line per instruction, which is what `-disassemble` reads.
fn hex_lines(words: &[Vec<u32>]) -> String {
    let mut out = String::new();
    for instruction in words {
        let octets: Vec<String> = instruction
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .map(|b| format!("{b:#04x}"))
            .collect();
        out.push_str(&octets.join(","));
        out.push('\n');
    }
    out
}

/// Pairs disassembled names back to the words that produced them.
///
/// A rejected line produces no output line, so the refused line numbers come from the
/// diagnostics and the names fill the gaps between them in order.
fn names_of(count: usize, output: &Output) -> Vec<Option<String>> {
    let refused: std::collections::BTreeSet<usize> = output
        .stderr
        .lines()
        .filter_map(crate::patterns::invalid_instruction)
        .collect();
    let mut emitted: std::collections::VecDeque<String> = output
        .stdout
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('.'))
        .filter_map(|l| l.split_whitespace().next().map(str::to_owned))
        .collect();

    let mut named = Vec::with_capacity(count);
    for index in 0..count {
        if refused.contains(&(index + 1)) || emitted.is_empty() {
            named.push(None);
        } else {
            named.push(emitted.pop_front());
        }
    }
    if !emitted.is_empty() {
        // More names than places means the accounting is wrong; no names beats misaligned
        // ones.
        return vec![None; count];
    }
    named
}

/// Shells out to the reference disassembler.
fn run_llvm_disassemble(triple: &str, input: &str) -> Result<Output> {
    use std::io::Write as _;
    use std::process::{Command, Stdio};

    let mut child = Command::new("llvm-mc")
        .arg("-disassemble")
        .arg(format!("-triple={triple}"))
        .arg("-mcpu")
        .arg(MCPU)
        .arg(format!("-mattr={MATTR}"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context(concat!(
            "could not run `llvm-mc -disassemble`. It must be on PATH and built with the ",
            "AMDGPU target; `tools/toolchain/setup.sh` builds a VM that has it."
        ))?;
    child
        .stdin
        .take()
        .context("llvm-mc took no stdin")?
        .write_all(input.as_bytes())
        .context("writing words to llvm-mc")?;
    let done = child.wait_with_output().context("waiting for llvm-mc")?;
    Ok(Output {
        stdout: String::from_utf8_lossy(&done.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&done.stderr).into_owned(),
    })
}

/// The two files one recording is made of.
fn paths(dir: &Path, key: &str) -> (PathBuf, PathBuf, PathBuf) {
    (
        dir.join(format!("{key}.in")),
        dir.join(format!("{key}.out")),
        dir.join(format!("{key}.err")),
    )
}

/// Replays a recording.
///
/// Reads the answers without checking the recorded input: the solvers hold the input, so
/// each calls [`check_recording`] before replaying. Without that check, a changed probe
/// list would get old answers and produce a wrong table.
fn read_recording(dir: &Path, key: &str) -> Result<Output> {
    let (input_path, out_path, err_path) = paths(dir, key);
    let stdout = std::fs::read_to_string(&out_path)
        .with_context(|| format!("reading the recording at {}", out_path.display()))?;
    // A missing diagnostics file is tolerated: a run that rejected nothing has none.
    let stderr = std::fs::read_to_string(&err_path).unwrap_or_default();
    let _ = input_path;
    Ok(Output { stdout, stderr })
}

/// Checks a replayed recording was taken for the probes being asked about now.
pub(crate) fn check_recording(dir: &Path, key: &str, input: &str) -> Result<()> {
    let (input_path, _, _) = paths(dir, key);
    let recorded = std::fs::read_to_string(&input_path)
        .with_context(|| format!("reading the recorded input at {}", input_path.display()))?;
    anyhow::ensure!(
        recorded.replace("\r\n", "\n") == input.replace("\r\n", "\n"),
        concat!(
            "the recording at {} was taken for different probes than the ones being ",
            "solved now - re-record it with --record, on a machine that has llvm-mc"
        ),
        input_path.display()
    );
    Ok(())
}

/// Writes a recording out, so it can be committed and replayed with no toolchain.
fn write_recording(dir: &Path, key: &str, input: &str, output: &Output) -> Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let (input_path, out_path, err_path) = paths(dir, key);
    std::fs::write(&input_path, input)
        .with_context(|| format!("writing {}", input_path.display()))?;
    std::fs::write(&out_path, &output.stdout)
        .with_context(|| format!("writing {}", out_path.display()))?;
    std::fs::write(&err_path, &output.stderr)
        .with_context(|| format!("writing {}", err_path.display()))?;
    Ok(())
}

/// One instruction the assembler accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Sample {
    /// The mnemonic, as the reference printed it.
    pub(crate) mnemonic: String,
    /// Operands, split into individual tokens.
    pub(crate) operands: Vec<String>,
    /// The instruction's little-endian 32-bit words.
    pub(crate) words: Vec<u32>,
    /// The operand text exactly as printed, before splitting.
    pub(crate) printed: String,
}

/// One probe the assembler refused, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Rejection {
    /// One-based line number in the input.
    pub(crate) line: usize,
    /// The probe text, read from the input rather than the diagnostic.
    pub(crate) probe: String,
    /// What the assembler objected to.
    pub(crate) why: String,
}

/// Everything one invocation established.
#[derive(Debug, Clone, Default)]
pub(crate) struct Assembled {
    /// Instructions that assembled, in output order.
    pub(crate) samples: Vec<Sample>,
    /// Probes that did not, in diagnostic order.
    pub(crate) rejected: Vec<Rejection>,
    /// For each sample, the zero-based input line it came from.
    ///
    /// Carried because a field at its default prints no modifier, so the listing does not
    /// say what was asked for. Pairing by position must account for refusals, or every
    /// later result shifts by one.
    pub(crate) from_line: Vec<usize>,
}

/// Parses an assembler invocation into samples and rejections.
pub(crate) fn parse(input: &str, output: &Output) -> Assembled {
    let probe_lines: Vec<&str> = input.lines().collect();
    let mut rejected = Vec::new();
    let mut refused_lines = std::collections::BTreeSet::new();

    for line in output.stderr.lines() {
        let Some((number, why)) = crate::patterns::rejection(line) else {
            continue;
        };
        let index = number.saturating_sub(1);
        refused_lines.insert(index);
        rejected.push(Rejection {
            line: number,
            probe: probe_lines.get(index).map_or("?", |l| l.trim()).to_owned(),
            why,
        });
    }

    let survivors: Vec<usize> = (0..probe_lines.len())
        .filter(|i| !refused_lines.contains(i))
        .collect();

    let mut samples = Vec::new();
    let mut from_line = Vec::new();
    for line in output.stdout.lines() {
        let Some(parsed) = crate::patterns::assembled(line) else {
            continue;
        };
        from_line.push(survivors.get(samples.len()).copied().unwrap_or(usize::MAX));
        samples.push(parsed);
    }

    Assembled {
        samples,
        rejected,
        from_line,
    }
}

/// A short, stable key for an arbitrary probe.
///
/// Content-derived, so a recording matches what was asked rather than the order it was
/// asked in. SHA-1 because it is already a dependency and is deterministic across
/// platforms and releases, which `DefaultHasher` is not.
pub(crate) fn key_for(input: &str) -> String {
    use sha1::Digest as _;
    use std::fmt::Write as _;

    let mut hasher = sha1::Sha1::new();
    hasher.update(input.as_bytes());
    let digest = hasher.finalize();
    digest[..6].iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

#[cfg(test)]
mod tests {
    use super::{Output, check_recording, parse, write_recording};

    /// A refusal shifts nothing.
    ///
    /// Outputs pair to inputs by position, so an unaccounted rejection would shift every
    /// later sample onto the wrong line.
    #[test]
    fn a_rejected_probe_does_not_shift_the_ones_after_it() {
        let input = "first_one v0, v1\nrefused_one v0\nthird_one v2, v3\n";
        let output = Output {
            stdout: concat!(
                "\tfirst_one v0, v1                        ; encoding: [0x01,0x03,0x00,0x7e]\n",
                "\tthird_one v2, v3                        ; encoding: [0x03,0x05,0x04,0x7e]\n",
            )
            .to_owned(),
            stderr: "<stdin>:2:1: error: invalid instruction\n".to_owned(),
        };
        let assembled = parse(input, &output);
        assert_eq!(assembled.samples.len(), 2);
        assert_eq!(assembled.rejected.len(), 1);
        assert_eq!(assembled.rejected[0].probe, "refused_one v0");
        // Zero-based: the first survivor is line 0, the second is line 2, not line 1.
        assert_eq!(assembled.from_line, vec![0, 2]);
    }

    /// Bytes become little-endian words.
    #[test]
    fn octets_become_little_endian_words() {
        let output = Output {
            stdout: "\tv_mov_b32_e32 v0, v1  ; encoding: [0x01,0x03,0x00,0x7e]\n".to_owned(),
            stderr: String::new(),
        };
        let assembled = parse("v_mov_b32_e32 v0, v1\n", &output);
        assert_eq!(assembled.samples[0].words, vec![0x7e00_0301]);
        assert_eq!(assembled.samples[0].mnemonic, "v_mov_b32_e32");
    }

    /// A listing line with no encoding is not a sample.
    ///
    /// Directives, labels and blank lines appear in the listing; counting one would shift
    /// the input pairing.
    #[test]
    fn a_line_without_an_encoding_is_not_a_sample() {
        let output = Output {
            stdout: "\t.text\n\t.globl main\n".to_owned(),
            stderr: String::new(),
        };
        assert!(parse("", &output).samples.is_empty());
    }

    /// The words `bin/orbistoun` looks for to tell a stale recording from a broken replay.
    ///
    /// Duplicated across a language boundary, since the shell cannot call this crate, and
    /// pinned from both ends below so a reworded message cannot break the shell's `grep`.
    const STALE_MARKER: &str = "was taken for different probes";

    /// Writes a recording for `key` whose input is `input`.
    fn recorded(dir: &std::path::Path, key: &str, input: &str) {
        let output = Output {
            stdout: String::new(),
            stderr: String::new(),
        };
        write_recording(dir, key, input, &output).expect("writing the recording");
    }

    /// The ordinary case: the probes have not moved, so replaying is sound.
    #[test]
    fn a_recording_taken_for_these_probes_is_accepted() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        recorded(
            dir.path(),
            "operands-memory",
            "s_load_dword s5, s[6:7], 0x10\n",
        );
        check_recording(
            dir.path(),
            "operands-memory",
            "s_load_dword s5, s[6:7], 0x10\n",
        )
        .expect("the recording describes exactly these probes");
    }

    /// A probe added after the recording was taken is rejected, although the replay would
    /// still produce the committed table.
    #[test]
    fn a_probe_added_since_the_recording_is_refused_by_name() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        recorded(
            dir.path(),
            "operands-memory",
            "s_load_dword s5, s[6:7], 0x10\n",
        );

        let with_one_more = concat!(
            "s_load_dword s5, s[6:7], 0x10\n",
            "s_load_dword s44, s[80:81], 0x2a0\n"
        );
        let refused = check_recording(dir.path(), "operands-memory", with_one_more)
            .expect_err("a recording taken for fewer probes is not a recording of these");

        let said = refused.to_string();
        assert!(
            said.contains(STALE_MARKER),
            "the message must carry the words the gate greps for, said: {said}"
        );
        assert!(
            said.contains("operands-memory.in"),
            "the message must name the recording to re-record, said: {said}"
        );
    }

    /// A removed probe is the same failure from the other side.
    #[test]
    fn a_probe_removed_since_the_recording_is_refused_too() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let two = concat!(
            "s_load_dword s5, s[6:7], 0x10\n",
            "s_load_dword s44, s[80:81], 0x2a0\n"
        );
        recorded(dir.path(), "operands-memory", two);
        check_recording(
            dir.path(),
            "operands-memory",
            "s_load_dword s5, s[6:7], 0x10\n",
        )
        .expect_err("dropping a probe changes what was solved just as much as adding one");
    }

    /// Line endings alone are not a probe edit, so a Windows checkout passes.
    #[test]
    fn line_endings_alone_are_not_a_different_probe_set() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        recorded(
            dir.path(),
            "operands-memory",
            "s_load_dword s5, s[6:7], 0x10\r\nds_read_b32 v1, v2 offset:4\r\n",
        );
        check_recording(
            dir.path(),
            "operands-memory",
            "s_load_dword s5, s[6:7], 0x10\nds_read_b32 v1, v2 offset:4\n",
        )
        .expect("the same probes, checked out differently, are the same probes");
    }

    /// A recording that is not there names the file it should be.
    #[test]
    fn a_missing_recording_says_which_one_is_missing() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let refused = check_recording(dir.path(), "operands-memory", "s_nop 0\n")
            .expect_err("there is no recording to compare against");
        assert!(
            refused.to_string().contains("operands-memory.in"),
            "said: {refused}"
        );
    }

    /// The other end of the marker: the gate really does look for these words.
    ///
    /// Reads the script, pinning the shell half of the [`STALE_MARKER`] coupling.
    #[test]
    fn the_gate_looks_for_the_words_this_message_carries() {
        let runner = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bin/orbistoun");
        let script = std::fs::read_to_string(&runner)
            .unwrap_or_else(|why| panic!("reading {}: {why}", runner.display()));
        assert!(
            script.contains(STALE_MARKER),
            "bin/orbistoun no longer greps for `{STALE_MARKER}`, so it can no longer tell a stale recording from a failed replay"
        );
    }
}
