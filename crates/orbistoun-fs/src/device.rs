//! The paths that are devices rather than files, answered before the mount table.
//!
//! `/dev/klog` has no host path: it is a stream the kernel produces, and here the kernel is
//! this emulator, which already writes what belongs in it (D389). The list is short because
//! each entry claims the platform has the device and that it is served truthfully; a device
//! answering plausibly is worse than an absent one. `/dev/random` qualifies because its
//! bytes carry no meaning, so serving it invents nothing. Its stream is deterministic,
//! the emulator-wide trade [`orbistoun_core::entropy`] documents.

use orbistoun_core::klog;

/// The kernel log, which `klogsrv` exists to forward.
pub const KLOG: &str = "/dev/klog";

/// The random device, and the name a program actually opens.
///
/// Both names reach one device, as on FreeBSD: `urandom` is the same device under a second
/// name and neither blocks once seeded.
pub const RANDOM: &str = "/dev/random";
/// The second name for [`RANDOM`].
pub const URANDOM: &str = "/dev/urandom";

/// Which device a guest path names, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Device {
    /// [`KLOG`].
    KernelLog,
    /// [`RANDOM`] and [`URANDOM`].
    Random,
}

/// The device a path names, or nothing.
///
/// Exact match only: `/dev/klog.old` is not the kernel log.
#[must_use]
pub fn named(guest_path: &str) -> Option<Device> {
    match guest_path.replace('\\', "/").as_str() {
        KLOG => Some(Device::KernelLog),
        RANDOM | URANDOM => Some(Device::Random),
        _ => None,
    }
}

/// The directory every device here lives in.
///
/// Named so `stat` and a listing agree that `/dev` is a directory without anything mounted
/// there.
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
    vec![
        KLOG.trim_start_matches("/dev/").to_owned(),
        RANDOM.trim_start_matches("/dev/").to_owned(),
        URANDOM.trim_start_matches("/dev/").to_owned(),
    ]
}

impl Device {
    /// Reads from the device, answering how many bytes arrived.
    ///
    /// Zero means nothing waiting, which for a log is not end of file: the kernel is still
    /// running. So `select` and `kevent` report an empty log as not ready rather than readable.
    pub fn read(self, into: &mut [u8]) -> usize {
        match self {
            Self::KernelLog => klog::read_into(into),
            // Deterministic; see `orbistoun_core::entropy`. Enough to seed a generator, not
            // for cryptography.
            Self::Random => {
                orbistoun_core::entropy::fill(into);
                into.len()
            }
        }
    }

    /// Whether a read would return something without waiting.
    #[must_use]
    pub fn readable(self) -> bool {
        match self {
            Self::KernelLog => klog::has_lines(),
            // Always: FreeBSD's random device does not block once seeded.
            Self::Random => true,
        }
    }

    /// Whether a write would be accepted.
    ///
    /// Never for the kernel log: a guest writing lines into the record another guest reads
    /// would be forging it. Refused rather than discarded, so a caller that checks knows.
    /// Every device has its own arm, even where the answer is the same, so adding a device
    /// means deciding its answer.
    #[must_use]
    #[allow(clippy::match_same_arms)]
    pub const fn writable(self) -> bool {
        match self {
            Self::KernelLog => false,
            // A write to FreeBSD's random device stirs the pool; this pool is a fixed sequence
            // with nothing to stir, so accepting the write would claim an effect (D578).
            Self::Random => false,
        }
    }
}

#[cfg(test)]
mod tests {
    /// A device is a name, not a prefix.
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
        assert_eq!(
            super::in_directory(),
            vec!["klog".to_owned(), "random".to_owned(), "urandom".to_owned()],
            "the listing is the set of names that open, and `random_tests` holds it to that"
        );
    }

    /// The kernel log is never writable, whatever a guest asks.
    #[test]
    fn the_kernel_log_refuses_writes() {
        assert!(!super::Device::KernelLog.writable());
    }
}

#[cfg(test)]
mod random_tests {
    use super::Device;

    /// Both names reach the device, and neither is a prefix match.
    #[test]
    fn the_random_device_answers_to_both_its_names() {
        assert_eq!(super::named("/dev/random"), Some(Device::Random));
        assert_eq!(super::named("/dev/urandom"), Some(Device::Random));
        assert_eq!(
            super::named("/dev/random2"),
            None,
            "a device is a name, not a prefix"
        );
        assert_eq!(super::named("/dev/rand"), None);
    }

    /// A read fills the whole buffer and says so.
    ///
    /// Reporting bytes it did not write would hand a guest its own stale buffer as randomness.
    #[test]
    fn a_read_fills_what_it_says_it_filled() {
        let mut buffer = [0xA5_u8; 9];
        let got = Device::Random.read(&mut buffer);
        assert_eq!(got, buffer.len(), "the count must be what was written");
        assert!(
            buffer.iter().any(|&b| b != 0xA5),
            "the buffer was left as the caller had it"
        );
    }

    /// Ready without waiting, and not writable.
    #[test]
    fn the_random_device_is_readable_and_refuses_writes() {
        assert!(
            Device::Random.readable(),
            "FreeBSD's does not block once seeded, and there is no pool to be short of here"
        );
        assert!(
            !Device::Random.writable(),
            "a write stirs a pool on FreeBSD, and there is nothing here to stir"
        );
    }

    /// The device directory lists it, so a guest enumerating `/dev` finds what it can open.
    #[test]
    fn the_device_directory_lists_every_name_that_opens() {
        let listed = super::in_directory();
        for name in ["klog", "random", "urandom"] {
            assert!(listed.contains(&name.to_owned()), "{name} is not listed");
            assert!(
                super::named(&format!("/dev/{name}")).is_some(),
                "{name} is listed but does not open"
            );
        }
    }
}
