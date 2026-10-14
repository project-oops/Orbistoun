//! `perf` and `bench` - a run recorded with the Tracy profiler and read back (D831).

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// The Tracy tools: `ORBISTOUN_TRACY_TOOLS`, then where `--install-tracy` puts them under the data
/// root, then `PATH`.
fn find_tools() -> Option<orbistoun_perf::Tools> {
    let root = orbistoun_paths::Paths::resolve().data_root().to_path_buf();
    let named = orbistoun_env::TRACY_TOOLS.get().map(PathBuf::from);
    orbistoun_perf::Tools::find(
        named
            .into_iter()
            .chain([orbistoun_perf::Tools::default_dir(&root)]),
    )
}

/// The tools, installing the pinned version under the data root when `install` asks.
fn tools(install: bool) -> Result<orbistoun_perf::Tools> {
    if install {
        let root = orbistoun_paths::Paths::resolve().data_root().to_path_buf();
        let dir = orbistoun_perf::Tools::default_dir(&root);
        println!(
            "installing Tracy {} into {}",
            orbistoun_perf::TRACY_VERSION,
            dir.display()
        );
        return orbistoun_perf::install(&dir).map_err(anyhow::Error::msg);
    }
    let tools = find_tools().with_context(|| {
        format!(
            "no Tracy tools found: pass --install-tracy to fetch Tracy {}, or name a directory in {}",
            orbistoun_perf::TRACY_VERSION,
            orbistoun_env::TRACY_TOOLS.name
        )
    })?;
    if !tools.matches_client() {
        eprintln!(
            "warning: the Tracy tools in {} are not version {}, which this build's client speaks",
            tools.dir().display(),
            orbistoun_perf::TRACY_VERSION
        );
    }
    Ok(tools)
}

/// What one recorded run of a title produced.
pub(crate) struct Recorded {
    /// The capture file.
    pub trace: PathBuf,
    /// What the capture tool counted.
    pub summary: orbistoun_perf::CaptureSummary,
    /// The capture read back.
    pub report: orbistoun_perf::Report,
}

/// Runs `path` for `seconds` with the Tracy client on a free port, recording it into the title's
/// own `perf` directory, and reads the recording back. Nothing is recorded in the title's compat
/// record: a profiled run measures the emulator's time, not how far the title got.
fn record(
    tools: &orbistoun_perf::Tools,
    path: &Path,
    seconds: u64,
    input: Option<&Path>,
    worker: Option<&Path>,
) -> Result<Recorded> {
    anyhow::ensure!(
        cfg!(feature = "tracy"),
        "this build has no Tracy client: build orbistoun-cli with --features tracy"
    );
    let title = crate::common::title_id(path).unwrap_or_else(|| "untitled".to_owned());
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let trace = orbistoun_paths::Paths::resolve()
        .title_perf_dir(&title)
        .join(format!("{stamp}.tracy"));
    let port = orbistoun_perf::free_port().context("finding a free port for the run's client")?;
    // SAFETY: single-threaded here; the spawned worker inherits the variables at startup.
    unsafe { std::env::set_var(orbistoun_env::TRACY.name, "1") };
    // SAFETY: as above.
    unsafe { std::env::set_var("TRACY_PORT", port.to_string()) };
    // As the window plays it, unless the environment says otherwise: what is measured is what a
    // player gets.
    for (var, value) in orbistoun_env::AS_PLAYED {
        if std::env::var_os(var.name).is_none() {
            // SAFETY: as above.
            unsafe { std::env::set_var(var.name, value) };
        }
    }
    // Longer than the run, so the capture ends when the worker's client goes, with all of it.
    let capture = orbistoun_perf::Capture::start(tools, &trace, port, seconds + 120)
        .context("starting tracy-capture")?;
    // The worker's log and the guest's own output, beside the recording: a title printing every
    // frame would bury the report, and a bench of every title would be unreadable.
    let log = trace.with_extension("log");
    let ran = run_worker(path, seconds, (input, worker), &log);
    println!("  the run's log: {}", log.display());
    if let Err(error) = ran {
        capture.abandon();
        return Err(error);
    }
    let (trace, summary) = capture.finish().context("finishing the capture")?;
    let mut report = orbistoun_perf::Report::read(tools, &trace).map_err(anyhow::Error::msg)?;
    for (waiting, done) in orbistoun_gpu::perf::WAITS_ON {
        report.waits_on(waiting.label(), done.label());
    }
    Ok(Recorded {
        trace,
        summary,
        report,
    })
}

