//! A stream of well-distributed bytes, and it is deterministic on purpose.
//!
//! # Why an emulator's randomness must not be random
//!
//! Every measurement this project makes - `FURTHER`/`same`/`BACK`, a fault address, a call
//! budget - rests on two runs of one build behaving identically (D181, D238). A guest seeded
//! from a physically random source takes a different path each time, and the difference between
//! two runs stops meaning anything about the change being measured.
//!
//! So what a guest gets here is **well distributed rather than unpredictable**: enough for a
//! generator it seeds, a hash it salts, or an identifier it wants distinct, and reproducible
//! across runs. `std::random_device` has answered from this reasoning since it was implemented;
//! this module is that generator lifted out so the random devices in `orbistoun-fs` answer from
//! the same stream instead of a second copy of it.
//!
//! **A guest doing cryptography against this is not getting cryptography.** Nothing observed
//! does, and if something starts to, that is a decision to take then rather than a property to
//! quietly rely on now.
//!
//! # One pool, as a real system has
//!
//! The state is shared, so a guest reading a random device advances what the C++ entropy source
//! answers next, exactly as two readers of one kernel pool would. That keeps the model honest
//! and it keeps the run reproducible, because a deterministic guest makes the same calls in the
//! same order.
//!
//! The generator is `splitmix64` (Sebastiano Vigna, public domain).

use core::sync::atomic::{AtomicU64, Ordering};

/// The `splitmix64` increment, also the seed.
///
/// Any odd constant serves and this is the one the reference uses. Fixed rather than taken from
/// the clock, which is the whole point of the module.
const GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;

/// Where the stream has got to.
static STATE: AtomicU64 = AtomicU64::new(GAMMA);

/// The `splitmix64` finaliser: one state value in, one well-distributed word out.
///
/// **Pure, so the generator is testable without touching the shared stream.** A test that had to
/// advance the global would change what a guest running beside it sees, and two tests in one
/// binary run on parallel threads.
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
/// Every byte is written, including a tail shorter than a word - a buffer left partly untouched
/// would hand a guest whatever it had there before and read as randomness that happened to
/// repeat.
pub fn fill(into: &mut [u8]) {
    for chunk in into.chunks_mut(8) {
        let word = next_word().to_le_bytes();
        chunk.copy_from_slice(&word[..chunk.len()]);
    }
}

#[cfg(test)]
mod tests {
    /// The mixer is a function of its input and nothing else, which is what lets a run be
    /// repeated. Two different states must not collide either, or a guest seeding from
    /// successive words would get the same sequence twice.
    #[test]
    fn the_mixer_is_deterministic_and_spreads_its_input() {
        assert_eq!(super::mix(1), super::mix(1), "same state, same word");
        assert_ne!(
            super::mix(1),
            super::mix(2),
            "adjacent states must not collide"
        );

        // **`mix(0)` is 0, and that is the published finaliser rather than a defect here.**
        // Asserted so nobody "fixes" it: every step is a multiply or a shift-xor of zero, so
        // zero is a fixed point. The state reaches it once per 2^64 words - an odd increment
        // walks every value - and one zero word in that many is a property of the generator
        // this project chose, not a hole in it. Stated because the first version of this test
        // asserted the opposite and was wrong.
        assert_eq!(
            super::mix(0),
            0,
            "the finaliser's fixed point, by construction"
        );
    }

    /// Every byte is written, including a tail that does not fill a word.
    ///
    /// The failure this catches is silent: a buffer whose last bytes keep whatever the caller
    /// had there reads as randomness that happened to repeat, and a guest hashing it would get
    /// a collision it could never explain.
    #[test]
    fn a_buffer_is_filled_to_its_last_byte() {
        // Nine bytes: one whole word and a single-byte tail. A sentinel that the stream is
        // vanishingly unlikely to produce in every position, so an unwritten byte shows up.
        let mut buffer = [0xA5_u8; 9];
        super::fill(&mut buffer);
        assert!(
            buffer.iter().any(|&b| b != 0xA5),
            "the buffer was not written at all"
        );
        let mut tail = [0xA5_u8; 1];
        super::fill(&mut tail);
        // A single byte comes from a fresh word each time, so the odds of it matching the
        // sentinel are 1 in 256 - checked across several draws rather than one, which makes
        // this a real assertion instead of a flaky one.
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
