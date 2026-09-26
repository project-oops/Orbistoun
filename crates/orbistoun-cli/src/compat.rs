//! Per-title compatibility records: `compat` and the record a run writes.

use crate::common::{previous_trace, title_id};
use anyhow::{Context, Result};

/// Where one title's record lives.
fn compat_path(dir: &std::path::Path, title: &str) -> std::path::PathBuf {
    dir.join(format!("{title}.toml"))
}

/// Reads a title record, or an empty one.
fn load_compat(dir: &std::path::Path, title: &str) -> Result<orbistoun_overrides::OverrideFile> {
    let path = compat_path(dir, title);
    match std::fs::read_to_string(&path) {
        Ok(text) => orbistoun_overrides::OverrideFile::from_toml(&text)
            .with_context(|| format!("parsing {}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Ok(orbistoun_overrides::OverrideFile::default())
        }
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

/// `compat list` - every recorded title, furthest first.
pub(crate) fn cmd_compat_list(dir: &std::path::Path) -> Result<()> {
    let mut rows: Vec<(String, orbistoun_overrides::Status)> = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            println!("no records yet - {} does not exist", dir.display());
            return Ok(());
        }
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
            rows.push((title, status));
        }
    }
    if rows.is_empty() {
        println!("no titles recorded yet");
        return Ok(());
    }
    // Ranked and rendered in the library, so the table and the record agree on which title is
    // closest to running, and a golden test holds the whole shape against the records (D184).
    let ranked = orbistoun_overrides::frontier(rows);
    println!("{} titles recorded", ranked.len());
    println!();
    print!("{}", orbistoun_overrides::render_frontier(&ranked));
    Ok(())
}

/// Reads the `compat/` records into ranked rows, each carrying its title metadata and notes.
///
/// A `.toml` that only configures a title and has measured nothing is skipped. The `status`
/// baseline is preferred over the `experiment` slot, which is marked when used.
fn compat_rows(
    entries: std::fs::ReadDir,
    shots: &std::path::Path,
) -> Result<Vec<(orbistoun_overrides::Row, orbistoun_overrides::Title, String)>> {
    let mut rows = Vec::new();
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
        // Prefer the unassisted baseline (`status`); fall back to the `experiment` slot, marked so
        // a reader knows the number came from a run with stub answers.
        let (status, experiment) = match (file.status, file.experiment) {
            (Some(s), _) => (s, false),
            (None, Some(e)) => (e, true),
            // A file that only configures a title and has measured nothing is not a row.
            (None, None) => continue,
        };
        // A screenshot if one sits beside the record, embedded relative to the repository root,
        // where the table is written by default.
        let shot = shots.join(format!("{title}.png"));
        let screenshot = shot.exists().then(|| {
            shots
                .join(format!("{title}.png"))
                .to_string_lossy()
                .replace('\\', "/")
        });
        let notes = status.notes.clone();
        rows.push((
            orbistoun_overrides::Row {
                title,
                name: file.title.name.clone(),
                status,
                experiment,
                screenshot,
            },
            file.title,
            notes,
        ));
    }
    Ok(rows)
}

/// The generated compat files that are not what the records render to, each named for the message.
///
/// A missing file (a whole page deleted) reads as drift the same as a differing one. Read-only:
/// it never touches the tree, so `--check` mutates nothing.
fn compat_drift(
    rendered_pages: &[(std::path::PathBuf, String)],
    out: &std::path::Path,
    doc: &str,
) -> Vec<String> {
    let mut drifted = Vec::new();
    for (page, text) in rendered_pages {
        if std::fs::read_to_string(page).ok().as_deref() != Some(text) {
            drifted.push(page.display().to_string());
        }
    }
    if std::fs::read_to_string(out).ok().as_deref() != Some(doc) {
        drifted.push(out.display().to_string());
    }
    drifted
}

