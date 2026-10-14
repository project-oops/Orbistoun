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
    (input, host_clock): (Option<&Path>, bool),
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
    if host_clock {
        // SAFETY: as above.
        unsafe { std::env::set_var(orbistoun_env::CLOCK.name, "host") };
    }
    // Longer than the run, so the capture ends when the worker's client goes, with all of it.
    let capture = orbistoun_perf::Capture::start(tools, &trace, port, seconds + 120)
        .context("starting tracy-capture")?;
    let ran = run_worker(path, seconds, input);
    if let Err(error) = ran {
        capture.abandon();
        return Err(error);
    }
    let (trace, summary) = capture.finish().context("finishing the capture")?;
    let report = orbistoun_perf::Report::read(tools, &trace).map_err(anyhow::Error::msg)?;
    Ok(Recorded {
        trace,
        summary,
        report,
    })
}

/// One run of `path` in a worker, stopped after `seconds`.
fn run_worker(path: &Path, seconds: u64, input: Option<&Path>) -> Result<()> {
    let mut worker =
        orbistoun_worker::WorkerHandle::spawn_self().context("spawning a worker process")?;
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
    for event in &events {
        match event {
            orbistoun_proto::Event::Terminated { outcome, .. } => println!("  outcome {outcome:?}"),
            orbistoun_proto::Event::Failed { error } => println!("  failed {error}"),
            _ => {}
        }
    }
    worker.shutdown().context("shutting the worker down")?;
    Ok(())
}

/// `perf` - record a run of one title and rank where its time went.
pub(crate) fn cmd_perf(
    path: Option<&Path>,
    (seconds, top): (u64, usize),
    (input, host_clock): (Option<&Path>, bool),
    (open, install): (bool, bool),
) -> Result<()> {
    let tools = tools(install)?;
    let Some(path) = path else {
        println!("Tracy tools ready in {}", tools.dir().display());
        return Ok(());
    };
    // As a benchmark runs the title: its compat record's pad script unless one is named, and the
    // host clock for a staged homebrew build.
    let title = crate::common::title_id(path).unwrap_or_default();
    let recorded_input = pad_script(&title);
    let input = input.or(recorded_input.as_deref());
    let recorded = record(&tools, path, seconds, (input, host_clock || staged(path)))?;
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

/// The pad script `title`'s compat record replays, when it has one.
fn pad_script(title: &str) -> Option<PathBuf> {
    let inputs = crate::records::title_dir(Path::new("compat"), title)
        .join(orbistoun_overrides::INPUTS_FILE);
    inputs.is_file().then_some(inputs)
}

/// Whether `module` is a staged homebrew build, under the library's `data/homebrew`, which plays on
/// the host clock.
fn staged(module: &Path) -> bool {
    let mut above = module.ancestors().skip(2).map(Path::file_name);
    above.next() == Some(Some(std::ffi::OsStr::new("homebrew")))
        && above.next() == Some(Some(std::ffi::OsStr::new("data")))
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
}

/// Every title in the library, by its module: each title directory's `eboot.bin`, and each staged
/// build's under `data/homebrew`, which run on the host clock.
fn library() -> Vec<(PathBuf, bool)> {
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
    modules(&titles)
        .into_iter()
        .map(|module| (module, false))
        .chain(
            modules(&titles.join("data").join("homebrew"))
                .into_iter()
                .map(|module| (module, true)),
        )
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

/// The most of the GPU something else may be using for a benchmark to measure. An idle desktop
/// at 4K and 120 Hz keeps the 3D engines 12 to 19% busy; a game beside it, 88%.
const GPU_BUSY_MOST: f64 = 35.0;

/// `bench` - record every title in turn and rank the costs across them, against the last benchmark.
pub(crate) fn cmd_bench(only: &[String], seconds: u64, top: usize) -> Result<()> {
    let tools = tools(false)?;
    let library = library();
    // A name matching no title would bench nothing and say nothing: an option after `--` is read
    // as a title's name.
    let unknown: Vec<&String> = only
        .iter()
        .filter(|name| {
            !library
                .iter()
                .any(|(module, _)| crate::common::title_id(module).as_ref() == Some(*name))
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
    for (module, homebrew) in library {
        let title = crate::common::title_id(&module).unwrap_or_default();
        if !only.is_empty() && !only.contains(&title) {
            continue;
        }
        let skipped = |why: String| BenchEntry {
            title: title.clone(),
            fps: None,
            median_ms: None,
            p99_ms: None,
            by_self: Vec::new(),
            skipped: Some(why),
        };
        if let Some(busy) = others_on_the_gpu().filter(|&busy| busy > GPU_BUSY_MOST) {
            println!("{title}: skipped - something else is using {busy:.0}% of the GPU");
            entries.push(skipped(format!("the GPU was {busy:.0}% busy")));
            continue;
        }
        let input = pad_script(&title);
        println!("{title}: recording {seconds} s");
        match record(&tools, &module, seconds, (input.as_deref(), homebrew)) {
            Ok(recorded) => entries.push(BenchEntry {
                title: title.clone(),
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

/// The most recent benchmark kept in `dir`.
fn latest_bench(dir: &Path) -> Option<Vec<BenchEntry>> {
    let latest = std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .max()?;
    serde_json::from_slice(&std::fs::read(latest).ok()?).ok()
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
    // Each title's self time as shares of its own, so a long-running title does not drown the rest.
    let mut weight: std::collections::BTreeMap<&str, (f64, usize)> =
        std::collections::BTreeMap::new();
    for entry in entries {
        let whole: u64 = entry.by_self.iter().map(|zone| zone.total_ns).sum();
        if whole == 0 {
            continue;
        }
        for zone in &entry.by_self {
            let slot = weight.entry(&zone.name).or_default();
            slot.0 += orbistoun_perf::as_f64(zone.total_ns) / orbistoun_perf::as_f64(whole);
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
        "\ncosts across titles, by self time (each title weighed equally)"
    );
    for (name, share, titles) in ranked.into_iter().take(top) {
        let _ = writeln!(out, "  {name:<28} {share:>6.1}%  in {titles} title(s)");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{BenchEntry, render_bench, staged};

    /// A module under the library's `data/homebrew` is staged; a title's own is not.
    #[test]
    fn staged_homebrew_is_told_by_where_it_lies() {
        let library = std::path::Path::new("titles");
        assert!(staged(&library.join("data/homebrew/STKT00001/eboot.bin")));
        assert!(!staged(&library.join("MDEM00001/eboot.bin")));
        assert!(!staged(&library.join("homebrew/STKT00001/eboot.bin")));
    }
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
        }
    }

    /// Each title weighs equally: a zone that is all of a short title's time outranks one that is
    /// a small part of a long title's, however many nanoseconds the long one spent in it.
    #[test]
    fn costs_across_titles_weigh_each_title_equally() {
        let entries = [
            entry(
                "LONG",
                30.0,
                &[("draw", 9_000_000), ("dispatch", 1_000_000)],
            ),
            entry("SHORT", 60.0, &[("dispatch", 1_000)]),
        ];
        let previous = [entry("LONG", 20.0, &[])];
        let text = render_bench(&entries, Some(&previous), 5);
        let dispatch = text.find("dispatch").expect("ranked");
        let draw = text
            .lines()
            .position(|line| line.trim_start().starts_with("draw"));
        let dispatch_line = text
            .lines()
            .position(|line| line.trim_start().starts_with("dispatch"));
        assert!(dispatch_line < draw, "{text}");
        assert!(text[dispatch..].contains("55.0%"), "{text}");
        assert!(text.contains("20.0"), "the last benchmark's rate: {text}");
    }
}
