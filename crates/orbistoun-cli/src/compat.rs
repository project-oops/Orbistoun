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
    // Ranked and rendered one layer down, so the table and the record cannot disagree
    // about which title is closest to running - and so a test can hold the whole shape
    // against the records in the tree (D184).
    let ranked = orbistoun_overrides::frontier(rows);
    println!("{} titles recorded", ranked.len());
    println!();
    print!("{}", orbistoun_overrides::render_frontier(&ranked));
    Ok(())
}

/// Read the `compat/` records into ranked rows, each carrying its title metadata and notes.
///
/// A `.toml` that only configures a title and has measured nothing is skipped, and the honest
/// `status` baseline is preferred over the `experiment` slot (marked when it falls back, D181).
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
        // Prefer the honest baseline (`status`); fall back to the `experiment` slot, marking it so
        // a reader knows the number came from a run with overrides and is less comparable (D181).
        let (status, experiment) = match (file.status, file.experiment) {
            (Some(s), _) => (s, false),
            (None, Some(e)) => (e, true),
            // A file that only configures a title and has measured nothing is not a row.
            (None, None) => continue,
        };
        // A screenshot if one sits beside the record. Embedded relative to the repo root, which
        // is where the table is written by default.
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
    // **A page each as well as the table.** The table ranks titles against each other and a page
    // describes one - a reader wanting to know what `PPSA28061` *is* should not have to find a
    // row in a list of thirty-four (D660).
    let pages = out
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join("docs")
        .join("titles");
    // Render every page and the table into memory first, so `--check` compares against exactly
    // what a write would produce, and both arms render from one code path.
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
        // The guard `status --check` is for README/PROJECT_STATUS; this is the same guard over the
        // two generated things it does not cover, `COMPATIBILITY.md` and the per-title pages. Orphan
        // pages are left alone, because a write leaves them alone too, so check and write agree on
        // exactly which trees are current (17e5).
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
                // Said at the moment it is written, not left for whoever reads the file
                // later. The number is real and it is not a claim about the emulator as it
                // stands.
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
/// **Derived rather than typed**, for the same reason the status half of a record is: a display
/// name written by hand is wrong for a re-release, missing for the next title somebody adds, and
/// unfalsifiable either way. The file is the title's own statement of what it is (D660).
///
/// `None` for anything that ships no such file, which is every homebrew payload in the corpus -
/// and that is a fact worth keeping rather than papering over.
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
    // **The localised name is preferred over the top-level one**, because that is where a title
    // puts the name a person sees; the top level is the fallback and is sometimes the internal
    // one. The default language names itself, so nothing here picks a locale.
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
/// # Why this is one function and not two callers doing the same thing
///
/// A run records automatically and `compat record` records on request, and they were about to
/// be two implementations of *which slot, and is this better* - the pair of rules that decide
/// whether the compatibility database means anything. Two copies of that drift, and the drift
/// is invisible until the two disagree about one title (D323).
///
/// **Routed rather than refused.** A run under a measured policy is a real result about a
/// different question, so it goes in its own slot; the only refusal left is within a slot,
/// where an automatic best-ever that moves backwards is not a record of anything (D312).
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
    // **Refreshed whether or not the run is kept.** The metadata describes the title, not the
    // run, so a record that loses the better-run comparison should still learn the title's name -
    // and an empty read must not erase what an earlier one found (D660).
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
            // **The metadata is still saved, and that is the whole reason it is refreshed above.**
            // It describes the *title*, not the run, so a record that loses the better-run
            // comparison should still learn the title's name - and the first version of this
            // returned here without writing, so the name it had just read was thrown away every
            // time except on a title that happened to improve (D660).
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
/// One place rather than two, because the metadata refresh needs the same write as the status one
/// and a second copy of it would eventually disagree about the directory.
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
/// **What makes the record maintain itself, and it used to only ask.** A compatibility
/// database nobody updates becomes a graveyard of stale entries within a month, and the
/// moment anybody notices is the moment they stop trusting all of it (D182). This printed
/// the command and waited: `compat/` then sat untouched for four days while the loop kept
/// finding things, so the repository's record of a title disagreed with every run anybody
/// had done. **A prompt nobody acts on measured nothing.**
///
/// The run already knows both numbers, so writing costs no more than asking did - and it is
/// safe unattended only because of the slot routing, which stopped a helped run being able
/// to overwrite the honest one (D312, D323).
pub(crate) fn record_compat(path: &std::path::Path, trace: &orbistoun_report::trace::CallTrace) {
    let dir = std::path::Path::new("compat");
    let Some(title) = title_id(path) else {
        return;
    };
    // **A title id, not whatever the containing directory was called.** Recording is automatic
    // now, and a binary sitting in a scratch folder produced `compat/New folder (2).toml` -
    // a tracked record named after somebody's Explorer default, describing a payload rather
    // than a title. A title id is an identifier: letters, digits, dot, dash, underscore.
    //
    // Said rather than skipped, because a run that quietly declines to record is
    // indistinguishable from one that had nothing to record (D347).
    if !title
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    {
        println!();
        println!("  not recorded: {title:?} is a directory name, not a title id");
        println!("  move the module under a directory named for the title to record it");
        return;
    }
    // **An intervened run is not a result about the emulator.** A diagnostic that maps memory
    // the guest never asked for, plants a value or forces an answer has changed the program
    // being measured, so what it reached says nothing about what the title reaches (D227).
    //
    // Found by adding recording to `turn`: the last boot of a turn is a diagnostic, so the
    // first turn to record filed `25 imports, ran to the time limit` for a title that reaches
    // 13 - a number bought by a reserved region, filed as a compatibility claim. The hazard
    // was already there for `run` under any diagnostic; nothing had exercised it (D355).
    if trace.conditions.intervened {
        println!();
        println!("  not recorded: this run was under a diagnostic, so what it reached is");
        println!("  a fact about the intervention rather than about the title");
        return;
    }
    let status = orbistoun_report::trace::status_of(trace, orbistoun_nid::today());

    // **Only into a directory that already exists.** Creating one in whatever directory
    // somebody happened to run from is a surprise, and a surprise that writes files. A
    // checkout always has it; anybody else opts in once by making it.
    if !dir.is_dir() {
        println!();
        println!("  {title} reached {} imports, unrecorded", status.imports);
        println!("  `mkdir compat` and every run after this one records itself");
        return;
    }

    // **Written rather than suggested.** This printed the command to run and waited for
    // somebody to type it, and `compat/` then went four days without being touched while the
    // loop kept finding things - so the repository's own record of a title disagreed with
    // every run anybody had done. A prompt nobody acts on is a prompt that measured nothing.
    //
    // Safe to do unattended only because of the slot routing: a helped run can no longer
    // overwrite the honest number, and nothing here can move an entry backwards (D312, D323).
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
        // **Two states, and only one of them is nothing to say.** The record holding something
        // as good is the ordinary outcome of a run that changed nothing; the record holding
        // something *better* is a regression, and this printed for neither (D635).
        Ok(Kept::NotBetter { previous, .. }) => note_if_below_best(&status, &previous),
        // **Reported, not swallowed.** A run that could not write its own result is a run
        // whose finding exists only on this screen, and silence there is how four days of
        // them went missing in the first place.
        Err(e) => {
            println!();
            println!("  could not record {title}: {e:#}");
        }
    }

    // **Whose gap is this?** A run that faulted gets the attribution framed by the hardware ground
    // truth, at the fault, so a session reads the default - orbistoun's - before reaching for the
    // seductive conclusion that the title itself is at fault (D708). The record was just loaded and
    // written by `keep_status`; re-reading it is cheap and keeps this independent of that path.
    if trace.fault.is_some() {
        let file = load_compat(dir, &title).unwrap_or_default();
        println!();
        for line in file.fault_attribution() {
            println!("  {line}");
        }

        // **What orbistoun stubbed on the way here - the candidate causes, named.** The
        // upstream-divergence reading should be the default, not something dug for by hand: each of
        // these is a place orbistoun handed the guest a placeholder instead of a real answer, and
        // any could be what steered it into the wall (D708). Most-called first, capped so a long
        // tail does not bury the head.
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

        // **The guest's own log lines are the title's output, not evidence of incompleteness.** A
        // `TODO:` marker in the guest's logging is the title printing its own note; shipping titles
        // carry them, and reading one as an unfinished code path is how a wall gets misattributed to
        // the title (D708).
        println!();
        println!(
            "  the guest's own log lines above are the title's output, not orbistoun's; a TODO:/todo:"
        );
        println!("  marker among them is the title's own note, not a sign its code is unfinished.");
    }
}

