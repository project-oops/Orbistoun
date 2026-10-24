//! The worker's own memory as the host counts it: sampled at each perf window, and summed up at the
//! run's end as its peak and how fast its private memory grew over the last minute - what tells a
//! run that will exhaust the host from one that holds steady.

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use orbistoun_proto::ProcessMemory;

/// The worker's memory now; `None` where the host does not say.
#[must_use]
fn sample() -> Option<ProcessMemory> {
    imp::sample()
}

/// Samples the worker's memory and keeps it for the run's end: what a perf window reports. Never
/// waits: a sample taken while another thread notes one is reported and not kept.
#[must_use]
pub(crate) fn sample_and_note() -> Option<ProcessMemory> {
    let memory = sample()?;
    let at = started().elapsed().as_secs_f64();
    if let Some(mut trend) = trend_now() {
        trend.note(at, memory);
    }
    Some(memory)
}

/// The kept samples, where no other thread holds them: the census never waits.
fn trend_now() -> Option<std::sync::MutexGuard<'static, Trend>> {
    match trend().try_lock() {
        Ok(trend) => Some(trend),
        Err(std::sync::TryLockError::Poisoned(poisoned)) => Some(poisoned.into_inner()),
        Err(std::sync::TryLockError::WouldBlock) => None,
    }
}

/// The run's memory lines, from a sample taken now and those the perf windows kept: its peak and
/// growth, what it holds by holder, and the flag when the host's overhead is over its budget.
/// Empty where the host does not say.
#[must_use]
pub(crate) fn describe_at_end() -> Vec<String> {
    let Some(memory) = sample_and_note() else {
        return Vec::new();
    };
    let held = held::Breakdown::now(memory.private);
    trend_now()
        .and_then(|trend| trend.describe())
        .into_iter()
        .chain([held.describe()])
        .chain(held.over(held::OVERHEAD_BUDGET))
        .chain(overrun_line(crate::device_thread::overruns()))
        .collect()
}

/// The line naming hand-offs to the device admitted over their limits after waiting for room, when
/// there were any: a bound that would have deadlocked the command processor had it held.
fn overrun_line(overruns: u64) -> Option<String> {
    (overruns > 0).then(|| {
        format!(
            "{overruns} hand-off(s) to the device were admitted over their limits after waiting for room"
        )
    })
}

/// Logs what the worker holds by holder, at a perf window that sampled `memory`; flags the host's
/// overhead when it is over its budget, the first time and again each gibibyte further.
pub(crate) fn log_breakdown(memory: Option<ProcessMemory>) {
    use std::sync::atomic::{AtomicU64, Ordering};
    /// The overhead last flagged, in whole gibibytes.
    static FLAGGED: AtomicU64 = AtomicU64::new(0);
    let Some(memory) = memory else {
        return;
    };
    let held = held::Breakdown::now(memory.private);
    tracing::info!("{}", held.describe());
    let Some(flag) = held.over(held::OVERHEAD_BUDGET) else {
        return;
    };
    let whole = held.overhead() >> 30;
    if FLAGGED.fetch_max(whole, Ordering::Relaxed) < whole {
        tracing::warn!("{flag}");
    }
}

mod heap;
mod held;

pub(crate) use heap::sample_from_env as sample_heap_from_env;

/// When the run's first sample was asked for, which every sample's time is counted from.
fn started() -> &'static Instant {
    static STARTED: OnceLock<Instant> = OnceLock::new();
    STARTED.get_or_init(Instant::now)
}

fn trend() -> &'static Mutex<Trend> {
    static TREND: Mutex<Trend> = Mutex::new(Trend::new());
    &TREND
}

/// How far back the growth is measured.
const RATED_OVER: f64 = 60.0;

/// The shortest stretch a growth is given for: a rate over less says little.
const RATED_FROM: f64 = 5.0;

/// Samples of private memory over the run's recent past, and the latest whole sample.
#[derive(Debug)]
struct Trend {
    /// Seconds into the run and private bytes, oldest first: those within [`RATED_OVER`] of the
    /// latest, and the newest before that.
    private: VecDeque<(f64, u64)>,
    latest: Option<ProcessMemory>,
}

impl Trend {
    const fn new() -> Self {
        Self {
            private: VecDeque::new(),
            latest: None,
        }
    }

