//! The stall watchdog: a run that goes `ORBISTOUN_STALL_REPORT` seconds with no flip and no
//! command-processor packet completed logs one report of where everything stands, and with `:end`
//! then ends as a limit ends it.
//!
//! The report names each queue and compute stream with the packet it stands at and, for a memory
//! wait, what it waits for and what the word holds ([`orbistoun_gpu::agc_driver::queue_states`]);
//! what the device thread is doing; and each guest thread with how much of a core it used over a
//! short window, the import it is inside with its first argument, and whether it spins in a
//! sleep or yield poll loop. It runs on a thread of its own and takes no lock a stalled thread
//! holds, so the report is written whatever holds what.

use std::fmt::Write as _;
use std::time::{Duration, Instant};

/// How long a run goes without progress before it is reported when the variable is unset.
const DEFAULT_AFTER: Duration = Duration::from_secs(300);

/// How often the watchdog looks.
const TICK: Duration = Duration::from_secs(1);

/// The window each thread's processor time is measured over in a report.
const WINDOW: Duration = Duration::from_secs(2);

/// The share of a core past which a thread whose recent calls are all sleeps or yields is spinning.
const SPINNING_SHARE: f64 = 0.5;

/// What `ORBISTOUN_STALL_REPORT` asks for: how long without progress, and whether to end then.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Asked {
    pub(crate) after: Duration,
    pub(crate) end: bool,
}

/// What a value of `ORBISTOUN_STALL_REPORT` asks for: unset, the default; `off`, nothing;
/// `<seconds>` or `<seconds>:end`.
///
/// # Errors
///
/// The value, where it is none of those.
pub(crate) fn parse(value: Option<&str>) -> Result<Option<Asked>, String> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(Some(Asked {
            after: DEFAULT_AFTER,
            end: false,
        }));
    };
    if value == "off" {
        return Ok(None);
    }
    let (seconds, end) = value
        .strip_suffix(":end")
        .map_or((value, false), |seconds| (seconds, true));
    seconds
        .parse::<u64>()
        .ok()
        .filter(|&seconds| seconds > 0)
        .map(|seconds| {
            Some(Asked {
                after: Duration::from_secs(seconds),
                end,
            })
        })
        .ok_or_else(|| value.to_owned())
}

/// Whether the run has stalled, from what it has done by each look: flips and packets completed.
#[derive(Debug)]
pub(crate) struct Watchdog {
    after: Duration,
    progress: Option<(u64, u64)>,
    since: Duration,
    reported: bool,
}

impl Watchdog {
    pub(crate) const fn new(after: Duration) -> Self {
        Self {
            after,
            progress: None,
            since: Duration::ZERO,
            reported: false,
        }
    }

    /// Looks at `progress` at `now`, both since the run began: answers how long the run has gone
    /// without progress the first time that reaches the bound, and nothing otherwise, until
    /// progress resumes and a later stall reaches it again.
    pub(crate) fn look(&mut self, now: Duration, progress: (u64, u64)) -> Option<Duration> {
        if self.progress != Some(progress) {
            self.progress = Some(progress);
            self.since = now;
            self.reported = false;
            return None;
        }
        let stalled = now.saturating_sub(self.since);
        if self.reported || stalled < self.after {
            return None;
        }
        self.reported = true;
        Some(stalled)
    }
}

/// A guest thread as the report names it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ThreadLine {
    pub(crate) name: String,
    pub(crate) handle: u64,
    /// The share of a core it used over the window, where the host says.
    pub(crate) busy: Option<f64>,
    /// Its processor time over its life, where the host says.
    pub(crate) total: Option<Duration>,
    /// The import it is inside now, with its first argument and where it was called from.
    pub(crate) inside: Option<String>,
    /// The labels of its latest calls, newest first.
    pub(crate) recent: Vec<String>,
    /// The callee-saved registers of the loop it waits in, where it waits in one around one call.
    pub(crate) saved: Option<Vec<(&'static str, u64)>>,
}

/// Whether a call is a sleep or a yield.
fn sleeps(label: &str) -> bool {
    let label = label.to_ascii_lowercase();
    label.contains("sleep") || label.contains("yield")
}

/// Whether a thread spins in a sleep or yield poll loop: busy past [`SPINNING_SHARE`] of a core,
/// with every one of its latest calls a sleep or a yield.
pub(crate) fn spinning(recent: &[String], busy: Option<f64>) -> bool {
    busy.is_some_and(|busy| busy >= SPINNING_SHARE)
        && !recent.is_empty()
        && recent.iter().all(|label| sleeps(label))
}