/// `compat markdown` - render every record as a ranked markdown table into a tracked file.
pub(crate) fn cmd_compat_markdown(
    dir: &std::path::Path,
    out: &std::path::Path,
    shots: &std::path::Path,
    check: bool,
) -> Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            println!("no records yet - {} does not exist", dir.display());
            return Ok(());
        }
        Err(e) => return Err(e).with_context(|| format!("reading {}", dir.display())),
    };
    let rows = compat_rows(entries, shots)?;
    if rows.is_empty() {
        println!("no titles recorded yet");
        return Ok(());
    }
    let count = rows.len();
    // A page per title as well as the table: the table ranks titles, a page describes one (D660).
    let pages = out
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join("docs")
        .join("titles");
    // Render every page and the table into memory first, so `--check` compares against exactly what
    // a write produces, from one code path.
    let rendered_pages: Vec<(std::path::PathBuf, String)> = rows
        .iter()
        .map(|(row, meta, notes)| {
            (
                pages.join(format!("{}.md", row.title)),
                orbistoun_overrides::render_title_page(row, meta, notes),
            )
        })
        .collect();
    let rows: Vec<orbistoun_overrides::Row> = rows.into_iter().map(|(r, _, _)| r).collect();
    let table = orbistoun_overrides::render_markdown(&rows);
    let doc = format!(
        concat!(
            "# Compatibility\n\n",
            "_Generated by `orbistoun-cli compat markdown` from the records in `compat/`. Do not edit ",
            "by hand; re-run it. Ranked by how far each guest got - reach, then distinct imports, ",
            "then how many of those imports were **answered** by something real, then standing, then frames, then calls._\n\n",
            "**Answered** is how many of the imports a title called landed on a real implementation rather than a placeholder. It is counted in **functions**, not calls: a percentage of calls belongs to whatever the guest loops on, which is why `standing` beside it could not see a day that took one title from 35 unanswered functions to 20 (D563). A dash means the run predates the measurement. **Reach** climbs `rejected`, `parsed`, `linked`, `entered`, `exited`, `flipped`, `presented` - `flipped` means the guest got a frame to the output layer, a place reached and not a picture shown (D558); `presented`, the top rung, means a buffer whose pixels differ from what it held before the guest ran, and nothing in the corpus has reached it. **From** is `run` for a result measuring the emulator as it stands and `experiment` for a run ",
            "resting on an answer nothing measured (less comparable - D181, D557); an answer taken from the target is not a prop. A 📷 marks a guest with a captured ",
            "framebuffer; see the Screenshots section.\n\n",
            "{}"
        ),
        table
    );

    if check {
        // The same guard as `status --check`, over `COMPATIBILITY.md` and the per-title pages.
        // Orphan pages are left alone by both check and write, so they agree on which files are
        // current.
        let drifted = compat_drift(&rendered_pages, out, &doc);
        if !drifted.is_empty() {
            anyhow::bail!(
                "these generated files are not what the records render to - run `orbistoun-cli compat markdown`: {}",
                drifted.join(", ")
            );
        }
        println!("every generated compat file is current ({count} titles)");
        return Ok(());
    }

    std::fs::create_dir_all(&pages).with_context(|| format!("creating {}", pages.display()))?;
    for (page, text) in &rendered_pages {
        std::fs::write(page, text).with_context(|| format!("writing {}", page.display()))?;
    }
    std::fs::write(out, &doc).with_context(|| format!("writing {}", out.display()))?;
    println!("wrote {} ({count} titles)", out.display());
    Ok(())
}

/// `compat record` - transcribe the last run of a title into its record.
pub(crate) fn cmd_compat_record(
    path: &std::path::Path,
    dir: &std::path::Path,
    note: Option<&str>,
    force: bool,
) -> Result<()> {
    let title = title_id(path)
        .with_context(|| format!("{} has no containing directory to name it", path.display()))?;

    let trace = previous_trace(path).with_context(|| {
        format!("no trace for {title} - run it first, so there is a measurement to record")
    })?;

    let mut status = orbistoun_report::trace::status_of(&trace, orbistoun_nid::today());
    if let Some(note) = note {
        note.clone_into(&mut status.notes);
    }

    match keep_status(dir, &title, &status, &title_metadata(path), force)? {
        Kept::NotBetter { slot, previous } => anyhow::bail!(
            concat!(
                "{}: not recorded - the {} entry is better, or the same run again ({} {} imports, ",
                "{} calls).\n\nUse --force to record it anyway."
            ),
            title,
            slot,
            previous.reach.label(),
            previous.imports,
            previous.calls
        ),
        Kept::Written { slot, path } => {
            println!(
                "{}: [{slot}] {} - {} imports, {} calls, {}% standing",
                path.display(),
                status.reach.label(),
                status.imports,
                status.calls,
                status.standing
            );
            if status.propped_up() {
                // Said when written: the number is real, and it is not a claim about the emulator
                // unassisted.
                println!(
                    "  under {} - kept apart from the honest record",
                    status.describe_policy()
                );
            }
        }
    }
    Ok(())
}