    /// Keeps `memory`, sampled `at` seconds into the run.
    fn note(&mut self, at: f64, memory: ProcessMemory) {
        self.private.push_back((at, memory.private));
        self.latest = Some(memory);
        // The oldest goes while the one after it is still old enough to measure from.
        while self
            .private
            .get(1)
            .is_some_and(|&(then, _)| at - then >= RATED_OVER)
        {
            self.private.pop_front();
        }
    }

    /// Private bytes a minute over the last [`RATED_OVER`] seconds or what of them was sampled, with
    /// the stretch measured and its first and last private bytes; `None` over less than
    /// [`RATED_FROM`] seconds.
    #[allow(clippy::cast_precision_loss)] // bytes far below 2^52 are exact enough for a rate
    fn growth(&self) -> Option<(f64, f64, u64, u64)> {
        let &(last_at, last) = self.private.back()?;
        let &(first_at, first) = self
            .private
            .iter()
            .rev()
            .find(|&&(then, _)| last_at - then >= RATED_OVER)
            .or_else(|| self.private.front())?;
        let seconds = last_at - first_at;
        if seconds < RATED_FROM {
            return None;
        }
        let per_minute = (last as f64 - first as f64) / seconds * 60.0;
        Some((per_minute, seconds, first, last))
    }

    /// The line the run's end prints.
    fn describe(&self) -> Option<String> {
        let latest = self.latest?;
        let mut line = format!(
            "worker memory: peak {} private, {} resident; now {} private, {} resident",
            gib(latest.peak_private),
            gib(latest.peak_working_set),
            gib(latest.private),
            gib(latest.working_set),
        );
        match self.growth() {
            Some((per_minute, seconds, first, last)) => {
                let _ = write!(
                    line,
                    "; private {}{} a minute over the last {seconds:.0} s ({} to {})",
                    if per_minute < 0.0 { "-" } else { "+" },
                    mib(per_minute.abs()),
                    gib(first),
                    gib(last),
                );
            }
            None => line.push_str("; too few samples to give its growth"),
        }
        Some(line)
    }
}

#[allow(clippy::cast_precision_loss)] // a byte count shown to a tenth of a GiB
fn gib(bytes: u64) -> String {
    format!("{:.1} GiB", bytes as f64 / f64::from(1u32 << 30))
}

fn mib(bytes: f64) -> String {
    format!("{:.0} MiB", bytes / f64::from(1u32 << 20))
}

#[cfg(windows)]
mod imp {
    use orbistoun_proto::ProcessMemory;
    use windows_sys::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    /// `GetProcessMemoryInfo` for this process, through its extended counters.
    pub(super) fn sample() -> Option<ProcessMemory> {
        let size = u32::try_from(size_of::<PROCESS_MEMORY_COUNTERS_EX>()).ok()?;
        let mut counters = PROCESS_MEMORY_COUNTERS_EX {
            cb: size,
            ..PROCESS_MEMORY_COUNTERS_EX::default()
        };
        // SAFETY: takes no arguments and answers a pseudo-handle that needs no closing.
        let process = unsafe { GetCurrentProcess() };
        // SAFETY: the pseudo-handle names this process; the structure is the extended one, which
        // begins with the basic one the function is declared over, and `size` is its whole size,
        // so the function writes only within it.
        let read = unsafe {
            GetProcessMemoryInfo(
                process,
                (&raw mut counters).cast::<PROCESS_MEMORY_COUNTERS>(),
                size,
            )
        };
        (read != 0).then_some(ProcessMemory {
            working_set: counters.WorkingSetSize as u64,
            peak_working_set: counters.PeakWorkingSetSize as u64,
            private: counters.PrivateUsage as u64,
            peak_private: counters.PeakPagefileUsage as u64,
        })
    }
}

#[cfg(not(windows))]
mod imp {
    /// Not sampled off Windows.
    pub(super) const fn sample() -> Option<orbistoun_proto::ProcessMemory> {
        None
    }
}

#[cfg(test)]
#[allow(clippy::cast_precision_loss)] // test byte counts far below 2^52
mod tests {
    use super::{ProcessMemory, Trend};

    fn private(bytes: u64) -> ProcessMemory {
        ProcessMemory {
            working_set: bytes / 2,
            peak_working_set: bytes,
            private: bytes,
            peak_private: bytes + (1 << 30),
        }
    }