/// A thread as one line of the report.
pub(crate) fn thread_line(thread: &ThreadLine) -> String {
    let mut line = format!("{} (thread {:#x})", thread.name, thread.handle);
    match thread.busy {
        Some(busy) => {
            let _ = write!(line, ": {:.0}% of a core", busy * 100.0);
        }
        None => line.push_str(": busy unknown"),
    }
    if let Some(total) = thread.total {
        let _ = write!(line, ", {:.1} s in all", total.as_secs_f64());
    }
    match &thread.inside {
        Some(inside) => {
            let _ = write!(line, "; inside {inside}");
        }
        None => line.push_str("; in its own code"),
    }
    if spinning(&thread.recent, thread.busy) {
        line.push_str("; spinning in a sleep or yield poll loop");
    }
    if let Some(latest) = thread.recent.first() {
        let _ = write!(line, "; latest call {latest}");
    }
    if let Some(saved) = &thread.saved {
        let registers: Vec<String> = saved
            .iter()
            .map(|(name, value)| format!("{name}={value:#x}"))
            .collect();
        let _ = write!(line, "; its loop holds {}", registers.join(" "));
    }
    line
}

/// Each guest thread still running, its processor time over a [`WINDOW`] measured here, the
/// busiest first.
fn threads() -> Vec<ThreadLine> {
    let records: Vec<orbistoun_kernel::thread::ThreadRecord> = orbistoun_kernel::thread::all()
        .into_iter()
        .filter(|record| !record.finished)
        .collect();
    let before: Vec<Option<Duration>> = records
        .iter()
        .map(|record| orbistoun_kernel::thread::cpu_time(record.handle))
        .collect();
    std::thread::sleep(WINDOW);
    let inside = orbistoun_thunk::threads_inside();
    let waiting = crate::waits::waiting(&orbistoun_thunk::recorded_calls());
    let mut lines: Vec<ThreadLine> = records
        .iter()
        .zip(before)
        .map(|(record, before)| {
            let total = orbistoun_kernel::thread::cpu_time(record.handle);
            let busy = before.zip(total).map(|(before, after)| {
                after.saturating_sub(before).as_secs_f64() / WINDOW.as_secs_f64()
            });
            let host = record.host;
            let inside = inside
                .iter()
                .find(|&&(at, ..)| host != 0 && at == host)
                .map(|&(_, index, first, rsp)| {
                    let call = crate::report::labelled(index as usize);
                    let mut said = format!("{call}({first:#x})");
                    if let Some(object) = crate::report::waited_on(&call, first) {
                        let _ = write!(said, " on {object}");
                    }
                    if let Some(site) = crate::report::called_from(rsp) {
                        let _ = write!(said, ", called from {site:#x}");
                    }
                    said
                });
            let recent = if host == 0 {
                Vec::new()
            } else {
                orbistoun_thunk::recent_calls_on(host)
                    .into_iter()
                    .map(|(index, ..)| crate::report::labelled(index as usize))
                    .collect()
            };
            let saved = waiting
                .iter()
                .find(|wait| host != 0 && wait.host == host)
                .map(|wait| {
                    orbistoun_report::trace::CALLEE_SAVED
                        .iter()
                        .copied()
                        .zip(wait.saved)
                        .collect()
                });
            ThreadLine {
                name: record.name.clone(),
                handle: record.handle,
                busy,
                total,
                inside,
                recent,
                saved,
            }
        })
        .collect();
    lines.sort_by(|a, b| b.busy.unwrap_or(0.0).total_cmp(&a.busy.unwrap_or(0.0)));
    lines
}

/// What the device thread is doing, as a line.
fn device_line() -> String {
    let (running, awaiting, handed) = crate::device_thread::state();
    let mut line = running.map_or_else(
        || "device thread: idle".to_owned(),
        |running| {
            format!(
                "device thread: running one job for {:.1} s",
                running.as_secs_f64()
            )
        },
    );
    let _ = write!(line, "; {awaiting} callers waiting for its answer");
    match handed {
        Some((jobs, bytes)) => {
            let _ = write!(
                line,
                "; {jobs} jobs ({} MiB) handed over and not yet run",
                bytes >> 20
            );
        }
        None => line.push_str("; what it was handed is not known: its lock is held"),
    }
    line
}

/// The whole report, for a run that has gone `stalled` without progress: `progress` is its flips
/// and completed packets.
fn report(stalled: Duration, (flips, packets): (u64, u64)) -> String {
    let mut text = format!(
        "the run has stalled: no flip and no command-processor packet completed for {:.0} s ({flips} flips, {packets} packets so far)",
        stalled.as_secs_f64()
    );
    text.push_str("\nqueues:");
    for state in orbistoun_gpu::agc_driver::queue_states() {
        let _ = write!(
            text,
            "\n  {}",
            orbistoun_gpu::agc_driver::describe_queue(&state)
        );
    }
    let _ = write!(text, "\nhand-offs:\n  {}", device_line());
    let _ = write!(
        text,
        "\nguest threads, busiest first, over the last {} s:",
        WINDOW.as_secs()
    );
    for thread in threads() {
        let _ = write!(text, "\n  {}", thread_line(&thread));
    }
    text
}

/// What the run has done: flips, and packets the command processor completed.
fn progress() -> (u64, u64) {
    (crate::render::flips(), orbistoun_gpu::cp::packets_done())
}