/// What a title says about itself, read from the `param.json` it ships.
///
/// Derived rather than typed, so the name is the title's own statement and right for a re-release
/// (D660). `None` for anything that ships no such file, such as a homebrew payload.
fn title_metadata(path: &std::path::Path) -> orbistoun_overrides::Title {
    let Some(dir) = path.parent() else {
        return orbistoun_overrides::Title::default();
    };
    let Ok(text) = std::fs::read_to_string(dir.join("sce_sys").join("param.json")) else {
        return orbistoun_overrides::Title::default();
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
        return orbistoun_overrides::Title::default();
    };
    let string = |key: &str| {
        json.get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned)
    };
    // The localised name is preferred: that is the name a person sees, and the top level is
    // sometimes an internal one. The default language names itself, so no locale is chosen here.
    let localised = json
        .get("localizedParameters")
        .and_then(|l| {
            let language = l
                .get("defaultLanguage")
                .and_then(serde_json::Value::as_str)?;
            l.get(language)
        })
        .and_then(|entry| entry.get("titleName"))
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned);
    orbistoun_overrides::Title {
        id: string("titleId"),
        name: localised.or_else(|| string("titleName")),
        content_version: string("contentVersion"),
        master_version: string("masterVersion"),
    }
}

/// What happened to a status offered to the record.
enum Kept {
    /// Written into the named slot.
    Written {
        /// Which slot took it.
        slot: &'static str,
        /// The file written.
        path: std::path::PathBuf,
    },
    /// That slot already holds something better, or this is the same run again.
    NotBetter {
        /// Which slot was compared against.
        slot: &'static str,
        /// What it holds.
        previous: orbistoun_overrides::Status,
    },
}

/// Puts a status in the slot its policy belongs to, if it improves on what is there.
///
/// The one implementation of "which slot, and is this better", shared by automatic recording and
/// `compat record`. A run is routed to its slot, never refused on policy; within a slot an
/// automatic best-ever does not move backwards (D312).
fn keep_status(
    dir: &std::path::Path,
    title: &str,
    status: &orbistoun_overrides::Status,
    meta: &orbistoun_overrides::Title,
    force: bool,
) -> Result<Kept> {
    let propped = status.propped_up();
    let slot = if propped { "experiment" } else { "status" };

    let mut file = load_compat(dir, title)?;
    // Refreshed whether or not the run is kept: the metadata describes the title, not the run, and
    // an empty read does not erase an earlier one.
    if !meta.is_empty() {
        file.title = meta.clone();
    }
    let previous = if propped {
        file.experiment.as_ref()
    } else {
        file.status.as_ref()
    };
    if let Some(previous) = previous {
        if !force && !status.worth_recording(previous) {
            let previous = previous.clone();
            // The refreshed metadata is saved even when the status is not.
            if !meta.is_empty() {
                write_compat(dir, title, &file)?;
            }
            return Ok(Kept::NotBetter { slot, previous });
        }
    }
    if propped {
        file.experiment = Some(status.clone());
    } else {
        file.status = Some(status.clone());
    }

    let path = write_compat(dir, title, &file)?;
    Ok(Kept::Written { slot, path })
}

/// Writes a record, answering where it went.
///
/// Shared by the metadata and status writes so they agree on the directory.
fn write_compat(
    dir: &std::path::Path,
    title: &str,
    file: &orbistoun_overrides::OverrideFile,
) -> Result<std::path::PathBuf> {
    let text = file.to_toml().context("rendering the record")?;
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let path = compat_path(dir, title);
    std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}

