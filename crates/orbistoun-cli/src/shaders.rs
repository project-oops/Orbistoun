//! Shader translation coverage: `shaders`.

use anyhow::Result;

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

/// Whether a translator produces a module for this shader, at any stage it could belong to.
///
/// # Why every stage is tried rather than one
///
/// A corpus is a directory of binaries and nothing in a binary says which stage ran it. The
/// stage is not cosmetic: an export has nowhere to go in a compute dispatch and is refused,
/// so judging a pixel shader as compute would report it untranslatable for a reason that is
/// about the question rather than the shader. Asking "is there a stage this translates at"
/// is the honest form of the question a corpus can answer, and it is cheap - only shaders that
/// fail everywhere pay for every stage.
///
/// The mesh stage joined the sweep when it was built (worklog 558): a guest vertex program is a
/// primitive shader, and judging one as compute refuses it at the message it opens with.
///
/// Wavefront fidelity, because it is the one that is correct unconditionally: a refusal here
/// is about the shader rather than about a model that cannot represent a lane mask.
/// Whether a shader translates at any stage, and what refused it if not.
///
/// # Why the reason comes back and not just the verdict
///
/// The census answered "11 of 14" and gave no way to find the three. That is fine while the
/// blocker list explains them - an unsupported instruction names itself - and useless the moment
/// every instruction is supported and three shaders still refuse, which is where the corpus now
/// is (worklog 578). A tool that can count a failure and not name it sends the reader to write
/// a one-off program, which is the thing first-party tooling exists to avoid.
///
/// **Every distinct refusal, not the last one.** Reporting only the last was tried first and is
/// actively misleading: a shader that samples is refused at the mesh stage for *being* a mesh
/// module, which says nothing about why the fragment stage - the one that could have run it -
/// turned it down. The stages that agree are collapsed, so a shader refused identically
/// everywhere still reads as one line.
fn translates(
    decoded: &orbistoun_shader::Decode,
    encodings: &orbistoun_shader::EncodingTable,
) -> Result<(), Vec<String>> {
    use orbistoun_translate::Width;
    use orbistoun_translate::wavefront::{Stage, Window, translate_for};

    // **The default window, because this asks whether a shader translates and never runs one.**
    // `Window::base` says where the guest-memory window sits, which every memory access is
    // checked against at execution; it cannot change whether translation succeeds, only what the
    // translated module then reads. A probe that answers "does this translate" therefore has no
    // base to supply and must not invent one that looks meaningful.
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
/// A thin shim over `orbistoun_shader::report`, per principle 13: what the report says
/// is a property of the analysis, so this command and the run report cannot disagree
/// about it.
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
    // Every shader that translated at no stage, and what the last stage said. Collected here
    // rather than derived from the coverage, which records the verdict and not the reason.
    let mut refusals: Vec<(String, Vec<String>)> = Vec::new();
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
        let decoded = decode(&bytes, &encodings, &operands);
        let verdict = translates(&decoded, &encodings);
        if let Err(reasons) = &verdict {
            refusals.push((name.to_owned(), reasons.clone()));
        }
        coverage.observe_translated(name, &decoded, &supported, Some(verdict.is_ok()));
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

    // **Named, not merely counted.** The blocker list above explains a shader refused for an
    // instruction; this explains one refused for anything else, which is the only kind left once
    // every instruction is supported.
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
        // Reported rather than assumed irrelevant: a corpus whose files carry a
        // different extension would otherwise look empty for no stated reason.
        println!(
            "
{skipped} file(s) skipped - not a shader"
        );
    }
    Ok(())
}
