//! Exporting and checking a submission bundle: `submit`.

use crate::SubmitAction;
use crate::common::{build_stamp, knowledge_path};
use anyhow::{Context, Result};

/// Every title result this machine holds, both slots.
///
/// Reads the same directory `compat list` does, so a submission carries what the tree carries.
fn gathered_results(dir: &std::path::Path) -> Result<orbistoun_submit::Results> {
    let mut results = orbistoun_submit::Results::default();
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        // Not an error: a machine with no recorded title still has measurements worth sending.
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
    // Generated patches first, then hand-written ones, which are added second and win on name.
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

/// The promotion each measurement implies, as a patch a maintainer can apply.
///
/// `learned.toml` is one machine's cache; a knowledge file is what the emulator ships. Every field
/// of the generated entry comes from the measurement, so nothing is recalled (D322). Skipped where
/// the entry already exists, since nothing can say which of two entries is current, and where no
/// library is known, since a patch aimed at a guessed file has to be undone.
fn generated_proposals(
    learned: &orbistoun_hle::learned::Learned,
    into: &std::path::Path,
) -> Result<Vec<orbistoun_submit::Proposal>> {
    let mut out = Vec::new();
    for measurement in &learned.measurements {
        // No library, no patch: a measurement recorded without one does not say which file it
        // belongs in.
        if measurement.library.is_empty() {
            continue;
        }
        let path = knowledge_path(&measurement.library);
        let Ok(existing) = std::fs::read_to_string(&path) else {
            continue;
        };
        // One claim per function: an existing entry means this was already promoted.
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
            // The measurement's own oracle: a promotion is no better known than the observation
            // behind it.
            known: measurement.known,
            evidence: format!("measured against {}", measurement.measured),
            assumes: measurement.assumes.clone(),
        });
    }
    Ok(out)
}

/// Source changes waiting in `patches/`, and what each rests on.
///
/// Read from a file a person writes, not inferred from the diffs: only whoever produced a patch can
/// say where its behaviour came from.
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
        // Refused rather than skipped: a proposal naming an absent diff describes a change nobody
        // can read.
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
pub(crate) fn dispatch_submit(action: &SubmitAction) -> Result<()> {
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
        // Refused with the reason: an empty bundle almost always means the loop was never run, not
        // that the machine found nothing.
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
    // The diffs travel as files, read with ordinary patch tools. `local_proposals` has already
    // refused any entry naming an absent one.
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
        // Reported here, the last point before the bundle leaves the machine: a claim that cannot
        // name its tree cannot be checked by the receiver.
        println!("  ! this build cannot name its commit, so a receiver cannot check it");
        println!("    against a tree - commit first, or set ORBISTOUN_COMMIT");
    }
    if bundle.proposals.is_empty() {
        println!(
            "  send the directory. Nothing in it is a title file: it carries claims, each one\n  reproducible by anybody holding the same title"
        );
    } else {
        // Reported separately: claims are checkable by a command, a patch needs a person to read it
        // (D322).
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

    // Counted from the contents, not quoted from the manifest, so an edited bundle cannot announce
    // totals nothing here checked (D315).
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

    // Listed apart because nothing here can check them: a patch is settled by a person reading it
    // and running the gate, and must not inherit the measurements' trust (D322).
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

    // Re-derived, not trusted (D297). Agreement prints nothing; each difference is named
    // individually.
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
