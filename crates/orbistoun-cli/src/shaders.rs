//! Shader translation coverage: `shaders`.

use anyhow::Result;

/// Prints how this corpus compares with the last time it was looked at, and records it.
///
/// Uses the import side's `FURTHER` / `same` / `BACK` vocabulary, since both are the same loop over
/// different material. Keyed by corpus path so corpora keep separate histories.
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
        // Reported apart from a regression: implementing one blocker routinely uncovers the next
        // instruction in a shader, which is progress.
        for name in &movement.uncovered {
            println!("  uncovered  {name}");
        }
    }

    write_shader_summary(corpus, &summary);
}

/// Where a corpus's history lives.
///
/// Named by a hash of the path, so two corpora never share one file.
fn shader_summary_path(corpus: &std::path::Path) -> std::path::PathBuf {
    let paths = orbistoun_paths::Paths::resolve();
    // A small stable digest of the path; it only has to separate corpora, not resist collision.
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
/// A failure is reported and not fatal: the comparison is optional and the worklist is not.
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

/// Whether a shader translates at any stage, and every distinct refusal if not.
///
/// A binary does not say which stage ran it, and the stage matters: an export has nowhere to go in
/// a compute dispatch, and a guest vertex program is a primitive shader refused as compute. So
/// every stage is tried, at wavefront fidelity, which is correct unconditionally. Every distinct
/// refusal is returned, because the last stage's reason says nothing about why the stage that could
/// have run the shader refused it; identical refusals collapse to one line.
fn translates(
    decoded: &orbistoun_shader::Decode,
    encodings: &orbistoun_shader::EncodingTable,
) -> Result<(), Vec<String>> {
    use orbistoun_translate::Width;
    use orbistoun_translate::wavefront::{Stage, Window, translate_for};

    // The default window: this asks whether a shader translates and never runs one. The window base
    // affects only what the translated module reads, not whether translation succeeds.
    let window = Window::default();
    let mut refused: Vec<String> = Vec::new();
    for stage in [Stage::Compute, Stage::Fragment, Stage::Mesh] {
        match translate_for(decoded, encodings, Width::Wave64, stage, window) {
            Ok(_) => return Ok(()),
            Err(e) => {
                let said = e.to_string();
                if !refused.iter().any(|seen| seen.ends_with(&said)) {
                    refused.push(format!("{stage:?}: {said}"));
                }
            }
        }
    }
    Err(refused)
}

/// Analyses every shader binary in a directory.
///
/// A shim over `orbistoun_shader::report`, so this command and the run report cannot disagree
/// (D034).
pub(crate) fn cmd_shaders(path: &std::path::Path, top: Option<usize>) -> Result<()> {
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

    // Only the corpus's own extension: the directory also holds reference-text files that would
    // decode as shaders.
    let mut entries: Vec<std::path::PathBuf> = files
        .iter()
        // Both kinds: a shader dumped from a title and one generated here decode identically. Their
        // provenance decides whether they may be committed, not whether this can read them.
        .filter(|p| is_shader(p))
        .cloned()
        .collect();
    // Sorted so two runs over an unchanged directory produce identical output.
    entries.sort();

    let skipped = files.len() - entries.len();

    if entries.is_empty() {
        // Said plainly: "0 of 0 complete" reads like success.
        println!(
            "no shader files in {} ({skipped} other file(s) present)",
            path.display()
        );
        return Ok(());
    }

    let mut coverage = CorpusCoverage::new();
    // Every shader that translated at no stage, and why. The coverage records the verdict, not the
    // reason.
    let mut refusals: Vec<(String, Vec<String>)> = Vec::new();
    for entry in &entries {
        let bytes =
            std::fs::read(entry).map_err(|e| anyhow::anyhow!("{}: {e}", entry.display()))?;
        let name = entry
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("<unnamed>");
        // The worklist ranks what is not translatable, so it asks the translator rather than
        // assuming.
        let supported = |key: orbistoun_shader::OpcodeKey| {
            key.encoding
                .and_then(|i| encodings.encodings().get(usize::from(i)))
                .is_some_and(|e| {
                    orbistoun_translate::model::supports_named(&encodings, &e.name, key.opcode)
                })
        };
        let decoded = decode(&bytes, &encodings, &operands);
        let verdict = translates(&decoded, &encodings);
        if let Err(reasons) = &verdict {
            refusals.push((name.to_owned(), reasons.clone()));
        }
        coverage.observe_translated(name, &decoded, &supported, Some(verdict.is_ok()));
    }

    // The tier comes from the translator, the only layer that knows why an instruction is refused;
    // the shader crate sees only what blocks a shader.
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

    // Named, not merely counted: this explains refusals that are not about an unsupported
    // instruction.
    if !refusals.is_empty() {
        println!("\nshaders that translate at no stage");
        for (name, reasons) in &refusals {
            println!("  {name}");
            for reason in reasons {
                println!("    {reason}");
            }
        }
    }

    report_shader_movement(&coverage, &encodings, &mnemonics, path);
    if skipped > 0 {
        // Reported so a corpus with a different extension does not look empty for no stated reason.
        println!(
            "
{skipped} file(s) skipped - not a shader"
        );
    }
    Ok(())
}