/// One run of `path` in a worker - this build's, or the build `exe` - stopped after `seconds`.
fn run_worker(
    path: &Path,
    seconds: u64,
    (input, exe): (Option<&Path>, Option<&Path>),
    log: &Path,
) -> Result<()> {
    let mut worker = match exe {
        Some(exe) => orbistoun_worker::WorkerHandle::spawn_other_logging_to(exe, &[], log),
        None => orbistoun_worker::WorkerHandle::spawn_self_logging_to(&[], log),
    }
    .context("spawning a worker process")?;
    let events = worker
        .request(&orbistoun_proto::Request::Run {
            path: path.to_path_buf(),
            symbols_db: None,
            limit_seconds: Some(seconds),
            call_budget: None,
            input_script: input.map(std::path::absolute).transpose()?,
            capture_input: None,
            staged: false,
            relink: false,
        })
        .context("driving the worker")?;
    let mut failed = None;
    for event in &events {
        match event {
            orbistoun_proto::Event::Terminated { outcome, .. } => println!("  outcome {outcome:?}"),
            orbistoun_proto::Event::Failed { error } => failed = Some(error.clone()),
            _ => {}
        }
    }
    worker.shutdown().context("shutting the worker down")?;
    // A run that never started has nothing to record, and its capture would wait for a client.
    match failed {
        Some(error) => Err(anyhow::anyhow!("the run failed: {error}")),
        None => Ok(()),
    }
}

/// `perf` - record a run of one title and rank where its time went.
pub(crate) fn cmd_perf(
    path: Option<&Path>,
    (seconds, top): (Option<u64>, usize),
    input: Option<&Path>,
    (open, install): (bool, bool),
) -> Result<()> {
    let tools = tools(install)?;
    let Some(path) = path else {
        println!("Tracy tools ready in {}", tools.dir().display());
        return Ok(());
    };
    // As a benchmark runs the title: its compat record's pad script unless one is named.
    let title = crate::common::title_id(path).unwrap_or_default();
    let recorded_input = pad_script(&title);
    let input = input.or(recorded_input.as_deref());
    let seconds = run_seconds(&title, seconds);
    let recorded = record(&tools, path, seconds, input, None)?;
    let text = format!(
        "{}  {} zones, {:.1} s recorded, {:.1} fps by the capture's own count\n\n{}",
        recorded.trace.display(),
        recorded.summary.zones,
        recorded.summary.seconds,
        recorded.summary.fps(),
        recorded.report.render(top)
    );
    println!("{text}");
    let beside = recorded.trace.with_extension("txt");
    std::fs::write(&beside, &text).with_context(|| format!("writing {}", beside.display()))?;
    if open {
        tools
            .open_viewer(Some(&recorded.trace), None)
            .context("opening the Tracy viewer")?;
    }
    Ok(())
}

/// How long a measured run of `title` records: `given`, else the title's own `perf_seconds`
/// setting, else [`RUN_SECONDS`].
fn run_seconds(title: &str, given: Option<u64>) -> u64 {
    given
        .or_else(|| {
            orbistoun_overrides::Resolved::for_run(
                title,
                &orbistoun_paths::Paths::resolve().overrides_dir(),
            )
            .int(orbistoun_overrides::PERF_SECONDS)
            .and_then(|seconds| u64::try_from(seconds).ok())
        })
        .unwrap_or(RUN_SECONDS)
}

