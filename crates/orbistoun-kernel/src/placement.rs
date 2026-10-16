//! Placing a host thread on the cores a guest thread's affinity asked for.
//!
//! The mask applied is the effective one [`crate::thread::Affinity::mapped`] computed under the
//! configured policy: zero under `observe`, which leaves the thread where the host scheduler puts
//! it. One OS call per platform; a host with no affinity call answers that it cannot.

/// Why a thread could not be placed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PlacementError {
    /// The host refused the mask, with the OS's own error.
    #[cfg_attr(
        not(any(windows, target_os = "linux")),
        allow(
            dead_code,
            reason = "constructed only on a host with an affinity call; a test names it everywhere"
        )
    )]
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

/// The lowest `cores` set bits of `allowed`: the first that many cores a process may use.
#[cfg(any(windows, target_os = "linux", test))]
fn first_cores(allowed: u64, cores: u32) -> u64 {
    let mut taken = 0u64;
    let mut left = cores;
    for bit in 0..64 {
        if left == 0 {
            break;
        }
        if allowed & (1 << bit) != 0 {
            taken |= 1 << bit;
            left -= 1;
        }
    }
    taken
}

/// The cores the process may use now, a bit each.
#[cfg(windows)]
fn allowed_cores() -> Result<u64, PlacementError> {
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessAffinityMask};
    // SAFETY: takes no arguments; the pseudo-handle needs no closing.
    let process = unsafe { GetCurrentProcess() };
    let (mut allowed, mut system) = (0usize, 0usize);
    // SAFETY: the pseudo-handle is valid for the call, and both masks are live locals it writes.
    if unsafe { GetProcessAffinityMask(process, &raw mut allowed, &raw mut system) } == 0 {
        return Err(PlacementError::Refused(
            std::io::Error::last_os_error().to_string(),
        ));
    }
    Ok(allowed as u64)
}

/// The cores the calling thread may use now, a bit each: a Linux process has no mask of its own.
#[cfg(target_os = "linux")]
fn allowed_cores() -> Result<u64, PlacementError> {
    let current = rustix::thread::sched_getaffinity(None)
        .map_err(|e| PlacementError::Refused(e.to_string()))?;
    Ok((0..64)
        .filter(|&core| current.is_set(core))
        .fold(0u64, |mask, core| mask | (1 << core)))
}

/// No call says which cores a process may use on this host.
#[cfg(all(test, not(any(windows, target_os = "linux"))))]
fn allowed_cores() -> Result<u64, PlacementError> {
    Err(PlacementError::Unsupported)
}

/// Holds the whole process to the first `cores` of the cores it may use now (D835).
#[cfg(windows)]
pub(crate) fn limit_process_cores(cores: u32) -> Result<(), PlacementError> {
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, SetProcessAffinityMask};
    // SAFETY: takes no arguments; the pseudo-handle needs no closing.
    let process = unsafe { GetCurrentProcess() };
    let mask = first_cores(allowed_cores()?, cores);
    let mask = usize::try_from(mask)
        .map_err(|_| PlacementError::Refused("the mask is wider than a pointer".to_owned()))?;
    // SAFETY: the pseudo-handle is valid for the call, and the mask a plain integer naming a subset
    // of the cores the process may use.
    if unsafe { SetProcessAffinityMask(process, mask) } == 0 {
        return Err(PlacementError::Refused(
            std::io::Error::last_os_error().to_string(),
        ));
    }
    Ok(())
}

/// Holds the calling thread, and every thread it starts from now, to the first `cores` of the
/// cores it may use: a Linux process has no mask of its own, and threads inherit their creator's.
#[cfg(target_os = "linux")]
pub(crate) fn limit_process_cores(cores: u32) -> Result<(), PlacementError> {
    pin(first_cores(allowed_cores()?, cores))
}

/// No call holds a process to some cores on this host.
#[cfg(not(any(windows, target_os = "linux")))]
pub(crate) fn limit_process_cores(_cores: u32) -> Result<(), PlacementError> {
    Err(PlacementError::Unsupported)
}

/// Sets the process's priority among the host's programs (D835).
#[cfg(windows)]
pub(crate) fn set_process_priority(
    priority: crate::thread::HostPriority,
) -> Result<(), PlacementError> {
    use crate::thread::HostPriority;
    use windows_sys::Win32::System::Threading::{
        ABOVE_NORMAL_PRIORITY_CLASS, BELOW_NORMAL_PRIORITY_CLASS, GetCurrentProcess,
        IDLE_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS, SetPriorityClass,
    };
    let class = match priority {
        HostPriority::Idle => IDLE_PRIORITY_CLASS,
        HostPriority::BelowNormal => BELOW_NORMAL_PRIORITY_CLASS,
        HostPriority::Normal => NORMAL_PRIORITY_CLASS,
        HostPriority::AboveNormal => ABOVE_NORMAL_PRIORITY_CLASS,
    };
    // SAFETY: takes no arguments; the pseudo-handle needs no closing.
    let process = unsafe { GetCurrentProcess() };
    // SAFETY: the pseudo-handle is valid for the call and the class one the call defines.
    if unsafe { SetPriorityClass(process, class) } == 0 {
        return Err(PlacementError::Refused(
            std::io::Error::last_os_error().to_string(),
        ));
    }
    Ok(())
}

/// No priority class is set away from Windows yet.
#[cfg(not(windows))]
pub(crate) fn set_process_priority(
    _priority: crate::thread::HostPriority,
) -> Result<(), PlacementError> {
    Err(PlacementError::Unsupported)
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

#[cfg(test)]
mod limit_tests {
    use super::first_cores;

    /// The first cores a process may use, skipping the ones it may not.
    #[test]
    fn the_first_cores_are_the_lowest_allowed() {
        assert_eq!(first_cores(0xff, 4), 0x0f);
        assert_eq!(first_cores(0b1010_1010, 2), 0b0000_1010);
        assert_eq!(first_cores(0b11, 8), 0b11, "no more than there are");
        assert_eq!(first_cores(0xff, 0), 0);
    }
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

    /// Pinning a thread to the first core the process may use takes effect where the host can
    /// place threads, on a thread of its own so the test runner's thread is left alone. Not core 0
    /// as such: a process held to other cores - a build limited to some of a machine's - may not
    /// use it.
    #[test]
    fn a_thread_can_be_pinned_to_its_first_allowed_core() {
        let first = super::allowed_cores().map_or(1, |allowed| super::first_cores(allowed, 1));
        let pinned = std::thread::spawn(move || super::pin_current_thread(first))
            .join()
            .expect("the thread ran");
        if cfg!(any(windows, target_os = "linux")) {
            assert_eq!(pinned, Ok(()));
        } else {
            assert_eq!(pinned, Err(super::PlacementError::Unsupported));
        }
    }
}
