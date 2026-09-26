//! Starting another title: `sceSystemServiceLaunchApp`.
//!
//! On the hardware the system suspends the caller and brings the named title up in its place.
//! Here one process runs one guest, so the request goes to whoever owns the run: the worker
//! streams it to the front end, which ends this run and starts the title. The guest is told the
//! request was taken and carries on until its run is ended.

use std::sync::OnceLock;

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError, GuestFn};

/// Answered for a request with no title id to launch.
///
/// The hardware's own error is unmeasured; this is the placeholder range's invalid-argument code,
/// so it cannot be read as a real one (D670).
const NO_TITLE: u64 = GuestError::InvalidArgument.as_raw() as u64;

/// The longest title id read. Ids are nine characters; the bound stops a missing terminator from
/// walking memory.
const MAX_TITLE_ID: usize = 64;

/// Receives each title id a guest asks to launch. Installed by whoever owns the run.
pub type LaunchObserver = fn(&str);

static OBSERVER: OnceLock<LaunchObserver> = OnceLock::new();

/// Installs the launch observer. First install wins.
pub fn install_launch_observer(observer: LaunchObserver) {
    let _ = OBSERVER.set(observer);
}

/// Reads the NUL-terminated title id at `address`.
fn title_id(address: u64) -> Option<String> {
    let at = usize::try_from(address).ok().filter(|&a| a != 0)?;
    let mut bytes = Vec::new();
    for offset in 0..MAX_TITLE_ID {
        // SAFETY: a guest-supplied string in identity-mapped guest memory, read one byte at a time
        // so the scan stops at the terminator.
        let byte = unsafe { std::ptr::read(std::ptr::with_exposed_provenance::<u8>(at + offset)) };
        if byte == 0 {
            break;
        }
        bytes.push(byte);
    }
    String::from_utf8(bytes).ok().filter(|id| !id.is_empty())
}

/// `sceSystemServiceLaunchApp(title_id, argv, param)` - ask the system to start a title.
///
/// `argv` and `param` (size, user, crash-report and system-version flags) are not read: the run
/// that starts the title is the front end's, configured as any other launch of it is.
fn launch_app(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(id) = title_id(args[0]) else {
        return NO_TITLE;
    };
    if let Some(observer) = OBSERVER.get() {
        observer(&id);
    }
    0
}

/// Implementations this module provides, by name.
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[("sceSystemServiceLaunchApp", launch_app)]
}

#[cfg(test)]
mod tests {
    use super::{NO_TITLE, launch_app};
    use orbistoun_core::GUEST_ARG_REGISTERS;

    /// A request names its title, and one with no title is refused rather than taken.
    #[test]
    fn a_launch_names_its_title_and_an_empty_one_is_refused() {
        let id = b"GLCB00001\0";
        let mut args = [0; GUEST_ARG_REGISTERS];
        args[0] = id.as_ptr() as u64;
        assert_eq!(super::title_id(args[0]).as_deref(), Some("GLCB00001"));
        assert_eq!(launch_app(&args), 0);

        args[0] = 0;
        assert_eq!(launch_app(&args), NO_TITLE);
        let empty = b"\0";
        args[0] = empty.as_ptr() as u64;
        assert_eq!(launch_app(&args), NO_TITLE);
    }
}
