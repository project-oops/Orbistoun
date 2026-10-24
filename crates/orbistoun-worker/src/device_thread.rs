//! The one host thread the graphics device is driven from.
//!
//! Backend calls run on a host thread because the guest thread that reaches them runs on a stack
//! the host's exception dispatch cannot walk, and a graphics driver raises and handles exceptions
//! as ordinary business. The thread is made on first use and kept for the process, since a thread
//! spawn per call costs several times the work it carries. A call hands it the work and waits for
//! the answer; a call made from the device thread itself runs in place.

use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{Sender, channel, sync_channel};
use std::sync::{Condvar, Mutex, OnceLock};

/// Work for the device thread, with its borrows erased - see [`on_device`].
type Job = Box<dyn FnOnce() + Send + 'static>;

thread_local! {
    /// Whether this thread is the device thread.
    static ON_DEVICE_THREAD: Cell<bool> = const { Cell::new(false) };
}

/// The channel to the device thread, which is started the first time it is asked for.
fn device() -> Option<&'static Mutex<Sender<Job>>> {
    static DEVICE: OnceLock<Option<Mutex<Sender<Job>>>> = OnceLock::new();
    DEVICE
        .get_or_init(|| {
            let (sender, jobs) = channel::<Job>();
            std::thread::Builder::new()
                .name("orbistoun-device".to_owned())
                .spawn(move || {
                    ON_DEVICE_THREAD.with(|on| on.set(true));
                    crate::profile::watch_this_thread("device");
                    for job in jobs {
                        RUNNING_SINCE
                            .store(since_start() + 1, std::sync::atomic::Ordering::Relaxed);
                        job();
                        RUNNING_SINCE.store(0, std::sync::atomic::Ordering::Relaxed);
                    }
                })
                .ok()
                .map(|_| Mutex::new(sender))
        })
        .as_ref()
}

/// Runs `work` on the device thread and answers what it returned - `None` when it panicked or the
/// thread could not be reached.
pub fn on_device<'a, R: Send + 'a>(work: impl FnOnce() -> R + Send + 'a) -> Option<R> {
    if ON_DEVICE_THREAD.with(Cell::get) {
        return catch_unwind(AssertUnwindSafe(work)).ok();
    }
    let (answer, answered) = sync_channel::<Option<R>>(1);
    let job: Box<dyn FnOnce() + Send + 'a> = Box::new(move || {
        let ran = orbistoun_gpu::perf::span(orbistoun_gpu::perf::Span::DeviceJob, || {
            catch_unwind(AssertUnwindSafe(work)).ok()
        });
        let _ = answer.send(ran);
    });
    // SAFETY: only the lifetime changes, which does not affect a boxed trait object's layout. Every
    // borrow `job` holds outlives its use: this function returns only after the job has run and
    // answered or been dropped unrun, and a panic inside it is caught.
    let job: Job = unsafe { std::mem::transmute::<Box<dyn FnOnce() + Send + 'a>, Job>(job) };
    device()?
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .send(job)
        .ok()?;
    AWAITING.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let answer = orbistoun_gpu::perf::span(orbistoun_gpu::perf::Span::DeviceAnswerWait, || {
        answered.recv().ok().flatten()
    });
    AWAITING.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    answer
}

/// When the job the device thread runs now began, in nanoseconds since [`since_start`]'s start,
/// plus one; zero while it runs none.
static RUNNING_SINCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Callers waiting for the device thread to answer them.
static AWAITING: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Nanoseconds since the first time this was asked.
fn since_start() -> u64 {
    static START: OnceLock<std::time::Instant> = OnceLock::new();
    u64::try_from(
        START
            .get_or_init(std::time::Instant::now)
            .elapsed()
            .as_nanos(),
    )
    .unwrap_or(u64::MAX - 1)
}

/// What the device thread is doing: how long the job it runs now has run, if it runs one, how many
/// callers wait for its answer, and the jobs and bytes handed to it without waiting and not yet
/// run (`None` where that count's lock is held now). Takes no lock that a job holds.
pub(crate) fn state() -> (Option<std::time::Duration>, u64, Option<(usize, usize)>) {
    let since = RUNNING_SINCE.load(std::sync::atomic::Ordering::Relaxed);
    let running = (since != 0)
        .then(|| std::time::Duration::from_nanos(since_start().saturating_sub(since - 1)));
    let handed = handed_over().held.try_lock().ok().map(|held| *held);
    (
        running,
        AWAITING.load(std::sync::atomic::Ordering::Relaxed),
        handed,
    )
}

