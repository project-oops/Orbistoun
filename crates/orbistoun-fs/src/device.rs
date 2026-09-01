//! The paths that are not files, and are not on the host either.
//!
//! # Why a device needs its own answer
//!
//! Everything else in this crate resolves a guest path to a host path and opens it. `/dev/klog`
//! has no host path: it is a stream the kernel produces, and here the kernel is this emulator.
//! So it is answered before the mount table is consulted, by name.
//!
//! **One device, deliberately.** The list is short because each entry is a claim that the
//! platform has that device *and* that this project can serve it truthfully. `/dev/klog` earns
//! its place because orbistoun already writes exactly what belongs in it (D389); `/dev/random`,
//! `/dev/null` and the rest do not have that argument made for them yet, and a device that
//! answers plausibly is worse than one that is absent - the guest cannot tell.

use orbistoun_core::klog;

/// The kernel log, which `klogsrv` exists to forward.
pub const KLOG: &str = "/dev/klog";

/// Which device a guest path names, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Device {
    /// [`KLOG`].
    KernelLog,
}

/// The device a path names, or nothing.
///
/// Exact match only. A device is a name, not a prefix: `/dev/klog.old` is not the kernel log,
/// and treating it as one would answer a guest's typo with a working descriptor.
#[must_use]
pub fn named(guest_path: &str) -> Option<Device> {
    match guest_path.replace('\\', "/").as_str() {
        KLOG => Some(Device::KernelLog),
        _ => None,
    }
}

/// The directory every device here lives in.
///
/// Named so `stat` and a listing can agree that `/dev` is a directory without the mount table
/// knowing about it - nothing is mounted there and nothing should be.
pub const DIRECTORY: &str = "/dev";

/// Whether a guest path is the device directory.
#[must_use]
pub fn is_directory(guest_path: &str) -> bool {
    let path = guest_path.replace('\\', "/");
    path.trim_end_matches('/') == DIRECTORY
}

/// The names inside [`DIRECTORY`].
#[must_use]
pub fn in_directory() -> Vec<String> {
    vec![KLOG.trim_start_matches("/dev/").to_owned()]
}

impl Device {
    /// Reads from the device, answering how many bytes arrived.
    ///
    /// Zero means *nothing waiting*, which for a log is not an end of file: the kernel is
    /// still running and will have more to say. A guest that treats zero as the end stops
    /// early, which is why `select` and `kevent` report a log with nothing in it as **not
    /// ready** rather than readable.
    pub fn read(self, into: &mut [u8]) -> usize {
        match self {
            Self::KernelLog => klog::read_into(into),
        }
    }

    /// Whether a read would return something without waiting.
    #[must_use]
    pub fn readable(self) -> bool {
        match self {
            Self::KernelLog => klog::has_lines(),
        }
    }

    /// Whether a write would be accepted.
    ///
    /// **Never, for the kernel log.** A program writing to `/dev/klog` on this platform would
    /// be asking the kernel to log on its behalf, and a guest that could inject lines into the
    /// record another guest reads is a guest editing the evidence. Refused rather than
    /// discarded, so a caller that checks knows.
    #[must_use]
    pub const fn writable(self) -> bool {
        match self {
            Self::KernelLog => false,
        }
    }
}

#[cfg(test)]
mod tests {
    /// **A device is a name, not a prefix.**
    #[test]
    fn only_the_exact_name_is_the_device() {
        assert_eq!(super::named("/dev/klog"), Some(super::Device::KernelLog));
        assert_eq!(super::named("/dev/klog.old"), None);
        assert_eq!(super::named("/dev/klo"), None);
        assert_eq!(super::named("/dev"), None);
        assert_eq!(super::named(""), None);
    }

    /// A guest that mixes separators still names it.
    #[test]
    fn a_backslash_separator_still_names_it() {
        assert_eq!(super::named("\\dev\\klog"), Some(super::Device::KernelLog));
    }

    /// The device directory is a directory, and the device inside it is not.
    #[test]
    fn the_device_directory_is_one() {
        assert!(super::is_directory("/dev"));
        assert!(super::is_directory("/dev/"));
        assert!(!super::is_directory("/dev/klog"));
        assert!(!super::is_directory("/devices"));
        assert_eq!(super::in_directory(), vec!["klog".to_owned()]);
    }

    /// The kernel log is never writable, whatever a guest asks.
    #[test]
    fn the_kernel_log_refuses_writes() {
        assert!(!super::Device::KernelLog.writable());
    }
}
