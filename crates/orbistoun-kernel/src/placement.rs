//! Placing a host thread on the cores a guest thread's affinity asked for.
//!
//! The mask applied is the effective one [`crate::thread::Affinity::mapped`] computed under the
//! configured policy: zero under `observe`, which leaves the thread where the host scheduler puts
//! it. One OS call per platform; a host with no affinity call answers that it cannot.

/// Why a thread could not be placed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PlacementError {
    /// The host refused the mask, with the OS's own error.
    Refused(String),
    /// This host has no call that sets a thread's affinity.
    #[cfg_attr(
        any(windows, target_os = "linux"),
        allow(
            dead_code,
            reason = "constructed only on a host with no affinity call; a test names it everywhere"
        )
    )]
    Unsupported,
}

impl core::fmt::Display for PlacementError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Refused(why) => write!(f, "the host refused the mask: {why}"),
            Self::Unsupported => f.write_str("this host has no call that sets a thread's affinity"),
        }
    }
}

/// Restricts the calling host thread to the cores `mask` names, bit `n` for core `n`.
///
/// A zero mask is no restriction and changes nothing.
pub(crate) fn pin_current_thread(mask: u64) -> Result<(), PlacementError> {
    if mask == 0 {
        return Ok(());
    }
    pin(mask)
}

#[cfg(windows)]
fn pin(mask: u64) -> Result<(), PlacementError> {
    use windows_sys::Win32::System::Threading::{GetCurrentThread, SetThreadAffinityMask};

    let mask = usize::try_from(mask)
        .map_err(|_| PlacementError::Refused("the mask is wider than a pointer".to_owned()))?;
    // SAFETY: `GetCurrentThread` takes no arguments and returns a pseudo-handle for the calling
    // thread that needs no closing.
    let thread = unsafe { GetCurrentThread() };
    // SAFETY: `thread` is the calling thread's pseudo-handle, valid for this call, and the mask is
    // a plain integer; the call reads nothing through a pointer.
    let previous = unsafe { SetThreadAffinityMask(thread, mask) };
    if previous == 0 {
        return Err(PlacementError::Refused(
            std::io::Error::last_os_error().to_string(),
        ));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn pin(mask: u64) -> Result<(), PlacementError> {
    let mut cores = rustix::thread::CpuSet::new();
    for core in (0..64).filter(|b| mask & (1 << b) != 0) {
        cores.set(core);
    }
    rustix::thread::sched_setaffinity(None, &cores)
        .map_err(|e| PlacementError::Refused(e.to_string()))
}

#[cfg(not(any(windows, target_os = "linux")))]
fn pin(_mask: u64) -> Result<(), PlacementError> {
    Err(PlacementError::Unsupported)
}

#[cfg(test)]
mod tests {
    /// A zero mask is no restriction and always succeeds, whatever the host.
    #[test]
    fn a_zero_mask_changes_nothing() {
        assert_eq!(super::pin_current_thread(0), Ok(()));
    }

    /// Pinning a thread to core 0, which every host has, takes effect where the host can place
    /// threads, on a thread of its own so the test runner's thread is left alone.
    #[test]
    fn a_thread_can_be_pinned_to_core_zero() {
        let pinned = std::thread::spawn(|| super::pin_current_thread(1))
            .join()
            .expect("the thread ran");
        if cfg!(any(windows, target_os = "linux")) {
            assert_eq!(pinned, Ok(()));
        } else {
            assert_eq!(pinned, Err(super::PlacementError::Unsupported));
        }
    }
}