/// Whether a run reached less than the record it is being compared against.
///
/// **Fewer imports, or the same imports and fewer calls.** Split out from the printing because
/// this is the decision and the rest is presentation - and because a rule about what counts as a
/// regression is worth a test rather than a reading (principle 8).
///
/// Standing is deliberately not consulted: implementing a function the guest already called raises
/// standing while moving no import and no call, so a run can be better on standing and worse on
/// reach, and reach is what this line is about.
fn is_below_best(
    status: &orbistoun_overrides::Status,
    previous: &orbistoun_overrides::Status,
) -> bool {
    status.imports < previous.imports
        || (status.imports == previous.imports && status.calls < previous.calls)
}

/// Says so when a run falls short of the best ever recorded for its title.
///
/// # The silence this replaces
///
/// `Kept::NotBetter` covered two states and printed for neither: *as good as the record*, which
/// is the ordinary outcome of a run that changed nothing, and *worse than the record*, which is a
/// regression. So a title that lost ground kept its old number in the file and said nothing on
/// screen, and every run afterwards compared itself to the previous run and reported `same`.
///
/// **PPSA28061 sat like that.** Its record holds 47 imports from 2026-08-23; the build reaches 26.
/// Two instruments looked straight at it - a ratchet that only moves up, and a verdict that only
/// looks back one run - and between them neither could say "below its own best" (D634, D635).
///
/// # Why the limits are printed beside the numbers
///
/// A shorter run legitimately reaches less. Printing the record's time limit next to this run's is
/// what lets a reader tell a regression from a shorter measurement without going to look - and
/// without it this line would cry wolf every time somebody tried a quicker run.
///
/// Silent unless the reach is genuinely lower, because a line that fires on equality is a line
/// people learn to skip.
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
        // **The one reading that would make this line wrong**, said before anybody has to ask.
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

    /// **A regression is fewer imports, or the same imports and fewer calls.**
    ///
    /// The rule behind the line that says a run fell below its own record. Written as a test
    /// because the branch it guards printed nothing for eight months and hid a 44% import loss
    /// in one title for sixteen days (D634, D635).
    ///
    /// All four cases, because a rule that only ever answered `true` would satisfy any one of
    /// them alone - which is exactly how the silence it replaces went unnoticed.
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

    /// **The compat `--check` fails on a hand-edited generated file, and passes on a fresh one.**
    ///
    /// `status --check` guards README/PROJECT_STATUS; this is the same guard over `COMPATIBILITY.md`
    /// and the per-title pages (17e5). A guard is only trustworthy once it has been watched reject
    /// something built to make it fail (D213), so both a corrupted table and a deleted page are
    /// exercised - a check that only ever answered "current" would pass either one alone.
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

        // A fresh write leaves the tree current, so --check passes.
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
