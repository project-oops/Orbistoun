//! How much processor time each guest thread's host thread has used, for a report that asks which
//! threads are burning a core while nothing moves. Each thread records its host's thread id as it
//! starts; the time is asked of the host when a report wants it.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::Duration;

use super::ThreadHandle;

/// Each guest thread's host thread id, recorded by the thread itself.
fn host_ids() -> &'static Mutex<BTreeMap<ThreadHandle, u32>> {
    static IDS: Mutex<BTreeMap<ThreadHandle, u32>> = Mutex::new(BTreeMap::new());
    &IDS
}

/// Records the calling host thread as the one guest thread `handle` runs on.
pub(super) fn note_current(handle: ThreadHandle) {
    if let Some(id) = current_id()
        && let Ok(mut ids) = host_ids().lock()
    {
        ids.insert(handle, id);
    }
}

/// The processor time, user and kernel, guest thread `handle`'s host thread has used; `None` where
/// the host does not say or the thread has gone.
#[must_use]
pub fn cpu_time(handle: ThreadHandle) -> Option<Duration> {
    let id = *host_ids().lock().ok()?.get(&handle)?;
    time_of(id)
}

#[cfg(windows)]
#[expect(
    clippy::unnecessary_wraps,
    reason = "the same signature as the other hosts', which have no id to give"
)]
fn current_id() -> Option<u32> {
    // SAFETY: takes no arguments and cannot fail.
    Some(unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() })
}

#[cfg(not(windows))]
fn current_id() -> Option<u32> {
    None
}

/// A `FILETIME`'s 100-nanosecond intervals as a duration.
#[cfg(windows)]
fn duration_of(time: windows_sys::Win32::Foundation::FILETIME) -> Duration {
    let ticks = (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime);
    Duration::from_nanos(ticks.saturating_mul(100))
}

#[cfg(windows)]
fn time_of(id: u32) -> Option<Duration> {
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME};
    use windows_sys::Win32::System::Threading::{
        GetThreadTimes, OpenThread, THREAD_QUERY_LIMITED_INFORMATION,
    };
    // SAFETY: opening a thread by id has no preconditions; a thread that has gone answers null.
    let thread = unsafe { OpenThread(THREAD_QUERY_LIMITED_INFORMATION, 0, id) };
    if thread.is_null() {
        return None;
    }
    let empty = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let (mut created, mut exited, mut kernel, mut user) = (empty, empty, empty, empty);
    // SAFETY: the handle was opened above with query rights and each out-pointer is a live local.
    let asked = unsafe {
        GetThreadTimes(
            thread,
            &raw mut created,
            &raw mut exited,
            &raw mut kernel,
            &raw mut user,
        )
    };
    // SAFETY: the handle was opened above and is closed exactly once.
    unsafe { CloseHandle(thread) };
    (asked != 0).then(|| duration_of(kernel) + duration_of(user))
}

#[cfg(not(windows))]
fn time_of(_id: u32) -> Option<Duration> {
    None
}

#[cfg(test)]
mod tests {
    /// A thread that recorded itself and then worked has used processor time the host can say, on
    /// a host that says; a thread never recorded has none.
    #[test]
    fn a_recorded_thread_s_time_is_read() {
        let handle: super::ThreadHandle = 0x7fff_fff0;
        assert_eq!(super::cpu_time(handle), None, "not recorded");
        let used = std::thread::spawn(move || {
            super::note_current(handle);
            let started = std::time::Instant::now();
            let mut spun = 0u64;
            while started.elapsed() < std::time::Duration::from_millis(50) {
                spun = std::hint::black_box(spun.wrapping_add(1));
            }
            super::cpu_time(handle)
        })
        .join()
        .expect("joined");
        if cfg!(windows) {
            assert!(
                used.is_some_and(|used| used >= std::time::Duration::from_millis(10)),
                "{used:?}"
            );
        } else {
            assert_eq!(used, None);
        }
    }
}