/// How long a measured run records where nothing says otherwise.
const RUN_SECONDS: u64 = 30;

/// The pad script `title`'s compat record replays, when it has one.
fn pad_script(title: &str) -> Option<PathBuf> {
    let inputs = crate::records::title_dir(Path::new("compat"), title)
        .join(orbistoun_overrides::INPUTS_FILE);
    inputs.is_file().then_some(inputs)
}

/// One title's line in a benchmark.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
struct BenchEntry {
    title: String,
    fps: Option<f64>,
    median_ms: Option<f64>,
    p99_ms: Option<f64>,
    /// The zones by self time, as the report ranked them.
    by_self: Vec<orbistoun_perf::ZoneStat>,
    /// Why it has no measurement, when it has none.
    skipped: Option<String>,
    /// How long the recording ran, which each zone's self time is a share of.
    #[serde(default)]
    seconds: Option<f64>,
}

impl BenchEntry {
    /// `title`'s line from one recorded run of it.
    fn of(title: String, recorded: Recorded) -> Self {
        Self {
            title,
            fps: recorded.report.frames.fps(),
            median_ms: recorded.report.frames.percentile(50.0),
            p99_ms: recorded.report.frames.percentile(99.0),
            skipped: recorded
                .report
                .frames
                .ms
                .is_empty()
                .then(|| "no frames presented".to_owned()),
            by_self: recorded.report.by_self,
            seconds: Some(recorded.summary.seconds).filter(|&seconds| seconds > 0.0),
        }
    }

    /// Each zone's self time as a share of the run, in percent.
    fn shares(&self) -> impl Iterator<Item = (&str, f64)> {
        let zones: u64 = self.by_self.iter().map(|zone| zone.total_ns).sum();
        let whole = self
            .seconds
            .map_or(orbistoun_perf::as_f64(zones), |seconds| seconds * 1e9);
        self.by_self
            .iter()
            .filter(move |_| whole > 0.0)
            .map(move |zone| {
                (
                    zone.name.as_str(),
                    orbistoun_perf::as_f64(zone.total_ns) * 100.0 / whole,
                )
            })
    }
}

/// `ab` - run one title under a baseline build and this one, alternately, and say what moved.
///
/// One run of each is not a comparison: the same binary's frame rate wanders by half between
/// runs as the guest's phase against the host clock falls. Alternating, and swapping which goes
/// first each round, puts both builds through the same drift.
pub(crate) fn cmd_ab(
    path: &Path,
    (baseline, against): (Option<&Path>, Option<&str>),
    (seconds, rounds, top): (Option<u64>, usize, usize),
    (input, warm_up): (Option<&Path>, bool),
) -> Result<()> {
    let built;
    let baseline = match (baseline, against) {
        (Some(baseline), _) => baseline,
        (None, Some(revision)) => {
            built = build_revision(revision)?;
            built.as_path()
        }
        (None, None) => anyhow::bail!("name a baseline build or a revision to build one from"),
    };
    anyhow::ensure!(
        baseline.is_file(),
        "no baseline build at {}",
        baseline.display()
    );
    let tools = tools(false)?;
    let title = crate::common::title_id(path).unwrap_or_default();
    let recorded_input = pad_script(&title);
    let input = input.or(recorded_input.as_deref());
    let seconds = run_seconds(&title, seconds);
    // A build's first run after it was built fills the shader and pipeline caches its later runs
    // take from, and whichever went first would pay for it alone (NVRB00001: 4.3 fps cold, 7.2 and
    // 7.6 warm). Each runs once uncounted.
    if warm_up {
        for exe in [Some(baseline), None] {
            let which = if exe.is_some() {
                "baseline"
            } else {
                "this build"
            };
            println!("warming up: {which}, {} s", seconds.min(WARM_UP_SECONDS));
            record(&tools, path, seconds.min(WARM_UP_SECONDS), input, exe)?;
        }
    }
    let (mut before, mut after) = (Vec::new(), Vec::new());
    for round in 0..rounds {
        let order = if round % 2 == 0 {
            [Some(baseline), None]
        } else {
            [None, Some(baseline)]
        };
        for exe in order {
            let which = if exe.is_some() {
                "baseline"
            } else {
                "this build"
            };
            println!("round {}/{rounds}: {which}, {seconds} s", round + 1);
            let recorded = record(&tools, path, seconds, input, exe)?;
            let entry = BenchEntry::of(which.to_owned(), recorded);
            if exe.is_some() {
                before.push(entry);
            } else {
                after.push(entry);
            }
        }
    }
    let text = format!(
        "{} against {}\n\n{}",
        path.display(),
        baseline.display(),
        render_ab(&before, &after, top)
    );
    println!("\n{text}");
    // Kept beside the recordings, as a perf report is: a comparison of an hour's runs is worth more
    // than a terminal's scrollback.
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let file = orbistoun_paths::Paths::resolve()
        .title_perf_dir(&title)
        .join(format!("ab-{stamp}.txt"));
    std::fs::write(&file, &text).with_context(|| format!("writing {}", file.display()))?;
    println!("kept as {}", file.display());
    Ok(())
}