/// Keeps what a run achieved, in the slot its policy belongs to.
///
/// Recording is automatic, so the record stays derived from runs rather than depending on someone
/// to act on a prompt (D182). It is safe unattended because of slot routing: a run with stub
/// answers cannot overwrite the unassisted number.
pub(crate) fn record_compat(path: &std::path::Path, trace: &orbistoun_report::trace::CallTrace) {
    let dir = std::path::Path::new("compat");
    let Some(title) = title_id(path) else {
        return;
    };
    // Only a title id is recorded - letters, digits, dot, dash, underscore - not whatever the
    // containing directory is called. A run that declines to record says so, so it is not mistaken
    // for a run with nothing to record.
    if !title
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    {
        println!();
        println!("  not recorded: {title:?} is a directory name, not a title id");
        println!("  move the module under a directory named for the title to record it");
        return;
    }
    // An intervened run is not a result about the emulator: a diagnostic that maps memory, plants a
    // value or forces an answer changes the program being measured (D355).
    if trace.conditions.intervened {
        println!();
        println!("  not recorded: this run was under a diagnostic, so what it reached is");
        println!("  a fact about the intervention rather than about the title");
        return;
    }
    let status = orbistoun_report::trace::status_of(trace, orbistoun_nid::today());

    // Only into a directory that already exists, so running elsewhere creates no files. A checkout
    // has it; anyone else opts in by creating it.
    if !dir.is_dir() {
        println!();
        println!("  {title} reached {} imports, unrecorded", status.imports);
        println!("  `mkdir compat` and every run after this one records itself");
        return;
    }

    // Written rather than suggested. Slot routing keeps this safe unattended (D312).
    match keep_status(dir, &title, &status, &title_metadata(path), false) {
        Ok(Kept::Written { slot, path }) => {
            println!();
            println!(
                "  recorded [{slot}] {} - {} imports, {} calls, {}% standing",
                status.reach.label(),
                status.imports,
                status.calls,
                status.standing
            );
            println!("  {}", path.display());
        }
        // A record as good as this run is the ordinary outcome; one holding something better means
        // a regression, which is reported.
        Ok(Kept::NotBetter { previous, .. }) => note_if_below_best(&status, &previous),
        // Reported, not swallowed: otherwise the result exists only on this screen.
        Err(e) => {
            println!();
            println!("  could not record {title}: {e:#}");
        }
    }

    // A run that faulted gets the attribution framed by the hardware ground truth, so the default
    // reading is that the gap is orbistoun's (D708). The record was just written by `keep_status`;
    // re-reading it keeps this independent of that path.
    if trace.fault.is_some() {
        let file = load_compat(dir, &title).unwrap_or_default();
        println!();
        for line in file.fault_attribution() {
            println!("  {line}");
        }

        // Each import orbistoun stubbed is a place the guest got a placeholder instead of a real
        // answer, and a candidate cause of the fault. Most-called first, capped so a long tail does
        // not bury the head.
        let stubbed = trace.stubbed_imports();
        if !stubbed.is_empty() {
            println!();
            println!(
                "  orbistoun answered these with placeholders this run - candidate causes, a real answer may avoid it:"
            );
            for import in stubbed.iter().take(8) {
                let shape = if import.shape.is_empty() {
                    String::new()
                } else {
                    format!("  {}", import.shape)
                };
                println!("    {} x{}{}", import.label, import.calls, shape);
            }
            if stubbed.len() > 8 {
                println!(
                    "    ... and {} more (`./bin/orbistoun questions` ranks them all)",
                    stubbed.len() - 8
                );
            }
        }

        // The guest's own log lines are the title's output: a `TODO:` in them is the title's own
        // note, not evidence of an unfinished path in the title.
        println!();
        println!(
            "  the guest's own log lines above are the title's output, not orbistoun's; a TODO:/todo:"
        );
        println!("  marker among them is the title's own note, not a sign its code is unfinished.");
    }
}

/// Whether a run reached less than the record it is being compared against.
///
/// Fewer imports, or the same imports and fewer calls. Split from the printing so the rule is
/// tested. Standing is not consulted: implementing an already-called function raises standing
/// without moving reach, and reach is what this line reports.
fn is_below_best(
    status: &orbistoun_overrides::Status,
    previous: &orbistoun_overrides::Status,
) -> bool {
    status.imports < previous.imports
        || (status.imports == previous.imports && status.calls < previous.calls)
}

/// Says so when a run falls short of the best ever recorded for its title.
///
/// Each run otherwise compares only with the previous run, so a title that lost ground reports
/// `same` indefinitely. The record's time limit is printed beside this run's, so a shorter run is
/// distinguishable from a regression. Silent unless reach is lower.
fn note_if_below_best(
    status: &orbistoun_overrides::Status,
    previous: &orbistoun_overrides::Status,
) {
    if !is_below_best(status, previous) {
        return;
    }
    println!();
    println!(
        "  below the best ever recorded for this title: {} imports and {} calls, against {} and {} on {}",
        status.imports, status.calls, previous.imports, previous.calls, previous.measured_on
    );
    println!(
        "  reached {} where the record reached {}",
        status.reach.label(),
        previous.reach.label()
    );
    match (status.limit_seconds, previous.limit_seconds) {
        // The reading that would make this line wrong, stated up front.
        (Some(now), Some(then)) if now < then => println!(
            concat!(
                "  this run had {}s against the record's {}s, so a shorter run is the ",
                "ordinary explanation"
            ),
            now, then
        ),
        (Some(now), Some(then)) => println!(
            "  this run had {now}s against the record's {then}s, so time is not the explanation"
        ),
        _ => {}
    }
    println!("  the record is not overwritten - it is a best-ever, and this run is not one");
}

