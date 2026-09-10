//! The paths that are not files, and are not on the host either.
//!
//! # Why a device needs its own answer
//!
//! Everything else in this crate resolves a guest path to a host path and opens it. `/dev/klog`
//! has no host path: it is a stream the kernel produces, and here the kernel is this emulator.
//! So it is answered before the mount table is consulted, by name.
//!
//! **A short list, deliberately.** Each entry is a claim that the platform has that device *and*
//! that this project can serve it truthfully, because a device answering plausibly is worse than
//! one that is absent - the guest cannot tell. `/dev/klog` earns its place because orbistoun
//! already writes exactly what belongs in it (D389).
//!
//! `/dev/random` earns its place on a different argument, and it is worth stating rather than
//! assuming: **a random device's contract is that its bytes carry no meaning.** There is no
//! layout to invent and no vendor semantics to guess, which is what disqualifies most candidates
//! here - so it can be served without inventing anything. Two titles ask for it by name (D578).
//!
//! What it hands back is deterministic, which is the emulator-wide trade
//! [`orbistoun_core::entropy`] documents: a run that cannot be repeated cannot be measured. That
//! is a real limitation and it is the *device's* limitation to state, not something for a reader
//! to discover.
//!
//! `/dev/null` and the rest still do not have an argument made for them.

use orbistoun_core::klog;

/// The kernel log, which `klogsrv` exists to forward.
pub const KLOG: &str = "/dev/klog";

/// The random device, and the name a program actually opens.
///
/// **Both names, one device, which is FreeBSD's own arrangement** - `urandom` is the same
/// device under a second name and neither blocks once the pool is seeded. The target kernel
/// is FreeBSD-derived, so that is a citable shape rather than a guess.
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
/// Exact match only. A device is a name, not a prefix: `/dev/klog.old` is not the kernel log,
/// and treating it as one would answer a guest's typo with a working descriptor.
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
    vec![
        KLOG.trim_start_matches("/dev/").to_owned(),
        RANDOM.trim_start_matches("/dev/").to_owned(),
        URANDOM.trim_start_matches("/dev/").to_owned(),
    ]
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
            // Deterministic on purpose, and that caveat belongs to the whole emulator rather
            // than to this device - see `orbistoun_core::entropy`. A guest seeding a
            // generator gets what it needs; a guest doing cryptography does not, and
            // nothing observed does.
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
            // Always. FreeBSD's random device does not block once seeded, and there is no
            // pool to be short of here - so a `select` reports it ready, which is true.
            Self::Random => true,
        }
    }

    /// Whether a write would be accepted.
    ///
    /// **Never, for the kernel log.** A program writing to `/dev/klog` on this platform would
    /// be asking the kernel to log on its behalf, and a guest that could inject lines into the
    /// record another guest reads is a guest editing the evidence. Refused rather than
    /// discarded, so a caller that checks knows.
    ///
    /// **Every device answers for itself, even where the answer is the same.** Clippy is right
    /// that the arms are identical and wrong about what to do with it: collapsing them to a
    /// wildcard would make the next device added silently non-writable, without anyone deciding
    /// it. The reasons differ - the log must not be forgeable, the random pool has nothing to
    /// stir - and each new device should have to write one.
    #[must_use]
    #[allow(clippy::match_same_arms)]
    pub const fn writable(self) -> bool {
        match self {
            Self::KernelLog => false,
            // A write to FreeBSD's random device stirs the pool, and this pool is a fixed
            // sequence with nothing to stir. Accepting the write would claim an effect
            // that does not happen, so it is refused and the caller can tell (D578).
            Self::Random => false,
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
    /// The failure this catches is the one the module note warns about: a device that answers
    /// plausibly. Reporting bytes it did not write would hand a guest its own stale buffer and
    /// call it randomness, which is indistinguishable from working until something hashes it.
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