/// The command line built with Tracy from `revision` of the repository the working directory lies
/// in, into the data root's `ab` directory, or as built there before.
fn build_revision(revision: &str) -> Result<PathBuf> {
    let dir = orbistoun_paths::Paths::resolve().data_root().join("ab");
    println!("building {revision} into {}", dir.display());
    orbistoun_perf::baseline::build_at(
        &std::env::current_dir().context("finding the repository")?,
        revision,
        &dir,
    )
    .map_err(anyhow::Error::msg)
}

/// The longest a warm-up run of [`cmd_ab`] lasts: long enough to pass a title's start, where most
/// of its shaders and pipelines are first made.
const WARM_UP_SECONDS: u64 = 20;

/// An A/B comparison as text: every run's frame rate for each build, their means, then the zones
/// whose share of the run moved most between the builds.
fn render_ab(before: &[BenchEntry], after: &[BenchEntry], top: usize) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let mean = |values: &[f64]| {
        (!values.is_empty())
            .then(|| values.iter().sum::<f64>() / orbistoun_perf::as_f64(values.len() as u64))
    };
    let shown = |value: Option<f64>| value.map_or_else(|| "-".to_owned(), |v| format!("{v:.1}"));
    for (name, runs) in [("baseline", before), ("this build", after)] {
        let rates: Vec<f64> = runs.iter().filter_map(|run| run.fps).collect();
        let medians: Vec<f64> = runs.iter().filter_map(|run| run.median_ms).collect();
        let _ = writeln!(
            out,
            "{name:<11} fps {:>6}  median ms {:>7}  runs: {}",
            shown(mean(&rates)),
            shown(mean(&medians)),
            rates
                .iter()
                .map(|fps| format!("{fps:.1}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
    }
    // Each zone's mean share of a run under each build; a zone absent from a run took none of it.
    let mut shares: std::collections::BTreeMap<&str, [f64; 2]> = std::collections::BTreeMap::new();
    for (side, runs) in [before, after].into_iter().enumerate() {
        let count = orbistoun_perf::as_f64(runs.len().max(1) as u64);
        for run in runs {
            for (zone, share) in run.shares() {
                shares.entry(zone).or_default()[side] += share / count;
            }
        }
    }
    let mut moved: Vec<(&str, [f64; 2])> = shares.into_iter().collect();
    moved.sort_by(|a, b| (b.1[1] - b.1[0]).abs().total_cmp(&(a.1[1] - a.1[0]).abs()));
    let _ = writeln!(
        out,
        "\nzones that moved, by self time as a share of the run (baseline -> this build)"
    );
    for (zone, [was, is]) in moved.into_iter().take(top) {
        let _ = writeln!(
            out,
            "  {zone:<28} {was:>6.1}% -> {is:>6.1}%  ({:+.1})",
            is - was
        );
    }
    out
}

/// Every title in the library, by its module: each title directory's `eboot.bin`, and each staged
/// build's under `data/homebrew`.
fn library() -> Vec<PathBuf> {
    let titles = orbistoun_paths::Paths::resolve().titles_dir();
    let modules = |dir: &Path| -> Vec<PathBuf> {
        let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .map(|entry| entry.path().join("eboot.bin"))
                    .filter(|module| module.is_file())
                    .collect()
            })
            .unwrap_or_default();
        found.sort();
        found
    };
    let staged = modules(&titles.join("data").join("homebrew"));
    // A title with a staged build is run from it, as `run` resolves it first (D722): its copy in
    // the library's own directory mounts another `/app0` and measures a run that cannot start.
    let staged_ids: std::collections::BTreeSet<String> = staged
        .iter()
        .filter_map(|module| crate::common::title_id(module))
        .collect();
    modules(&titles)
        .into_iter()
        .filter(|module| crate::common::title_id(module).is_none_or(|id| !staged_ids.contains(&id)))
        .chain(staged)
        .collect()
}