/// Hands `work`, which owns about `bytes` of memory, to the device thread to run after everything
/// handed to it before, and returns without waiting: what the caller does next asks nothing of it
/// until a later [`on_device`] call, which the thread answers only once `work` has run (D948). Run
/// in place on the device thread. `false` when the thread could not be reached, and `work` did not
/// run.
///
/// A caller that outpaces the device waits here for room (`HANDED_OVER`), so what is handed over
/// and not yet run - each job a copy of its target's contents - stays bounded; the order is the
/// thread's queue's whatever the wait.
pub fn on_device_later(bytes: usize, work: impl FnOnce() + Send + 'static) -> bool {
    if ON_DEVICE_THREAD.with(Cell::get) {
        let _ = catch_unwind(AssertUnwindSafe(work));
        return true;
    }
    let room = handed_over().admit(bytes);
    let job: Job = Box::new(move || {
        let _room = room;
        orbistoun_gpu::perf::span(orbistoun_gpu::perf::Span::DeviceJob, || {
            let _ = catch_unwind(AssertUnwindSafe(work));
        });
    });
    device().is_some_and(|device| {
        device
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .send(job)
            .is_ok()
    })
}

/// The most jobs and bytes handed over without waiting and not yet run: some frames' worth of draw
/// segments, and a few copies of a 4K target's contents (33 MB each).
const HANDED_OVER: (usize, usize) = (16, 128 << 20);

/// The longest a hand-off waits for room before it is admitted over the limits.
const ADMIT_WAIT: std::time::Duration = std::time::Duration::from_millis(50);

/// Hand-offs admitted over the limits after waiting [`ADMIT_WAIT`] for room.
static OVERRUNS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// How many hand-offs were admitted over the limits because no room came in time.
pub(crate) fn overruns() -> u64 {
    OVERRUNS.load(std::sync::atomic::Ordering::Relaxed)
}

/// The bytes [`on_device_later`] has handed over and not yet run. Takes no lock.
pub(crate) fn handed_over_bytes() -> u64 {
    handed_over()
        .bytes
        .load(std::sync::atomic::Ordering::Relaxed)
}

/// What [`on_device_later`] has handed over and not yet run.
fn handed_over() -> &'static Gate {
    static GATE: Gate = Gate::within(HANDED_OVER);
    &GATE
}

/// Room for work in flight, counted in jobs and bytes: a job is admitted while both stay within
/// the limits, or alone whatever it holds, and the room it took is given back when it is dropped.
struct Gate {
    held: Mutex<(usize, usize)>,
    /// The bytes `held` counts, published as it changes for a census that takes no lock.
    bytes: std::sync::atomic::AtomicU64,
    freed: Condvar,
    limits: (usize, usize),
}

impl Gate {
    const fn within(limits: (usize, usize)) -> Self {
        Self {
            held: Mutex::new((0, 0)),
            bytes: std::sync::atomic::AtomicU64::new(0),
            freed: Condvar::new(),
            limits,
        }
    }

