//! The standard streams, which a guest imports as data rather than as functions.
//!
//! `_Stdout` and `_Stderr` are objects holding a `FILE *`, read by the guest directly, so they
//! are filled before the guest starts (D323). Each holds a wrapped descriptor, 1 or 2, minted
//! by `wrap_descriptor` as `fdopen` does, so [`orbistoun_fs::open::wrapped_descriptor`] routes
//! a guest's standard output and standard error to separate places.

/// The descriptors the standard streams stand for, as POSIX fixes them.
///
/// Reference: POSIX.1-2008 `<unistd.h>` - `STDOUT_FILENO` is 1 and `STDERR_FILENO` is 2.
const STANDARD: &[(&str, u64)] = &[("_Stdout", 1), ("_Stderr", 2)];

/// Fills the standard-stream objects, for a guest that imports them.
///
/// Called once, after the data imports are published and before the guest is entered. A
/// guest that imports neither is left alone.
pub fn install() {
    for (name, descriptor) in STANDARD {
        if orbistoun_thunk::data_symbol(name).is_none() {
            // Not imported by this guest. Checked before the wrap, so no handle is minted.
            continue;
        }
        let Some(handle) = orbistoun_fs::open::wrap_descriptor(*descriptor) else {
            // A descriptor that cannot be wrapped leaves the object holding a null `FILE *`, which
            // a guest can test (D125).
            continue;
        };
        crate::write_guest_word(name, handle);
    }
}

#[cfg(test)]
mod tests {
    /// The names and descriptors are the POSIX ones, not swapped.
    #[test]
    fn the_streams_name_the_posix_descriptors() {
        assert_eq!(super::STANDARD[0], ("_Stdout", 1));
        assert_eq!(super::STANDARD[1], ("_Stderr", 2));
    }
}