/// The share of the 3D engines something else is using, when the host can say: a benchmark
/// measured beside a game measures the game too.
fn others_on_the_gpu() -> Option<f64> {
    if !cfg!(windows) {
        return None;
    }
    let out = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "((Get-Counter '\\GPU Engine(*engtype_3D)\\Utilization Percentage' -ErrorAction SilentlyContinue).CounterSamples | Measure-Object CookedValue -Sum).Sum",
        ])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

/// How long a benchmark waits for something else to stop using the GPU before it skips a title,
/// and how often it asks meanwhile: another run beside it often ends within a minute.
const GPU_WAIT: std::time::Duration = std::time::Duration::from_secs(60);
const GPU_ASKED_EVERY: std::time::Duration = std::time::Duration::from_secs(5);

/// How busy something else keeps the GPU, when it still keeps it busier than a benchmark can
/// measure beside after [`GPU_WAIT`]; `None` once it is free enough.
fn gpu_still_busy() -> Option<f64> {
    let started = std::time::Instant::now();
    loop {
        let busy = others_on_the_gpu().filter(|&busy| busy > GPU_BUSY_MOST)?;
        if started.elapsed() >= GPU_WAIT {
            return Some(busy);
        }
        println!("  something else is using {busy:.0}% of the GPU; waiting");
        std::thread::sleep(GPU_ASKED_EVERY);
    }
}

/// The most of the GPU something else may be using for a benchmark to measure. An idle desktop
/// at 4K and 120 Hz keeps the 3D engines 12 to 19% busy; a game beside it, 88%.
const GPU_BUSY_MOST: f64 = 35.0;

