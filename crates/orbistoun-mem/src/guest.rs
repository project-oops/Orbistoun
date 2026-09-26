//! Reads and writes of guest memory by guest address.
//!
//! The guest runs in this process under an identity mapping: a guest address is the host
//! address of the same byte. Every accessor refuses the null address and an address that does
//! not fit a host pointer, and accesses unaligned, because guest structures promise no
//! alignment.
//!
//! # Safety
//!
//! Every function here has one contract. Apart from the refused addresses, the bytes it touches
//! (the width of the value, or a string up to its terminator or its bound) must be mapped guest
//! memory, readable, and writable for the writers, for the duration of the call. That is the
//! contract of the guest call that supplied the address.

/// The longest guest path read, terminator excluded.
pub const MAX_PATH: usize = 1024;

/// The host address of guest address `at`, or `None` for null or an address past the host
/// pointer width.
fn host(at: u64) -> Option<usize> {
    usize::try_from(at).ok().filter(|&at| at != 0)
}

/// The little-endian `u32` at `at`.
///
/// # Safety
///
/// See the [module contract](self#safety).
#[must_use]
pub unsafe fn read_u32(at: u64) -> Option<u32> {
    let at = host(at)?;
    // SAFETY: the module contract: four readable guest bytes at `at`.
    Some(unsafe { std::ptr::read_unaligned(std::ptr::with_exposed_provenance::<u32>(at)) })
}

/// The little-endian `u64` at `at`.
///
/// # Safety
///
/// See the [module contract](self#safety).
#[must_use]
pub unsafe fn read_u64(at: u64) -> Option<u64> {
    let at = host(at)?;
    // SAFETY: the module contract: eight readable guest bytes at `at`.
    Some(unsafe { std::ptr::read_unaligned(std::ptr::with_exposed_provenance::<u64>(at)) })
}

/// Writes `value` little-endian at `at`; `false`, writing nothing, for a refused address.
///
/// # Safety
///
/// See the [module contract](self#safety).
pub unsafe fn write_u32(at: u64, value: u32) -> bool {
    let Some(at) = host(at) else {
        return false;
    };
    // SAFETY: the module contract: four writable guest bytes at `at`.
    unsafe { std::ptr::write_unaligned(std::ptr::with_exposed_provenance_mut::<u32>(at), value) };
    true
}

/// Writes `value` little-endian at `at`; `false`, writing nothing, for a refused address.
///
/// # Safety
///
/// See the [module contract](self#safety).
pub unsafe fn write_u64(at: u64, value: u64) -> bool {
    let Some(at) = host(at) else {
        return false;
    };
    // SAFETY: the module contract: eight writable guest bytes at `at`.
    unsafe { std::ptr::write_unaligned(std::ptr::with_exposed_provenance_mut::<u64>(at), value) };
    true
}

/// Copies `bytes` to `at`; `false`, writing nothing, for a refused address.
///
/// # Safety
///
/// See the [module contract](self#safety).
pub unsafe fn write_bytes(at: u64, bytes: &[u8]) -> bool {
    let Some(at) = host(at) else {
        return false;
    };
    // SAFETY: the module contract: `bytes.len()` writable guest bytes at `at`, which cannot
    // overlap a host slice the caller holds.
    unsafe {
        std::ptr::copy_nonoverlapping(
            bytes.as_ptr(),
            std::ptr::with_exposed_provenance_mut::<u8>(at),
            bytes.len(),
        );
    }
    true
}

/// The bytes of the NUL-terminated string at `at`, without the terminator, stopping after
/// `max` bytes when no terminator comes first.
///
/// The string is read one byte at a time, so an unterminated string reads at most `max` bytes
/// and a scan never runs past the end of a mapping further than it has read.
///
/// # Safety
///
/// See the [module contract](self#safety).
#[must_use]
pub unsafe fn read_cstr(at: u64, max: usize) -> Option<Vec<u8>> {
    let at = host(at)?;
    let mut bytes = Vec::new();
    for offset in 0..max {
        // SAFETY: the module contract: the string's bytes up to its terminator are readable.
        let byte = unsafe { std::ptr::read(std::ptr::with_exposed_provenance::<u8>(at + offset)) };
        if byte == 0 {
            break;
        }
        bytes.push(byte);
    }
    Some(bytes)
}

/// The guest path at `at`: at most [`MAX_PATH`] bytes, and `None` unless they are UTF-8.
///
/// # Safety
///
/// See the [module contract](self#safety).
#[must_use]
pub unsafe fn read_path(at: u64) -> Option<String> {
    // SAFETY: the module contract, passed on.
    String::from_utf8(unsafe { read_cstr(at, MAX_PATH) }?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A host buffer's address, as a guest would pass it.
    fn address<T>(value: &T) -> u64 {
        std::ptr::from_ref(value).expose_provenance() as u64
    }

    /// Words and bytes round-trip unaligned.
    #[test]
    fn values_round_trip_unaligned() {
        let mut buffer = [0_u8; 16];
        let base = std::ptr::from_mut(&mut buffer).expose_provenance() as u64 + 1;
        // SAFETY: `base..base + 8` lies inside `buffer`.
        assert!(unsafe { write_u64(base, 0x0102_0304_0506_0708) });
        // SAFETY: `base + 8..base + 12` lies inside `buffer`.
        assert!(unsafe { write_u32(base + 8, 0xAABB_CCDD) });
        // SAFETY: `base + 12..base + 14` lies inside `buffer`.
        assert!(unsafe { write_bytes(base + 12, b"hi") });
        // SAFETY: as the write above.
        assert_eq!(unsafe { read_u64(base) }, Some(0x0102_0304_0506_0708));
        // SAFETY: as the write above.
        assert_eq!(unsafe { read_u32(base + 8) }, Some(0xAABB_CCDD));
        // SAFETY: as the write above.
        assert_eq!(unsafe { read_cstr(base + 12, 2) }, Some(b"hi".to_vec()));
    }

    /// The null address is refused without a dereference.
    #[test]
    fn null_is_refused() {
        // SAFETY: null is refused before any access, here and below.
        assert_eq!(unsafe { read_u32(0) }, None);
        // SAFETY: as above.
        assert_eq!(unsafe { read_u64(0) }, None);
        // SAFETY: as above.
        assert!(!unsafe { write_u32(0, 1) });
        // SAFETY: as above.
        assert!(!unsafe { write_u64(0, 1) });
        // SAFETY: as above.
        assert!(!unsafe { write_bytes(0, b"x") });
        // SAFETY: as above.
        assert_eq!(unsafe { read_path(0) }, None);
    }

    /// A string stops at its terminator, or at the bound when it has none, and a path must be
    /// UTF-8.
    #[test]
    fn strings_stop_at_the_terminator_or_the_bound() {
        let text = *b"abc\0def";
        let unterminated = *b"xyzw";
        let invalid = [0xFF_u8, 0];
        // SAFETY: the read stops at the NUL inside `text`.
        let terminated = unsafe { read_cstr(address(&text), 64) };
        assert_eq!(terminated, Some(b"abc".to_vec()));
        // SAFETY: the bound stops the read inside `unterminated`.
        let bounded = unsafe { read_cstr(address(&unterminated), 3) };
        assert_eq!(bounded, Some(b"xyz".to_vec()));
        // SAFETY: as the first read.
        assert_eq!(unsafe { read_path(address(&text)) }, Some("abc".to_owned()));
        // SAFETY: `invalid` is terminated.
        assert_eq!(unsafe { read_path(address(&invalid)) }, None);
    }
}
