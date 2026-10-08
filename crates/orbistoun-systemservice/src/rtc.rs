//! `libSceRtc` - the real-time clock a title counts ticks by.
//!
//! A tick is a microsecond, counted from 0001-01-01 00:00 UTC in the proleptic Gregorian calendar
//! (D776). The clock is the host's wall clock, the one `gettimeofday` answers, so the two agree to
//! the microsecond. PPSA03416 reads it around every timed condition wait it polls with.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError, GuestFn};
use orbistoun_hle::guest_module;
use orbistoun_mem::guest;

guest_module! {
    "libSceRtc" {
        // A pointer to the 64-bit tick to fill.
        "sceRtcGetCurrentTick" => 1,
    }
}

/// Successful return, as a guest reads it.
const OK: u64 = 0;

/// Seconds from 0001-01-01 to the Unix epoch: 719,162 days of the proleptic Gregorian calendar.
const SECONDS_BEFORE_UNIX_EPOCH: u64 = 719_162 * 86_400;

/// Ticks in a second: a tick is a microsecond.
const TICKS_PER_SECOND: u64 = 1_000_000;

/// The tick for a wall-clock reading of `seconds` since the Unix epoch and `nanos` beyond them.
const fn tick_at(seconds: u64, nanos: u64) -> u64 {
    (seconds + SECONDS_BEFORE_UNIX_EPOCH) * TICKS_PER_SECOND + nanos / 1_000
}

/// `sceRtcGetCurrentTick(tick)`: the current tick, written to the caller's 64-bit word (D776). A
/// null destination is refused with the reserved placeholder; the console's answer to one is not
/// measured.
fn rtc_get_current_tick(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (seconds, nanos) = orbistoun_hle::clocks::wall_clock();
    // SAFETY: an address the guest passed for this call, valid by its contract: one 64-bit word.
    if args[0] != 0 && unsafe { guest::write_u64(args[0], tick_at(seconds, nanos)) } {
        OK
    } else {
        u64::from(GuestError::InvalidArgument.as_raw())
    }
}

/// Everything here, by symbol name.
#[must_use]
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[("sceRtcGetCurrentTick", rtc_get_current_tick)]
}

#[cfg(test)]
mod tests {
    /// The Unix epoch is tick 62,135,596,800,000,000, the count .NET's `DateTime` and the platform
    /// share from the same origin; a microsecond is one tick.
    #[test]
    fn a_tick_is_a_microsecond_from_the_first_of_january_of_year_one() {
        assert_eq!(super::tick_at(0, 0), 62_135_596_800_000_000);
        assert_eq!(super::tick_at(0, 1_999), 62_135_596_800_000_001);
        assert_eq!(super::tick_at(1, 0) - super::tick_at(0, 0), 1_000_000);
    }

    /// The current tick is written where the caller asks and moves on with the wall clock; a null
    /// destination is refused.
    #[test]
    fn the_current_tick_is_written_and_advances() {
        let mut word = 0u64;
        let mut args = [0u64; orbistoun_core::GUEST_ARG_REGISTERS];
        args[0] = std::ptr::from_mut(&mut word).expose_provenance() as u64;
        assert_eq!(super::rtc_get_current_tick(&args), super::OK);
        let first = word;
        let (seconds, _) = orbistoun_hle::clocks::wall_clock();
        assert!(first / 1_000_000 >= seconds + super::SECONDS_BEFORE_UNIX_EPOCH - 1);
        // Five milliseconds pass, as a guest's wait passes them on whichever clock the run reads
        // (D735): on the host's the sleep moves it, on the logical one the advance does.
        std::thread::sleep(std::time::Duration::from_millis(5));
        orbistoun_hle::clocks::advance(5_000_000);
        assert_eq!(super::rtc_get_current_tick(&args), super::OK);
        assert!(word >= first + 4_000, "{first} then {word}");
        args[0] = 0;
        assert_ne!(super::rtc_get_current_tick(&args), super::OK);
    }
}