/// Starts the watchdog, as `ORBISTOUN_STALL_REPORT` asks, on a thread of its own.
pub(crate) fn start() {
    let asked = parse(orbistoun_env::STALL_REPORT.get().as_deref()).unwrap_or_else(|value| {
        tracing::warn!(
            "{} is {value:?}, which is not <seconds>[:end] or off; stalls are reported after {} s",
            orbistoun_env::STALL_REPORT.name,
            DEFAULT_AFTER.as_secs()
        );
        Some(Asked {
            after: DEFAULT_AFTER,
            end: false,
        })
    });
    let Some(asked) = asked else {
        return;
    };
    let _ = std::thread::Builder::new()
        .name("orbistoun-stall-watchdog".to_owned())
        .spawn(move || {
            let started = Instant::now();
            let mut watchdog = Watchdog::new(asked.after);
            loop {
                std::thread::sleep(TICK);
                let now = progress();
                let Some(stalled) = watchdog.look(started.elapsed(), now) else {
                    continue;
                };
                tracing::warn!("{}", report(stalled, now));
                if asked.end {
                    crate::report::end_on_stall();
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::{Asked, ThreadLine, Watchdog, parse, spinning, thread_line};
    use std::time::Duration;

    /// Unset is the default, off is nothing, and a count of seconds may ask for the end.
    #[test]
    fn the_variable_names_seconds_and_the_end() {
        let asked = |seconds, end| {
            Ok(Some(Asked {
                after: Duration::from_secs(seconds),
                end,
            }))
        };
        assert_eq!(parse(None), asked(300, false));
        assert_eq!(parse(Some("120")), asked(120, false));
        assert_eq!(parse(Some("120:end")), asked(120, true));
        assert_eq!(parse(Some("off")), Ok(None));
        assert_eq!(parse(Some("0")), Err("0".to_owned()));
        assert_eq!(parse(Some("soon")), Err("soon".to_owned()));
    }

    /// A stall is reported once it lasts the bound, once only, and again only after progress
    /// resumed and stopped for the bound again.
    #[test]
    fn a_stall_is_reported_once_it_lasts_the_bound() {
        let at = Duration::from_secs;
        let mut watchdog = Watchdog::new(at(10));
        assert_eq!(watchdog.look(at(1), (5, 100)), None, "the first look");
        assert_eq!(watchdog.look(at(10), (5, 100)), None, "nine seconds");
        assert_eq!(watchdog.look(at(11), (5, 100)), Some(at(10)));
        assert_eq!(watchdog.look(at(30), (5, 100)), None, "said once");
        assert_eq!(watchdog.look(at(31), (5, 101)), None, "a packet completed");
        assert_eq!(watchdog.look(at(40), (5, 101)), None);
        assert_eq!(watchdog.look(at(41), (5, 101)), Some(at(10)));
        assert_eq!(watchdog.look(at(42), (6, 101)), None, "a flip");
    }

    /// A thread busy in calls that are all sleeps or yields spins; one idle, or calling anything
    /// else, does not.
    #[test]
    fn a_busy_thread_that_only_sleeps_is_spinning() {
        let sleeps = vec!["sceKernelUsleep".to_owned(), "scePthreadYield".to_owned()];
        assert!(spinning(&sleeps, Some(0.98)));
        assert!(!spinning(&sleeps, Some(0.02)), "idle");
        assert!(!spinning(&sleeps, None), "not known");
        assert!(!spinning(&[], Some(1.0)), "no calls");
        let mixed = vec![
            "sceKernelUsleep".to_owned(),
            "sceAgcDriverSubmitDcb".to_owned(),
        ];
        assert!(!spinning(&mixed, Some(1.0)));
    }

    /// A thread's line names it, how busy it is, what it is inside, whether it spins, and what its
    /// loop holds.
    #[test]
    fn a_thread_line_says_what_the_thread_is_doing() {
        let thread = ThreadLine {
            name: "[RAGE] RenderThread".to_owned(),
            handle: 0x2a,
            busy: Some(0.99),
            total: Some(Duration::from_millis(1_234_500)),
            inside: Some("sceKernelUsleep(0x3e8), called from 0x40001234".to_owned()),
            recent: vec!["sceKernelUsleep".to_owned(); 8],
            saved: Some(vec![("rbx", 0x7400_01d1_1bd8), ("rbp", 0)]),
        };
        assert_eq!(
            thread_line(&thread),
            "[RAGE] RenderThread (thread 0x2a): 99% of a core, 1234.5 s in all; inside sceKernelUsleep(0x3e8), called from 0x40001234; spinning in a sleep or yield poll loop; latest call sceKernelUsleep; its loop holds rbx=0x740001d11bd8 rbp=0x0"
        );
        let idle = ThreadLine {
            busy: None,
            total: None,
            inside: None,
            recent: Vec::new(),
            saved: None,
            ..thread
        };
        assert_eq!(
            thread_line(&idle),
            "[RAGE] RenderThread (thread 0x2a): busy unknown; in its own code"
        );
    }
}