/// `bench` - record every title in turn and rank the costs across them, against the last benchmark.
pub(crate) fn cmd_bench(
    only: &[String],
    (every, top): (Option<u64>, usize),
    at: Option<&str>,
) -> Result<()> {
    let tools = tools(false)?;
    // A revision's own build, so a benchmark measures a commit rather than whatever else the
    // working tree holds.
    let worker = at.map(build_revision).transpose()?;
    let library = library();
    // A name matching no title would bench nothing and say nothing: an option after `--` is read
    // as a title's name.
    let unknown: Vec<&String> = only
        .iter()
        .filter(|name| {
            !library
                .iter()
                .any(|module| crate::common::title_id(module).as_ref() == Some(*name))
        })
        .collect();
    if !unknown.is_empty() {
        anyhow::bail!(
            "no title in the library is named {}",
            unknown
                .iter()
                .map(|name| format!("\"{name}\""))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    let mut entries = Vec::new();
    for module in library {
        let id = crate::common::title_id(&module).unwrap_or_default();
        if !only.is_empty() && !only.contains(&id) {
            continue;
        }
        let title = id.clone();
        let skipped = |why: String| BenchEntry {
            title: title.clone(),
            fps: None,
            median_ms: None,
            p99_ms: None,
            by_self: Vec::new(),
            skipped: Some(why),
            seconds: None,
        };
        if let Some(busy) = gpu_still_busy() {
            println!("{title}: skipped - something else is using {busy:.0}% of the GPU");
            entries.push(skipped(format!("the GPU was {busy:.0}% busy")));
            continue;
        }
        let input = pad_script(&id);
        let seconds = run_seconds(&id, every);
        println!("{title}: recording {seconds} s");
        match record(
            &tools,
            &module,
            seconds,
            input.as_deref(),
            worker.as_deref(),
        ) {
            Ok(recorded) => entries.push(BenchEntry::of(title.clone(), recorded)),
            Err(error) => entries.push(skipped(format!("{error:#}"))),
        }
    }
    let dir = orbistoun_paths::Paths::resolve().data_root().join("bench");
    let previous = latest_bench(&dir);
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let file = dir.join(format!("{stamp}.json"));
    std::fs::write(&file, serde_json::to_string_pretty(&entries)?)
        .with_context(|| format!("writing {}", file.display()))?;
    println!("\n{}", render_bench(&entries, previous.as_deref(), top));
    println!("kept as {}", file.display());
    Ok(())
}

/// Each title's most recent measurement among the benchmarks kept in `dir`.
fn latest_bench(dir: &Path) -> Option<Vec<BenchEntry>> {
    let mut kept: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    kept.sort();
    // Each title's latest measurement, from whichever benchmark last measured it: a benchmark of
    // one title is not every other title's last.
    let mut latest: Vec<BenchEntry> = Vec::new();
    for path in kept.iter().rev() {
        let Some(entries) = std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Vec<BenchEntry>>(&bytes).ok())
        else {
            continue;
        };
        for entry in entries.into_iter().filter(|entry| entry.fps.is_some()) {
            if !latest.iter().any(|kept| kept.title == entry.title) {
                latest.push(entry);
            }
        }
    }
    (!latest.is_empty()).then_some(latest)
}

/// A benchmark as text: each title's frame rate and frame times beside the last benchmark's, then
/// the zones that cost most across all of them, each title weighing its own run equally.
fn render_bench(entries: &[BenchEntry], previous: Option<&[BenchEntry]>, top: usize) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{:<24} {:>8} {:>8} {:>10} {:>9}",
        "title", "fps", "was", "median ms", "p99 ms"
    );
    let shown = |value: Option<f64>| value.map_or_else(|| "-".to_owned(), |v| format!("{v:.1}"));
    for entry in entries {
        let was = previous
            .and_then(|previous| previous.iter().find(|e| e.title == entry.title))
            .and_then(|e| e.fps);
        let _ = writeln!(
            out,
            "{:<24} {:>8} {:>8} {:>10} {:>9}{}",
            entry.title,
            shown(entry.fps),
            shown(was),
            shown(entry.median_ms),
            shown(entry.p99_ms),
            entry
                .skipped
                .as_deref()
                .map_or_else(String::new, |why| format!("  ({why})")),
        );
    }
    // Each zone's self time as a share of its title's recording, so a long-running title does not
    // drown the rest - and an idle one, whose only zone is a few milliseconds of presenting in a run
    // of seconds, does not rank that zone as all of its time. A benchmark kept before recordings
    // carried their length shares its zones' own total.
    let mut weight: std::collections::BTreeMap<&str, (f64, usize)> =
        std::collections::BTreeMap::new();
    for entry in entries {
        let zones: u64 = entry.by_self.iter().map(|zone| zone.total_ns).sum();
        let whole = entry
            .seconds
            .map_or(orbistoun_perf::as_f64(zones), |seconds| seconds * 1e9);
        if whole <= 0.0 {
            continue;
        }
        for zone in &entry.by_self {
            let slot = weight.entry(&zone.name).or_default();
            slot.0 += orbistoun_perf::as_f64(zone.total_ns) / whole;
            slot.1 += 1;
        }
    }
    let measured = entries
        .iter()
        .filter(|entry| !entry.by_self.is_empty())
        .count()
        .max(1);
    let mut ranked: Vec<(&str, f64, usize)> = weight
        .into_iter()
        .map(|(name, (share, titles))| {
            (
                name,
                share * 100.0 / orbistoun_perf::as_f64(measured as u64),
                titles,
            )
        })
        .collect();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
    let _ = writeln!(
        out,
        "\ncosts across titles, by self time as a share of each title's run (each weighed equally)"
    );
    for (name, share, titles) in ranked.into_iter().take(top) {
        let _ = writeln!(out, "  {name:<28} {share:>6.1}%  in {titles} title(s)");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{BenchEntry, render_ab, render_bench};

    use orbistoun_perf::ZoneStat;

    fn entry(title: &str, fps: f64, zones: &[(&str, u64)]) -> BenchEntry {
        BenchEntry {
            title: title.to_owned(),
            fps: Some(fps),
            median_ms: Some(1000.0 / fps),
            p99_ms: None,
            by_self: zones
                .iter()
                .map(|&(name, total_ns)| ZoneStat {
                    name: name.to_owned(),
                    total_ns,
                    count: 1,
                    max_ns: total_ns,
                })
                .collect(),
            skipped: None,
            seconds: Some(1.0),
        }
    }

    /// Each title weighs equally, a zone counting as the share of its title's run it took: half of
    /// a busy title's run outranks the few milliseconds an idle title spent presenting, though they
    /// are all of the idle title's zones.
    #[test]
    fn costs_across_titles_are_shares_of_each_title_s_run() {
        let entries = [
            entry("BUSY", 30.0, &[("draw", 500_000_000)]),
            entry("IDLE", 1.0, &[("present", 8_000_000)]),
        ];
        let previous = [entry("BUSY", 20.0, &[])];
        let text = render_bench(&entries, Some(&previous), 5);
        let line = |zone: &str| {
            text.lines()
                .find(|line| line.trim_start().starts_with(zone))
                .unwrap_or_default()
                .to_owned()
        };
        assert!(line("draw").contains("25.0%"), "{text}");
        assert!(line("present").contains("0.4%"), "{text}");
        assert!(
            text.find("  draw") < text.find("  present"),
            "draw ranks first: {text}"
        );
        assert!(text.contains("20.0"), "the last benchmark's rate: {text}");

        // A benchmark kept without its recordings' lengths shares each title's zones' own total.
        let mut old = entry("OLD", 30.0, &[("draw", 1_000)]);
        old.seconds = None;
        assert!(render_bench(&[old], None, 5).contains("100.0%"));
    }

    /// An A/B report gives each build's mean rate, and ranks zones by how far their share of the
    /// run moved, a zone missing from one build counting as none of it.
    #[test]
    fn ab_ranks_zones_by_how_far_they_moved() {
        let before = [
            entry(
                "baseline",
                10.0,
                &[("build", 400_000_000), ("draw", 100_000_000)],
            ),
            entry(
                "baseline",
                20.0,
                &[("build", 200_000_000), ("draw", 100_000_000)],
            ),
        ];
        let after = [entry(
            "this build",
            30.0,
            &[("draw", 120_000_000), ("new", 50_000_000)],
        )];
        let text = render_ab(&before, &after, 5);
        assert!(text.contains("fps   15.0"), "{text}");
        assert!(text.contains("fps   30.0"), "{text}");
        assert!(text.contains("runs: 10.0 20.0"), "{text}");
        let order = |zone: &str| text.find(&format!("  {zone} ")).unwrap_or(usize::MAX);
        assert!(
            order("build") < order("new") && order("new") < order("draw"),
            "{text}"
        );
        assert!(text.contains("30.0% ->    0.0%  (-30.0)"), "{text}");
    }
}
