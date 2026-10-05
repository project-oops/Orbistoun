//! `libSceRandom` - random bytes for a title.
//!
//! A title reaches the platform's random source through `sceRandomGetRandomNumber(buf, size)`:
//! oops-apps' hosted `arc4random_buf` binds it to this library, loads the module first and asks
//! for at most 64 bytes a call, and SuperTuxKart plays on hardware through it. The family it
//! serves has no failure return, so a refusal there aborts the title.
//!
//! The bytes are a fixed-seed stream rather than the host's randomness, for the reason
//! `sceKernelUuidCreate` is a counter: the only progress measure this project has is whether one
//! run reached further than the last, and bytes that changed every run would be a difference
//! between two traces that meant nothing (D256). No guest can tell the stream from random.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError, GuestFn};
use orbistoun_hle::guest_module;
use orbistoun_mem::guest;

guest_module! {
    "libSceRandom" {
        "sceRandomGetRandomNumber" => 2,
    }
}

/// Successful return, as a guest reads it.
const OK: u64 = 0;

/// The most bytes one call has been seen to ask for, and answered, on hardware.
const MOST_BYTES: u64 = 64;

/// The stream's state: splitmix64, from a fixed seed so every run hands out the same bytes.
static STATE: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0x5eed_0f0f_5eed_0f0f);

/// The next eight bytes of the stream (splitmix64).
fn next_word() -> u64 {
    let mut z = STATE
        .fetch_add(0x9e37_79b9_7f4a_7c15, std::sync::atomic::Ordering::Relaxed)
        .wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// `size` bytes of the stream.
fn stream_bytes(size: usize) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(size.next_multiple_of(8));
    while bytes.len() < size {
        bytes.extend_from_slice(&next_word().to_le_bytes());
    }
    bytes.truncate(size);
    bytes
}

/// `sceRandomGetRandomNumber(buf, size)` - fills `size` bytes at `buf` and answers 0.
///
/// A null destination, or more than the 64 bytes a call has been seen to take, is refused with
/// the reserved placeholder rather than a vendor code nothing measured.
fn random_get_random_number(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (buf, size) = (args[0], args[1]);
    if buf == 0 || size > MOST_BYTES {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    let Ok(size) = usize::try_from(size) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    // SAFETY: an address the guest passed for this call, `size` bytes long by its contract.
    if !unsafe { guest::write_bytes(buf, &stream_bytes(size)) } {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// Implementations this module provides, by symbol name.
#[must_use]
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[("sceRandomGetRandomNumber", random_get_random_number)]
}

#[cfg(test)]
mod tests {
    /// The stream hands out every byte value and does not repeat a word early, so a guest seeding
    /// from it sees no constant.
    #[test]
    fn the_stream_is_varied() {
        let bytes = super::stream_bytes(4096);
        let mut seen = [false; 256];
        for &byte in &bytes {
            seen[usize::from(byte)] = true;
        }
        assert!(seen.iter().all(|&s| s), "every byte value appears");
        let words: std::collections::BTreeSet<u64> = (0..512).map(|_| super::next_word()).collect();
        assert_eq!(words.len(), 512, "no word repeats");
    }

    /// A request past what has been measured, or with nowhere to write, is refused and writes
    /// nothing.
    #[test]
    fn an_unmeasured_size_or_a_null_destination_is_refused() {
        let refused = u64::from(orbistoun_core::GuestError::InvalidArgument.as_raw());
        let mut args = [0u64; orbistoun_core::GUEST_ARG_REGISTERS];
        args[1] = 16;
        assert_eq!(
            super::random_get_random_number(&args),
            refused,
            "null destination"
        );
        args[0] = 0x1000;
        args[1] = 65;
        assert_eq!(super::random_get_random_number(&args), refused, "65 bytes");
    }
}
