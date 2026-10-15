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
    report.note_device_waits(
        &orbistoun_gpu::perf::DEVICE_WAITS.map(orbistoun_gpu::perf::Span::label),
    );
    report.note_emptied(&orbistoun_gpu::perf::EMPTIED.map(orbistoun_gpu::perf::Span::label));
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
    (open, install, sample): (bool, bool, bool),
) -> Result<()> {
    let tools = tools(install)?;
    if sample {
        // SAFETY: single-threaded here; the spawned worker inherits the variable at startup.
        unsafe { std::env::set_var(orbistoun_env::PROFILE.name, SAMPLED_BUCKETS.to_string()) };
    }
    let Some(path) = path else {
        println!("Tracy tools ready in {}", tools.dir().display());
        return Ok(());
    };
    // As a benchmark runs the title: its compat record's pad script unless one is named.
    let title = crate::common::title_id(path).unwrap_or_default();
    let recorded_input = pad_script(&title);
    let input = input.or(recorded_input.as_deref());
    let seconds = run_seconds(&title, seconds);
    let beside = before_measuring();
    let recorded = record(&tools, path, seconds, input, None)?;
    let mut text = format!(
        "{}{}  {} zones, {:.1} s recorded, {:.1} fps by the capture's own count\n\n{}",
        beside.map(|line| format!("{line}\n")).unwrap_or_default(),
        recorded.trace.display(),
        recorded.summary.zones,
        recorded.summary.seconds,
        recorded.summary.fps(),
        recorded.report.render(top)
    );
    if sample {
        let log = std::fs::read_to_string(recorded.trace.with_extension("log")).unwrap_or_default();
        text.push_str(&last_samples(&log));
    }
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

/// How many host functions a sampled report names per thread.
const SAMPLED_BUCKETS: usize = 20;

/// The share of a thread's samples in one place from which a sampled report shows only that place.
const IDLE_SHARE: f64 = 90.0;

/// The last sample report of each thread in a run's log (`ORBISTOUN_PROFILE`): a heading and its
/// indented lines, the guest's thread and the device thread each once, as the run last printed them.
/// Empty when the log holds none.
fn last_samples(log: &str) -> String {
    use std::fmt::Write as _;
    let mut blocks: Vec<(String, Vec<&str>)> = Vec::new();
    let mut lines = log.lines().peekable();
    while let Some(line) = lines.next() {
        let Some(at) = line.find("profile of the ") else {
            continue;
        };
        let heading = line[at..].trim_end_matches(':').to_owned();
        let thread = heading
            .split(" thread")
            .next()
            .unwrap_or_default()
            .to_owned();
        let mut body = Vec::new();
        while let Some(next) = lines.peek() {
            if !next.starts_with("   ") {
                break;
            }
            body.push(*next);
            lines.next();
        }
        blocks.retain(|(kept, _)| kept.split(" thread").next() != Some(thread.as_str()));
        blocks.push((heading, body));
    }
    let mut out = String::new();
    for (heading, body) in blocks {
        // A thread spending nearly all of its samples in one place - a job worker parked on its
        // wait - is that one line: a title's many workers would bury the thread doing the work.
        let top = body
            .first()
            .and_then(|line| line.trim().split('%').next()?.trim().parse::<f64>().ok());
        let shown = if top.is_some_and(|share| share >= IDLE_SHARE) {
            1
        } else {
            body.len()
        };
        let _ = writeln!(out, "\nsampled: {heading}");
        for line in body.iter().take(shown) {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
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
    /// The share of the run spent waiting on the device (`DEVICE_WAITS`): a title waiting for much
    /// of its run is bound by the device's work, not by anything a host-side change makes faster.
    #[serde(default)]
    device_waits: Option<f64>,
    /// How busy something else kept the host's cores as it was recorded, when it was more than an
    /// idle desktop does: a title bound by its guest thread is slower beside it, and a frame rate
    /// read without it reads as a regression (PPSA25872: 72 fps beside 33%, 201 beside 15%).
    #[serde(default)]
    beside_cpu: Option<f64>,
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
            device_waits: recorded.report.device_waits,
            beside_cpu: None,
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
    (baseline, against, with): (Option<&Path>, Option<&str>, &[PathBuf]),
    (seconds, rounds, top): (Option<u64>, usize, usize),
    (input, warm_up, check): (Option<&Path>, bool, bool),
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
    // The revision with the named files, rather than this build and all the working tree holds.
    let candidate = match (against, with.is_empty()) {
        (_, true) => None,
        (Some(revision), false) => Some(build_revision_with(revision, with)?),
        (None, false) => anyhow::bail!("--with lays files over a revision: name it with --against"),
    };
    let candidate = candidate.as_deref();
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
    let builds = [("baseline", Some(baseline)), ("this build", candidate)];
    if warm_up {
        for (which, exe) in builds {
            println!("warming up: {which}, {} s", seconds.min(WARM_UP_SECONDS));
            record(&tools, path, seconds.min(WARM_UP_SECONDS), input, exe)?;
        }
    }
    let (mut before, mut after) = (Vec::new(), Vec::new());
    let mut beside = Vec::new();
    for round in 0..rounds {
        let order = if round % 2 == 0 {
            builds
        } else {
            [builds[1], builds[0]]
        };
        for (which, exe) in order {
            println!("round {}/{rounds}: {which}, {seconds} s", round + 1);
            if let Some(line) = before_measuring() {
                beside.push(format!("round {} ({which}) {line}\n", round + 1));
            }
            let recorded = record(&tools, path, seconds, input, exe)?;
            let entry = BenchEntry::of(which.to_owned(), recorded);
            if which == builds[0].0 {
                before.push(entry);
            } else {
                after.push(entry);
            }
        }
    }
    let frames = if check {
        println!("checking frames on the logical clock");
        Some(check_frames(path, (seconds, input), (baseline, candidate))?)
    } else {
        None
    };
    let text = format!(
        "{} against {}\n{}\n{}{}",
        path.display(),
        baseline.display(),
        beside.concat(),
        render_ab(&before, &after, top),
        frames.map(|line| format!("\n{line}\n")).unwrap_or_default()
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

/// [`build_revision`] with `files` laid over it as the working tree holds them.
fn build_revision_with(revision: &str, files: &[PathBuf]) -> Result<PathBuf> {
    let dir = orbistoun_paths::Paths::resolve().data_root().join("ab");
    println!(
        "building {revision} with {} file(s) into {}",
        files.len(),
        dir.display()
    );
    orbistoun_perf::baseline::build_with(
        &std::env::current_dir().context("finding the repository")?,
        revision,
        files,
        &dir,
    )
    .map_err(anyhow::Error::msg)
}

/// Whether a build draws what the baseline does: each run on the logical clock with its submissions
/// carried out as they are made, so a run repeats, keeping every [`FRAMES_KEPT_EVERY`]th flip - the
/// baseline twice, and this build once. A flip the baseline's two runs disagree at drifts on its own
/// and says nothing; the rest are compared with this build's. A change made for speed changes no
/// frame (PPSA02664's late flips drift; a prototype that ran it six times faster was told apart from
/// a broken one only this way).
fn check_frames(
    path: &Path,
    (seconds, input): (u64, Option<&Path>),
    (baseline, candidate): (&Path, Option<&Path>),
) -> Result<String> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let root = orbistoun_paths::Paths::resolve()
        .data_root()
        .join("ab")
        .join("frames")
        .join(stamp.to_string());
    let runs = [
        ("baseline-1", Some(baseline)),
        ("baseline-2", Some(baseline)),
        ("this-build", candidate),
    ];
    for (name, exe) in runs {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        let every = FRAMES_KEPT_EVERY.to_string();
        set_env(&[
            (orbistoun_env::CLOCK.name, Some("logical".as_ref())),
            (orbistoun_env::SUBMIT.name, None),
            (orbistoun_env::FRAME_EVERY.name, Some(every.as_ref())),
            (orbistoun_env::FRAME_DIR.name, Some(dir.as_os_str())),
        ]);
        println!("  {name}, {seconds} s");
        let ran = run_worker(path, seconds, (input, exe), &dir.join("run.log"));
        set_env(&[
            (orbistoun_env::FRAME_EVERY.name, None),
            (orbistoun_env::FRAME_DIR.name, None),
        ]);
        for (var, value) in orbistoun_env::AS_PLAYED {
            set_env(&[(var.name, Some(value.as_ref()))]);
        }
        ran?;
    }
    let read = |name: &str| -> std::collections::BTreeMap<String, Vec<u8>> {
        std::fs::read_dir(root.join(name))
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .filter_map(|entry| {
                        let name = entry.file_name().to_string_lossy().into_owned();
                        let flip = name.starts_with("flip-")
                            && Path::new(&name)
                                .extension()
                                .is_some_and(|ext| ext.eq_ignore_ascii_case("rgba"));
                        flip.then(|| std::fs::read(entry.path()).ok().map(|bytes| (name, bytes)))
                            .flatten()
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    Ok(frames_verdict(
        &read("baseline-1"),
        &read("baseline-2"),
        &read("this-build"),
        &root,
    ))
}

/// What [`check_frames`] found: of the flips the baseline's two runs kept alike, how many this
/// build kept alike too, and the first it did not.
fn frames_verdict(
    first: &std::collections::BTreeMap<String, Vec<u8>>,
    second: &std::collections::BTreeMap<String, Vec<u8>>,
    this: &std::collections::BTreeMap<String, Vec<u8>>,
    root: &Path,
) -> String {
    let repeated: Vec<&String> = first
        .iter()
        .filter(|(name, bytes)| second.get(*name) == Some(bytes))
        .map(|(name, _)| name)
        .collect();
    let compared: Vec<&String> = repeated
        .iter()
        .copied()
        .filter(|name| this.contains_key(*name))
        .collect();
    let differing: Vec<&String> = compared
        .iter()
        .copied()
        .filter(|name| this.get(*name) != first.get(*name))
        .collect();
    let verdict = if compared.is_empty() {
        "nothing to compare".to_owned()
    } else if differing.is_empty() {
        format!("this build drew all {} alike", compared.len())
    } else {
        format!(
            "this build drew {} of {} otherwise, first {}",
            differing.len(),
            compared.len(),
            differing[0]
        )
    };
    format!(
        "frames: the baseline kept {} of {} flips alike across two runs; {verdict} ({})",
        repeated.len(),
        first.len(),
        root.display()
    )
}

/// Sets each variable, or removes it where `None`, for the workers spawned after.
fn set_env(vars: &[(&str, Option<&std::ffi::OsStr>)]) {
    for &(name, value) in vars {
        match value {
            // SAFETY: single-threaded here; the spawned worker inherits the variables at startup.
            Some(value) => unsafe { std::env::set_var(name, value) },
            // SAFETY: as above.
            None => unsafe { std::env::remove_var(name) },
        }
    }
}

/// Every how many flips a frame check keeps one.
const FRAMES_KEPT_EVERY: u32 = 30;

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

/// Waits, as a benchmark does, for something else to stop using the GPU before a measured run;
/// when it does not, the line the run's report carries, since the run measures that too. Another
/// session's run on the same device once made one in three runs of a title 2.7 times slower, its
/// device time per dispatch 23 ms rather than 10 (PPSA28061).
///
/// A busy CPU is not waited for - a virtual machine or a build beside the run may last all day - but
/// said the same way: a title bound by its guest thread loses a quarter of its frame rate beside
/// ten busy cores (NVRB00001, 9.9 -> 7.4 fps, the same build).
fn before_measuring() -> Option<String> {
    let mut beside = Vec::new();
    if let Some(busy) = gpu_still_busy() {
        beside.push(format!("{busy:.0}% of the GPU"));
    }
    if let Some(busy) = cpu_busy() {
        beside.push(format!("{busy:.0}% of the CPU"));
    }
    if beside.is_empty() {
        return None;
    }
    let line = format!(
        "measured beside something else using {}",
        beside.join(" and ")
    );
    println!("  {line}");
    Some(line)
}

/// How busy something else keeps the host's cores, over two seconds, when it is more than
/// [`CPU_BUSY_MOST`]; asked before a run starts, so none of it is the run's.
fn cpu_busy() -> Option<f64> {
    if !cfg!(windows) {
        return None;
    }
    let out = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "((Get-Counter '\\Processor(_Total)\\% Processor Time' -SampleInterval 1 -MaxSamples 2 -ErrorAction SilentlyContinue).CounterSamples | Measure-Object CookedValue -Average).Average",
        ])
        .output()
        .ok()?;
    let busy: f64 = String::from_utf8_lossy(&out.stdout).trim().parse().ok()?;
    (busy > CPU_BUSY_MOST).then_some(busy)
}

/// The most of the host's cores something else may be using before a run says so: an idle desktop
/// keeps them 2 to 5% busy.
const CPU_BUSY_MOST: f64 = 10.0;

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
            device_waits: None,
            beside_cpu: None,
        };
        if let Some(busy) = gpu_still_busy() {
            println!("{title}: skipped - something else is using {busy:.0}% of the GPU");
            entries.push(skipped(format!("the GPU was {busy:.0}% busy")));
            continue;
        }
        let beside_cpu = cpu_busy();
        if let Some(busy) = beside_cpu {
            println!("  measured beside something else using {busy:.0}% of the CPU");
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
            Ok(recorded) => entries.push(BenchEntry {
                beside_cpu,
                ..BenchEntry::of(title.clone(), recorded)
            }),
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
        "{:<24} {:>8} {:>8} {:>10} {:>9} {:>9}",
        "title", "fps", "was", "median ms", "p99 ms", "device %"
    );
    let shown = |value: Option<f64>| value.map_or_else(|| "-".to_owned(), |v| format!("{v:.1}"));
    for entry in entries {
        let was = previous
            .and_then(|previous| previous.iter().find(|e| e.title == entry.title))
            .and_then(|e| e.fps);
        let _ = writeln!(
            out,
            "{:<24} {:>8} {:>8} {:>10} {:>9} {:>9}{}",
            entry.title,
            shown(entry.fps),
            shown(was),
            shown(entry.median_ms),
            shown(entry.p99_ms),
            shown(entry.device_waits),
            entry
                .skipped
                .as_deref()
                .map(|why| format!("  ({why})"))
                .or_else(|| {
                    entry
                        .beside_cpu
                        .map(|busy| format!("  (beside {busy:.0}% of the CPU in use)"))
                })
                .unwrap_or_default(),
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
            device_waits: None,
            beside_cpu: None,
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

        // A title's share of its run spent waiting on the device stands beside its frame times.
        let mut bound = entry("BOUND", 30.0, &[("draw", 1_000)]);
        bound.device_waits = Some(35.0);
        let text = render_bench(&[bound], None, 5);
        assert!(text.contains("device %"), "{text}");
        assert!(
            text.lines()
                .find(|l| l.starts_with("BOUND"))
                .is_some_and(|l| l.trim_end().ends_with("35.0")),
            "{text}"
        );

        // A title recorded beside a busy host says so on its line.
        let mut beside = entry("BESIDE", 30.0, &[("draw", 1_000)]);
        beside.beside_cpu = Some(33.0);
        let text = render_bench(&[beside], None, 5);
        assert!(
            text.lines()
                .find(|l| l.starts_with("BESIDE"))
                .is_some_and(|l| l.contains("(beside 33% of the CPU in use)")),
            "{text}"
        );
    }

    /// A sampled report keeps each thread's last block from the log, heading and indented lines,
    /// and nothing of the log around them.
    #[test]
    fn the_last_sample_of_each_thread_is_kept() {
        let log = concat!(
            "2026 INFO profile of the guest main thread, 10 samples, 1% in the guest's image:\n",
            "   90.0% old main\n",
            "2026 INFO profile of the device thread, 10 samples, 0% in the guest's image:\n",
            "   50.0% device work\n",
            "[TITLE] a line of the guest's\n",
            "2026 INFO profile of the guest main thread, 12 samples, 2% in the guest's image:\n",
            "   80.0% new main\n",
            "    1.0% more\n",
            "2026 INFO profile of the guest 3 thread, 12 samples, 0% in the guest's image:\n",
            "   94.0% parked in its wait\n",
            "    3.0% waking\n",
            "2026 INFO the guest was still running\n",
        );
        let kept = super::last_samples(log);
        assert!(
            kept.contains("parked in its wait") && !kept.contains("waking"),
            "a parked thread is its one line: {kept}"
        );
        assert!(
            kept.contains("new main") && kept.contains("    1.0% more"),
            "{kept}"
        );
        assert!(kept.contains("device work"), "{kept}");
        assert!(!kept.contains("old main"), "{kept}");
        assert!(!kept.contains("still running"), "{kept}");
        assert!(super::last_samples("nothing sampled").is_empty());
    }

    /// A frame check compares this build only where the baseline's two runs agree, and names the
    /// first flip it drew otherwise.
    #[test]
    fn frames_are_compared_where_the_baseline_repeats() {
        let frames = |pairs: &[(&str, u8)]| -> std::collections::BTreeMap<String, Vec<u8>> {
            pairs
                .iter()
                .map(|&(name, byte)| (name.to_owned(), vec![byte; 4]))
                .collect()
        };
        let first = frames(&[("flip-1", 1), ("flip-2", 2), ("flip-3", 3)]);
        let second = frames(&[("flip-1", 1), ("flip-2", 2), ("flip-3", 9)]);
        let root = std::path::Path::new("frames");
        let alike = super::frames_verdict(&first, &second, &first, root);
        assert!(
            alike.contains("kept 2 of 3") && alike.contains("drew all 2 alike"),
            "{alike}"
        );
        let other = frames(&[("flip-1", 1), ("flip-2", 7), ("flip-3", 5)]);
        let differs = super::frames_verdict(&first, &second, &other, root);
        assert!(
            differs.contains("drew 1 of 2 otherwise, first flip-2"),
            "{differs}"
        );
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