#[cfg(test)]
mod tests {
    /// A status carrying only the two fields the rule reads.
    fn reach(imports: usize, calls: u64) -> orbistoun_overrides::Status {
        orbistoun_overrides::Status {
            reach: orbistoun_overrides::Reach::Entered,
            outcome: String::new(),
            imports,
            calls,
            standing: 0,
            default_return: "unimplemented".to_owned(),
            overrides: 0,
            propping: 0,
            limit_seconds: None,
            build: String::new(),
            measured_on: String::new(),
            frames: 0,
            unanswered: None,
            notes: String::new(),
        }
    }

    /// A regression is fewer imports, or the same imports and fewer calls.
    ///
    /// All four cases, because a rule that always answered `true` would satisfy any one of them.
    #[test]
    fn a_run_is_below_its_record_on_reach_and_not_on_anything_else() {
        let record = reach(47, 933);
        let fewer_imports = reach(26, 334);
        let same_imports_fewer_calls = reach(47, 900);
        let identical = reach(47, 933);
        let better = reach(48, 100);

        assert!(
            super::is_below_best(&fewer_imports, &record),
            "twenty-one imports gone is the case this exists for"
        );
        assert!(
            super::is_below_best(&same_imports_fewer_calls, &record),
            "the same reach with fewer calls is less of the guest run"
        );
        assert!(
            !super::is_below_best(&identical, &record),
            concat!(
                "a run equal to the record is the ordinary outcome and must stay silent - a line ",
                "that fires on equality is one people learn to skip"
            )
        );
        assert!(
            !super::is_below_best(&better, &record),
            concat!(
                "and more imports is not a regression even with far fewer calls, because reach is ",
                "what this line is about"
            )
        );
    }

    /// The compat `--check` fails on a hand-edited generated file, and passes on a fresh one.
    ///
    /// A guard is trusted only once made to fail (D227), so both a corrupted table and a deleted
    /// page are exercised.
    #[test]
    fn compat_check_rejects_a_hand_edited_generated_file() {
        let root = tempfile::tempdir().expect("a temp dir");
        let dir = root.path().join("compat");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("dist.toml"),
            r#"[compat]
[settings]
[status]
reach = "entered"
outcome = "0x1"
imports = 0
calls = 0
standing = 100
default_return = "unimplemented"
unanswered = 0
limit_seconds = 20
build = "0.1.0"
measured_on = "2026-09-10"
"#,
        )
        .unwrap();
        let out = root.path().join("COMPATIBILITY.md");
        let shots = dir.join("screenshots");

        // A fresh write leaves the tree current, so `--check` passes.
        super::cmd_compat_markdown(&dir, &out, &shots, false).expect("write");
        super::cmd_compat_markdown(&dir, &out, &shots, true)
            .expect("a freshly written tree is current");

        // One hand-edited cell in the table is caught, and the file is named.
        let good = std::fs::read_to_string(&out).unwrap();
        std::fs::write(&out, good.replacen("2026-09-10", "2026-09-11", 1)).unwrap();
        let err = super::cmd_compat_markdown(&dir, &out, &shots, true)
            .unwrap_err()
            .to_string();
        assert!(err.contains("COMPATIBILITY.md"), "names the table: {err}");

        // Rewriting restores it; a wholly deleted page is caught and named too.
        super::cmd_compat_markdown(&dir, &out, &shots, false).expect("rewrite");
        let page = out
            .parent()
            .unwrap()
            .join("docs")
            .join("titles")
            .join("dist.md");
        std::fs::remove_file(&page).unwrap();
        let err = super::cmd_compat_markdown(&dir, &out, &shots, true)
            .unwrap_err()
            .to_string();
        assert!(err.contains("dist.md"), "names the page: {err}");
    }
}