    const MIB: u64 = 1 << 20;

    /// The growth is measured from the newest sample a minute or more before the latest, so a run
    /// growing 50 MiB a second reads 3000 MiB a minute however its samples fall, and the samples
    /// kept stay a minute's worth.
    #[test]
    fn growth_is_measured_over_the_last_minute() {
        let mut trend = Trend::new();
        for second in 0..600u32 {
            trend.note(f64::from(second), private(u64::from(second) * 50 * MIB));
        }
        let (per_minute, seconds, first, last) = trend.growth().expect("rated");
        assert!(
            (per_minute - 3000.0 * MIB as f64).abs() < 1.0,
            "{per_minute}"
        );
        assert!((seconds - 60.0).abs() < f64::EPSILON);
        assert_eq!((first, last), (539 * 50 * MIB, 599 * 50 * MIB));
        assert!(trend.private.len() <= 62, "{} kept", trend.private.len());
    }

    /// Sparse windows - one every few minutes - are measured over what was sampled, and a
    /// stretch too short to say anything gives no rate.
    #[test]
    fn sparse_or_short_samples_are_rated_by_what_there_is() {
        let mut trend = Trend::new();
        trend.note(0.0, private(1000 * MIB));
        assert_eq!(trend.growth(), None, "one sample");
        trend.note(2.0, private(1100 * MIB));
        assert_eq!(trend.growth(), None, "two seconds");
        trend.note(300.0, private(1600 * MIB));
        let (per_minute, seconds, first, _) = trend.growth().expect("rated");
        assert!(
            (seconds - 298.0).abs() < f64::EPSILON,
            "from the newest a minute before"
        );
        assert_eq!(first, 1100 * MIB);
        assert!((per_minute - 500.0 * 60.0 / 298.0 * MIB as f64).abs() < 1.0);
    }

    /// The run's line gives the peaks, where it stands, and the growth with its sign.
    #[test]
    fn the_line_gives_the_peaks_and_the_growth() {
        let mut trend = Trend::new();
        assert_eq!(trend.describe(), None);
        trend.note(0.0, private(20 << 30));
        trend.note(60.0, private(23 << 30));
        assert_eq!(
            trend.describe().as_deref(),
            Some(concat!(
                "worker memory: peak 24.0 GiB private, 23.0 GiB resident; now 23.0 GiB ",
                "private, 11.5 GiB resident; private +3072 MiB a minute over the last 60 s ",
                "(20.0 GiB to 23.0 GiB)"
            ))
        );
        trend.note(120.0, private(22 << 30));
        assert!(
            trend
                .describe()
                .is_some_and(|line| line.contains("private -1024 MiB a minute")),
            "{:?}",
            trend.describe()
        );
    }

    /// A perf window's census - the sample kept, the breakdown logged, the run's lines - answers
    /// while another thread holds the live backend and the kept samples: it takes no lock, so it
    /// cannot wait on a thread that waits on the one it runs on.
    #[test]
    fn the_census_answers_while_its_holders_are_locked() {
        use std::sync::PoisonError;
        use std::sync::mpsc::channel;
        use std::time::Duration;
        let (locked, all_locked) = channel();
        let (release, released) = channel::<()>();
        let holder = std::thread::spawn(move || {
            let backend = crate::render::live_backend()
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let trend = super::trend()
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            locked.send(()).expect("the test waits");
            let _ = released.recv_timeout(Duration::from_secs(30));
            drop((backend, trend));
        });
        all_locked.recv().expect("the holder locks");
        let (answer, answered) = channel();
        std::thread::spawn(move || {
            super::log_breakdown(Some(private(20 << 30)));
            let _ = super::sample_and_note();
            let _ = answer.send(super::describe_at_end());
        });
        let census = answered.recv_timeout(Duration::from_secs(5));
        let _ = release.send(());
        holder.join().expect("the holder lets go");
        assert!(census.is_ok(), "the census waited on a lock");
    }

    /// The host says what this process holds, and a peak is never below what is held now.
    #[cfg(windows)]
    #[test]
    fn the_host_reports_this_process() {
        let memory = super::sample().expect("Windows reports it");
        assert!(memory.working_set > 0 && memory.private > 0);
        assert!(memory.peak_working_set >= memory.working_set);
        assert!(memory.peak_private >= memory.private);
    }
}
