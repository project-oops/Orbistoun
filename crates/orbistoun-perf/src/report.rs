//! A recorded run read back: where the time went, ranked, and how long its frames took.

use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;

use crate::{Tools, as_f64};

/// One zone's time across a capture, as `tracy-csvexport` totals it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ZoneStat {
    /// The span's name, as the run report names it.
    pub name: String,
    /// Nanoseconds in it, altogether.
    pub total_ns: u64,
    /// How many times it ran.
    pub count: u64,
    /// The longest once, in nanoseconds.
    pub max_ns: u64,
}

impl ZoneStat {
    /// Nanoseconds a time, on average.
    #[must_use]
    pub fn mean_ns(&self) -> u64 {
        self.total_ns.checked_div(self.count).unwrap_or(0)
    }
}

/// How long each frame - guest flip to guest flip - took, in milliseconds.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct FrameTimes {
    /// Every frame's time, in the order they ran.
    pub ms: Vec<f64>,
}

impl FrameTimes {
    /// The frame time below which `percent` of frames fall, nearest-rank; `None` with no frames.
    #[must_use]
    pub fn percentile(&self, percent: f64) -> Option<f64> {
        let mut sorted = self.ms.clone();
        sorted.sort_by(f64::total_cmp);
        let last = sorted.len().checked_sub(1)?;
        let rank = ((percent / 100.0) * as_f64(sorted.len() as u64)).ceil() as usize;
        sorted.get(rank.saturating_sub(1).min(last)).copied()
    }

    /// Frames a second over the frames recorded: their count over their summed time.
    #[must_use]
    pub fn fps(&self) -> Option<f64> {
        let total: f64 = self.ms.iter().sum();
        (total > 0.0).then(|| as_f64(self.ms.len() as u64) * 1e3 / total)
    }
}

/// A recorded run, read back.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct Report {
    /// Zones by their own time, children excluded - what to make faster.
    pub by_self: Vec<ZoneStat>,
    /// Zones by their whole time, children included - where in the run the time sits.
    pub by_total: Vec<ZoneStat>,
    /// Its frame times.
    pub frames: FrameTimes,
    /// The share of the run its threads spent waiting for the device, over the spans
    /// [`Self::note_device_waits`] was given; `None` until it is.
    #[serde(default)]
    pub device_waits: Option<f64>,
}

/// The plot each guest flip adds its frame time to (D829).
const FRAME_PLOT: &str = "frame ms";

impl Report {
    /// Reads `trace` back with `tools`' exporter: zones by self time and by total time, and the
    /// frame-time plot.
    ///
    /// # Errors
    ///
    /// When the exporter cannot be run, or fails on the file.
    pub fn read(tools: &Tools, trace: &Path) -> Result<Self, String> {
        let export = |args: &[&str]| -> Result<String, String> {
            let out = Command::new(tools.exporter())
                .args(args)
                .arg(trace)
                .output()
                .map_err(|e| format!("running {}: {e}", tools.exporter().display()))?;
            if !out.status.success() {
                return Err(format!(
                    "{} failed on {}: {}",
                    tools.exporter().display(),
                    trace.display(),
                    String::from_utf8_lossy(&out.stderr).trim()
                ));
            }
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        };
        Ok(Self {
            by_self: sorted(parse_zones(&export(&["--self"])?)),
            by_total: sorted(parse_zones(&export(&[])?)),
            frames: FrameTimes {
                ms: parse_plot(
                    &export(&["--unwrap", "--plot", "--filter", FRAME_PLOT])?,
                    FRAME_PLOT,
                ),
            },
            device_waits: None,
        })
    }

