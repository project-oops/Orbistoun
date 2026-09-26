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
use std::sync::{Mutex, OnceLock};

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
                        job();
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
        let _ = answer.send(catch_unwind(AssertUnwindSafe(work)).ok());
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
    answered.recv().ok().flatten()
}

#[cfg(test)]
mod tests {
    use super::on_device;

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
