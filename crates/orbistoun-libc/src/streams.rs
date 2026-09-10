//! The standard streams, which a guest imports as **data** rather than as functions.
//!
//! # Why these are not stubs
//!
//! `_Stdout` and `_Stderr` are objects holding a `FILE *`, not functions answering one. A
//! guest writes `fprintf(_Stdout, ...)`, and what it passes is whatever those objects contain
//! at the moment it reads them - so nothing can be resolved lazily on the first call, because
//! there is no call. They have to be filled before the guest starts (D307, D323).
//!
//! # What is put in them
//!
//! Not an invented `FILE` layout. `fprintf` already routes a stream by asking
//! [`orbistoun_fs::open::wrapped_descriptor`] which descriptor it stands for, and
//! `wrap_descriptor` mints exactly that kind of handle - the same mechanism `fdopen` uses. So
//! the standard streams are two wrapped descriptors, 1 and 2, and everything that already
//! knows how to write to a wrapped stream works on them unchanged.
//!
//! This matters because until now `fprintf(stdout, ...)` and `fprintf(stderr, ...)` both fell
//! through to the host's error stream: readable, but indistinguishable, and a guest's stdout
//! could not be told from its diagnostics.

/// The descriptors the standard streams stand for, as POSIX fixes them.
///
/// Reference: POSIX.1-2008 `<unistd.h>` - `STDOUT_FILENO` is 1 and `STDERR_FILENO` is 2.
const STANDARD: &[(&str, u64)] = &[("_Stdout", 1), ("_Stderr", 2)];

/// Fills the standard-stream objects, for a guest that imports them.
///
/// Called once, after the data imports have been published and before the guest is entered.
/// A guest that imports neither is left alone: there is nowhere to write and nothing that
/// would ever read it, which `write_guest_word` already treats as the honest answer.
pub fn install() {
    for (name, descriptor) in STANDARD {
        if orbistoun_thunk::data_symbol(name).is_none() {
            // Not imported by this guest, so there is no object to fill. Asked before the
            // wrap so a run that needs neither stream mints no handles either.
            continue;
        }
        let Some(handle) = orbistoun_fs::open::wrap_descriptor(*descriptor) else {
            // A descriptor that cannot be wrapped leaves the object holding zero, which is a
            // null `FILE *` - something a guest can test. Inventing a handle that stood for
            // nothing would be worse: it would be written to and go nowhere (D125).
            continue;
        };
        crate::write_guest_word(name, handle);
    }
}

#[cfg(test)]
mod tests {
    /// The names and descriptors are the POSIX ones, and are not accidentally swapped -
    /// which would send a guest's diagnostics to its output and vice versa, and read as
    /// working until somebody piped one of them.
    #[test]
    fn the_streams_name_the_posix_descriptors() {
        assert_eq!(super::STANDARD[0], ("_Stdout", 1));
        assert_eq!(super::STANDARD[1], ("_Stderr", 2));
    }
}
