//! A stream of well-distributed bytes, deterministic on purpose.
//!
//! Every measurement rests on two runs of one build behaving identically, so a guest
//! gets bytes that are well distributed rather than unpredictable: enough for a generator it
//! seeds, a hash it salts, or an identifier it wants distinct (D578). This is not cryptography.
//! The state is one shared pool, as a kernel's is, so `std::random_device` and the random
//! devices in `orbistoun-fs` advance the same stream. The generator is `splitmix64`
//! (Sebastiano Vigna, public domain).

use core::sync::atomic::{AtomicU64, Ordering};

/// The `splitmix64` increment, also the seed.
///
/// Any odd constant serves; this is the reference's. Fixed rather than taken from the clock.
const GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;

/// Where the stream has got to.
static STATE: AtomicU64 = AtomicU64::new(GAMMA);

/// The `splitmix64` finaliser: one state value in, one well-distributed word out.
///
/// Pure, so the generator is testable without advancing the shared stream a guest or a
/// parallel test would see.
#[must_use]
pub const fn mix(state: u64) -> u64 {
    let mut z = state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The next word of the stream.
///
/// The thin effectful half: it advances the shared state and mixes it.
#[must_use]
pub fn next_word() -> u64 {
    mix(STATE
        .fetch_add(GAMMA, Ordering::Relaxed)
        .wrapping_add(GAMMA))
}

/// Fills `into` from the stream.
///
/// Every byte is written, including a tail shorter than a word, so no stale bytes reach the
/// guest as apparent randomness.
pub fn fill(into: &mut [u8]) {
    for chunk in into.chunks_mut(8) {
        let word = next_word().to_le_bytes();
        chunk.copy_from_slice(&word[..chunk.len()]);
    }
}

#[cfg(test)]
mod tests {
    /// The mixer is a function of its input alone and does not collide on distinct states.
    #[test]
    fn the_mixer_is_deterministic_and_spreads_its_input() {
        assert_eq!(super::mix(1), super::mix(1), "same state, same word");
        assert_ne!(
            super::mix(1),
            super::mix(2),
            "adjacent states must not collide"
        );

        // `mix(0)` is 0 in the published finaliser: every step is a multiply or a shift-xor of zero.
        // The state reaches it once per 2^64 words, since an odd increment walks every value.
        assert_eq!(
            super::mix(0),
            0,
            "the finaliser's fixed point, by construction"
        );
    }

    /// Every byte is written, including a tail that does not fill a word.
    #[test]
    fn a_buffer_is_filled_to_its_last_byte() {
        // Nine bytes: one whole word and a single-byte tail, with a sentinel the stream is unlikely
        // to produce in every position.
        let mut buffer = [0xA5_u8; 9];
        super::fill(&mut buffer);
        assert!(
            buffer.iter().any(|&b| b != 0xA5),
            "the buffer was not written at all"
        );
        let mut tail = [0xA5_u8; 1];
        super::fill(&mut tail);
        // A single byte matches the sentinel 1 time in 256, so several draws are checked to keep
        // this from being flaky.
        let mut wrote_the_tail = false;
        for _ in 0..8 {
            super::fill(&mut tail);
            if tail[0] != 0xA5 {
                wrote_the_tail = true;
                break;
            }
        }
        assert!(wrote_the_tail, "a one-byte tail was never written");
    }

    /// The stream advances, so two reads do not hand a guest the same bytes.
    #[test]
    fn the_stream_does_not_repeat_itself() {
        let mut first = [0_u8; 16];
        let mut second = [0_u8; 16];
        super::fill(&mut first);
        super::fill(&mut second);
        assert_ne!(first, second, "two reads returned the same bytes");
    }
}