    /// The report as text: frame times, then the `top` zones by self time and by total time.
    #[must_use]
    pub fn render(&self, top: usize) -> String {
        let mut out = String::new();
        let ms = |value: Option<f64>| value.map_or_else(|| "-".to_owned(), |v| format!("{v:.2}"));
        let _ = writeln!(
            out,
            "frames {}  fps {}  frame ms: median {}  p95 {}  p99 {}  worst {}",
            self.frames.ms.len(),
            self.frames
                .fps()
                .map_or_else(|| "-".to_owned(), |fps| format!("{fps:.1}")),
            ms(self.frames.percentile(50.0)),
            ms(self.frames.percentile(95.0)),
            ms(self.frames.percentile(99.0)),
            ms(self.frames.percentile(100.0)),
        );
        if let Some(share) = self.device_waits {
            let _ = writeln!(
                out,
                concat!(
                    "waiting on the device: {:.1}% of the run - time taken out of one wait there ",
                    "shows up in another while the device's own work is the bound"
                ),
                share
            );
        }
        for (title, zones) in [
            ("by self time (what to make faster)", &self.by_self),
            ("by total time (where the time sits)", &self.by_total),
        ] {
            let whole: u64 = self.by_self.iter().map(|zone| zone.total_ns).sum();
            let _ = writeln!(out, "\n{title}");
            let _ = writeln!(
                out,
                "  {:<28} {:>10} {:>7} {:>10} {:>10} {:>10}",
                "zone", "total ms", "share", "count", "mean us", "max us"
            );
            for zone in zones.iter().take(top) {
                let share = if whole > 0 {
                    as_f64(zone.total_ns) * 100.0 / as_f64(whole)
                } else {
                    0.0
                };
                let _ = writeln!(
                    out,
                    "  {:<28} {:>10.1} {:>6.1}% {:>10} {:>10.1} {:>10.0}",
                    zone.name,
                    as_f64(zone.total_ns) / 1e6,
                    share,
                    zone.count,
                    as_f64(zone.mean_ns()) / 1e3,
                    as_f64(zone.max_ns) / 1e3,
                );
            }
        }
        out
    }
}

impl Report {
    /// Notes how much of the run the zones named `waits` - each the host waiting for the device -
    /// took together, as a share of every zone's own time.
    pub fn note_device_waits(&mut self, waits: &[&str]) {
        let whole: u64 = self.by_self.iter().map(|zone| zone.total_ns).sum();
        let waited: u64 = self
            .by_self
            .iter()
            .filter(|zone| waits.contains(&zone.name.as_str()))
            .map(|zone| zone.total_ns)
            .sum();
        self.device_waits = (whole > 0).then(|| as_f64(waited) * 100.0 / as_f64(whole));
    }

    /// Takes `done`'s whole time out of `waiting`'s own: `waiting` is a thread waiting on `done`,
    /// which runs on another thread, so the profiler counts it in both (D831). What is left is the
    /// hand-off itself.
    pub fn waits_on(&mut self, waiting: &str, done: &str) {
        let Some(done) = self
            .by_total
            .iter()
            .find(|zone| zone.name == done)
            .map(|zone| zone.total_ns)
        else {
            return;
        };
        if let Some(zone) = self.by_self.iter_mut().find(|zone| zone.name == waiting) {
            zone.total_ns = zone.total_ns.saturating_sub(done);
        }
        self.by_self = sorted(std::mem::take(&mut self.by_self));
    }
}

/// Zones, most time first.
fn sorted(mut zones: Vec<ZoneStat>) -> Vec<ZoneStat> {
    zones.sort_by(|a, b| {
        b.total_ns
            .cmp(&a.total_ns)
            .then_with(|| a.name.cmp(&b.name))
    });
    zones
}

/// `tracy-csvexport`'s statistics: a header, then
/// `name,src_file,src_line,total_ns,total_perc,counts,mean_ns,min_ns,max_ns,std_ns` a zone. Read
/// from the right, so a comma in a name stays in the name.
fn parse_zones(csv: &str) -> Vec<ZoneStat> {
    csv.lines()
        .skip(1)
        .filter_map(|line| {
            let fields: Vec<&str> = line.split(',').collect();
            let n = fields.len();
            if n < 10 {
                return None;
            }
            Some(ZoneStat {
                name: fields[..n - 9].join(","),
                total_ns: fields[n - 7].parse().ok()?,
                count: fields[n - 5].parse().ok()?,
                max_ns: fields[n - 2].parse().ok()?,
            })
        })
        .collect()
}