    /// Waits until a job of `bytes` fits, at most [`ADMIT_WAIT`], and takes its room; past that it
    /// is admitted anyway and counted ([`overruns`]). The caller may hold a lock a job already
    /// handed over needs before it can run and give its room back, so a wait without end deadlocks
    /// the command processor - GTA V's intro froze three runs in a row that way, a 4K video frame
    /// handed over each flip.
    fn admit(&'static self, bytes: usize) -> Room {
        let (most_jobs, most_bytes) = self.limits;
        let fits = |&(jobs, held_bytes): &(usize, usize)| {
            jobs == 0 || (jobs < most_jobs && held_bytes.saturating_add(bytes) <= most_bytes)
        };
        let mut held = self
            .held
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // A wait here is counted in the caller's hand-off span.
        if !fits(&held) {
            let (after, waited) = self
                .freed
                .wait_timeout_while(held, ADMIT_WAIT, |held| !fits(held))
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            held = after;
            if waited.timed_out() {
                OVERRUNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }
        held.0 += 1;
        held.1 = held.1.saturating_add(bytes);
        self.bytes
            .store(held.1 as u64, std::sync::atomic::Ordering::Relaxed);
        Room { gate: self, bytes }
    }

    /// What is in flight: jobs and bytes.
    #[cfg(test)]
    fn held(&self) -> (usize, usize) {
        *self
            .held
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// A job's room in a [`Gate`], given back when dropped: once the job ran, panicked, or was dropped
/// unrun.
struct Room {
    gate: &'static Gate,
    bytes: usize,
}

impl Drop for Room {
    fn drop(&mut self) {
        let mut held = self
            .gate
            .held
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        held.0 = held.0.saturating_sub(1);
        held.1 = held.1.saturating_sub(self.bytes);
        self.gate
            .bytes
            .store(held.1 as u64, std::sync::atomic::Ordering::Relaxed);
        self.gate.freed.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::{Gate, on_device, on_device_later};

    /// Work handed over without waiting runs on the device thread before anything handed after it,
    /// so the next call that waits sees it done; a panic in it leaves the thread running.
    #[test]
    fn work_handed_over_runs_before_what_follows_it() {
        use std::sync::{Arc, Mutex};
        let order = Arc::new(Mutex::new(Vec::new()));
        for step in 0..3 {
            let order = Arc::clone(&order);
            assert!(on_device_later(1 << 20, move || {
                std::thread::sleep(std::time::Duration::from_millis(5));
                order.lock().expect("order").push(step);
            }));
        }
        assert!(on_device_later(0, || panic!("inside")));
        let seen = on_device(|| order.lock().expect("order").clone()).expect("answered");
        assert_eq!(
            seen,
            vec![0, 1, 2],
            "all of it, in order, before the answer"
        );
    }

    /// Work in flight stays within the gate's jobs and bytes: a job that does not fit waits until
    /// one ahead of it is done, one larger than the bytes alone is admitted when nothing is in
    /// flight, and the room a job took comes back when it is dropped.
    #[test]
    fn work_in_flight_stays_within_its_jobs_and_bytes() {
        use std::sync::atomic::{AtomicBool, Ordering};
        static GATE: Gate = Gate::within((2, 100));
        static ADMITTED: AtomicBool = AtomicBool::new(false);
        let first = GATE.admit(60);
        let second = GATE.admit(30);
        assert_eq!(GATE.held(), (2, 90));
        let waiting = std::thread::spawn(|| {
            let third = GATE.admit(20);
            ADMITTED.store(true, Ordering::SeqCst);
            drop(third);
        });
        std::thread::sleep(std::time::Duration::from_millis(10));
        assert!(!ADMITTED.load(Ordering::SeqCst), "two jobs in flight");
        drop(first);
        waiting.join().expect("admitted once one was done");
        assert!(ADMITTED.load(Ordering::SeqCst));
        assert_eq!(
            GATE.held(),
            (1, 30),
            "the room each dropped job took is back"
        );
        // Nothing gives room back: the wait ends anyway, so a caller holding a lock a queued job
        // needs cannot deadlock, and the overrun is counted.
        let before = super::overruns();
        let held_on = GATE.admit(70);
        let over = GATE.admit(70);
        assert_eq!(
            GATE.held(),
            (3, 170),
            "admitted over the bytes after the wait"
        );
        assert_eq!(super::overruns(), before + 1);
        drop((held_on, over));
        drop(second);
        let alone = GATE.admit(1000);
        assert_eq!(
            GATE.held(),
            (1, 1000),
            "larger than the bytes, admitted alone"
        );
        drop(alone);
        assert_eq!(GATE.held(), (0, 0));
    }

    /// Work runs on one reused device thread with its borrows, a panic is an absent answer, and a
    /// nested call runs in place.
    #[test]
    fn work_runs_on_one_device_thread_and_answers() {
        let borrowed = String::from("123");
        let (sum, first) = on_device(|| {
            (
                borrowed.bytes().map(|b| i32::from(b - b'0')).sum::<i32>(),
                std::thread::current().id(),
            )
        })
        .expect("answered");
        assert_eq!(sum, 6);
        let second = on_device(|| std::thread::current().id()).expect("answered");
        assert_eq!(first, second, "one device thread, reused");
        assert_ne!(first, std::thread::current().id());
        let nested = on_device(|| on_device(|| 7)).flatten();
        assert_eq!(nested, Some(7), "a nested call runs in place");
        assert_eq!(on_device(|| -> i32 { panic!("inside") }), None);
        assert_eq!(on_device(|| 8), Some(8), "the thread survives a panic");
    }
}