/// The values of the plot `name` from `tracy-csvexport --unwrap --plot`, in time order: a row a
/// value, the plot's name first and the value last.
fn parse_plot(csv: &str, name: &str) -> Vec<f64> {
    csv.lines()
        .filter(|line| line.split(',').next() == Some(name))
        .filter_map(|line| line.rsplit(',').next()?.trim().parse().ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{FrameTimes, Report, ZoneStat, parse_plot, parse_zones, sorted};

    /// A span waiting on another thread's keeps only the hand-off as its own time, and ranks by
    /// it: the work it waited on is counted where it ran.
    #[test]
    fn a_wait_on_another_thread_is_not_its_own_time() {
        let zone = |name: &str, total_ns| ZoneStat {
            name: name.to_owned(),
            total_ns,
            count: 1,
            max_ns: total_ns,
        };
        let mut report = Report {
            by_self: sorted(vec![
                zone("hand-off", 900),
                zone("draws", 500),
                zone("other", 200),
            ]),
            by_total: vec![zone("on device", 800), zone("hand-off", 900)],
            frames: FrameTimes::default(),
            device_waits: None,
        };
        report.waits_on("hand-off", "on device");
        let names: Vec<(&str, u64)> = report
            .by_self
            .iter()
            .map(|zone| (zone.name.as_str(), zone.total_ns))
            .collect();
        assert_eq!(names, [("draws", 500), ("other", 200), ("hand-off", 100)]);
        report.waits_on("hand-off", "not recorded");
        assert_eq!(report.by_self[2].total_ns, 100, "nothing to take");
    }

    /// The device waits named are summed as a share of every zone's own time, and the report
    /// says so; a wait the run never recorded adds nothing.
    #[test]
    fn device_waits_are_their_share_of_the_run() {
        let zone = |name: &str, total_ns| ZoneStat {
            name: name.to_owned(),
            total_ns,
            count: 1,
            max_ns: total_ns,
        };
        let mut report = Report {
            by_self: sorted(vec![
                zone("draw room", 300),
                zone("setup: settle", 200),
                zone("draws", 500),
            ]),
            by_total: Vec::new(),
            frames: FrameTimes::default(),
            device_waits: None,
        };
        assert!(!report.render(5).contains("waiting on the device"));
        report.note_device_waits(&["draw room", "setup: settle", "never recorded"]);
        assert_eq!(report.device_waits, Some(50.0));
        assert!(
            report
                .render(5)
                .contains("waiting on the device: 50.0% of the run")
        );
    }

    /// The exporter's statistics, as `tracy-csvexport` 0.14.1 wrote them for a run of MDEM00001.
    const ZONES: &str = concat!(
        "name,src_file,src_line,total_ns,total_perc,counts,mean_ns,min_ns,max_ns,std_ns\n",
        "submit call,crates\\orbistoun-gpu\\src\\perf.rs,469,3912500000,0.39,3582,1092300,10,625904000,5.5\n",
        "whole: window,crates\\orbistoun-gpu\\src\\perf.rs,469,2000,0.0,4,500,10,900,1.0\n",
        "torn line\n",
    );

    #[test]
    fn zones_read_as_the_exporter_writes_them() {
        let zones = parse_zones(ZONES);
        assert_eq!(
            zones,
            [
                ZoneStat {
                    name: "submit call".to_owned(),
                    total_ns: 3_912_500_000,
                    count: 3582,
                    max_ns: 625_904_000,
                },
                ZoneStat {
                    name: "whole: window".to_owned(),
                    total_ns: 2000,
                    count: 4,
                    max_ns: 900,
                },
            ]
        );
        assert_eq!(zones[1].mean_ns(), 500);
        assert_eq!(
            sorted(zones.into_iter().rev().collect())[0].name,
            "submit call"
        );
    }

    /// A plot's own rows are read, in order, and another's are not.
    #[test]
    fn a_plot_reads_its_own_rows() {
        let csv = "name,time,value\nframe ms,10,16.6\nother,11,3\nframe ms,20,33.4\n";
        assert_eq!(parse_plot(csv, "frame ms"), [16.6, 33.4]);
    }

    /// Nearest-rank percentiles, the frame rate their sum makes, and nothing from no frames.
    #[test]
    fn frame_times_rank_and_rate() {
        let frames = FrameTimes {
            ms: vec![10.0, 20.0, 10.0, 10.0, 50.0],
        };
        assert_eq!(frames.percentile(50.0), Some(10.0));
        assert_eq!(frames.percentile(95.0), Some(50.0));
        assert_eq!(frames.percentile(100.0), Some(50.0));
        assert!((frames.fps().expect("frames") - 50.0).abs() < 1e-9);
        assert_eq!(FrameTimes::default().percentile(50.0), None);
        assert_eq!(FrameTimes::default().fps(), None);
        let rendered = Report {
            frames,
            ..Report::default()
        }
        .render(5);
        assert!(rendered.contains("median 10.00"), "{rendered}");
    }
}
