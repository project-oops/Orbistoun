//! Audio output HLE - `libSceAudioOut`.
//!
//! Guests often block on audio-buffer completion, so a port that never drains hangs the
//! title with no audio symptom. Output is paced from the sample rate rather than a device:
//! the output ports follow the contract obSCEne's `090-audio` checks measure on hardware.
//! `ajm`, `audio3d`, `audio_in` and `audio_out2` are declarations only.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError, GuestFn, HandleAllocator};
use orbistoun_hle::guest_module;

pub mod ajm;
pub mod audio3d;
pub mod audio_in;
pub mod audio_out2;

/// The high half libSceAudioOut numbers its errors from, rather than the kernel's `0x8002_0000`.
///
/// obSCEne's `090-audio/close-rejects-bad-handle` answers `0x80260003`, the audio
/// subsystem's own `NO_SUCH`; a guest checking `SCE_AUDIO_OUT_ERROR_*` matches only this base.
const AUDIO_ERROR_BASE: u32 = 0x8026_0000;

guest_module! {
    "libSceAudioOut" {
        "sceAudioOutInit" => 0,
        "sceAudioOutOpen" => 6,
        "sceAudioOutClose" => 1,
        "sceAudioOutOutput" => 2,
        "sceAudioOutSetVolume" => 3,
        // Imported by obSCEne by name. Two arguments: the handle and the buffer the state is
        // written into.
        "sceAudioOutGetPortState" => 2,
    }
}

/// `sceAudioOutInit()` - initialise the audio-output library.
///
/// A title checks this before opening a port. Initialisation is idempotent, so a repeated
/// call also succeeds.
fn audio_out_init(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    0
}

/// `sceAudioOutClose(handle)` - close an audio-output port.
///
/// A handle nothing opened answers `0x80260003` (`090-audio/close-rejects-bad-handle`). The
/// number is not returned to the allocator, so a stale handle cannot reach another port (D017).
fn audio_out_close(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Ok(raw) = u32::try_from(args[0]) else {
        return no_such_port();
    };
    let removed = PORTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(&raw)
        .is_some();
    if removed { 0 } else { no_such_port() }
}

/// What an open refuses a format it does not serve with.
///
/// `090-audio/open-shapes` measures `0x8026_0008` for 44.1 kHz at every chunk size: the audio
/// subsystem's base, code 8. What the 8 means is not established.
pub(crate) const UNSUPPORTED_FORMAT: u32 = 0x8026_0008;

/// The one sample rate a port opens at.
///
/// `090-audio/open-shapes` accepts `0xbb80` and refuses `0xac44` at chunk `0x100` through
/// `0x800`. Chunk size decides no measured case, so nothing here decides on it.
const SERVED_FREQUENCY: u64 = 48_000;

/// How many bytes `sceAudioOutGetPortState` writes (`090-audio/format-selector` reports 16).
pub(crate) const PORT_STATE_BYTES: usize = 16;

/// The port-state image the hardware writes, for a port of this many channels.
///
/// Transcribed, not modelled. The three measured selectors differ only at offset two, the
/// channel count; the other bytes (`81 00 .. c7 ff ff 05 00`, then zeros) are the same for
/// every port measured and their meaning is not claimed (`090-audio/format-selector`).
pub(crate) fn port_state(channels: u8) -> [u8; PORT_STATE_BYTES] {
    let mut state = [0_u8; PORT_STATE_BYTES];
    state[0] = 0x81;
    state[2] = channels;
    state[3] = 0xc7;
    state[4] = 0xff;
    state[5] = 0xff;
    state[6] = 0x05;
    state
}

/// How many channels a format selector opens.
///
/// Selector 0 answers one, selectors 1 and 2 answer two. An unmeasured selector gets the
/// stereo pair rather than a refusal, since all three measured selectors open.
const fn channels_for(selector: u64) -> u8 {
    if selector == 0 { 1 } else { 2 }
}

/// One open port, and what it was opened as.
#[derive(Debug, Clone, Copy)]
struct Port {
    /// Frames per chunk, which is what decides how long an output takes.
    frames: u64,
    /// The rate those frames play at.
    frequency: u64,
    /// The format selector, which decides the channel count the state reports.
    selector: u64,
}

/// Ports this shim has opened, by handle.
static PORTS: std::sync::Mutex<std::collections::BTreeMap<u32, Port>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());

/// Handles this shim has issued.
///
/// Per-subsystem, so a guest passing a pad handle to a port call is refused (D017).
static HANDLES: std::sync::Mutex<HandleAllocator> = std::sync::Mutex::new(HandleAllocator::new());

/// The port a handle names, if this shim opened it.
fn port(raw: u64) -> Option<Port> {
    let raw = u32::try_from(raw).ok()?;
    PORTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&raw)
        .copied()
}

/// The audio subsystem's code for a handle that names nothing.
fn no_such_port() -> u64 {
    u64::from(GuestError::vendor_in(AUDIO_ERROR_BASE, orbistoun_core::errno::NO_SUCH).as_raw())
}

/// `sceAudioOutOpen(user, type, index, length, frequency, param)` - open an output port.
///
/// A port that opens must also drain, or a guest waiting on buffer completion hangs; output
/// is paced from the sample rate (see [`audio_out_output`]), which is what makes opening one
/// safe without an audio backend.
fn audio_out_open(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (frames, frequency, selector) = (args[3], args[4], args[5]);
    if frequency != SERVED_FREQUENCY {
        return u64::from(UNSUPPORTED_FORMAT);
    }
    let mut allocator = HANDLES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(handle) = allocator.alloc() else {
        return u64::from(GuestError::NoMemory.as_raw());
    };
    PORTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(
            handle.as_raw(),
            Port {
                frames,
                frequency,
                selector,
            },
        );
    u64::from(handle.as_raw())
}

/// `sceAudioOutOutput(handle, samples)` - hand a chunk of samples to a port.
///
/// Sleeps for as long as the samples take to play. There is no device; what a guest needs is
/// the pacing, and returning at once would let an audio thread spin. `090-audio/blocking`
/// treats eight 512-frame buffers at 48 kHz (85 ms of audio) returning in under 40 ms as not
/// blocking. The hardware returns sooner than real time, through a queue of unmeasured depth,
/// so there is no queue here and every call waits. The frame-count return is assumed.
fn audio_out_output(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(port) = port(args[0]) else {
        return no_such_port();
    };
    if port.frequency != 0 && port.frames != 0 {
        let playing = std::time::Duration::from_micros(
            port.frames.saturating_mul(1_000_000) / port.frequency,
        );
        std::thread::sleep(playing);
        // The guest clock is logical by default (D582), so the sleep must also advance it, or a
        // guest timing the output sees no time pass.
        orbistoun_hle::clocks::advance(playing.as_nanos());
    }
    port.frames
}

/// `sceAudioOutGetPortState(handle, state)` - what a port is set to.
///
/// Writes the sixteen measured bytes with the selector's channel count; see [`port_state`].
fn audio_out_get_port_state(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (handle, into) = (args[0], args[1]);
    let Some(port) = port(handle) else {
        return no_such_port();
    };
    if into == 0 {
        return u64::from(
            GuestError::vendor_in(AUDIO_ERROR_BASE, orbistoun_core::errno::FAULT).as_raw(),
        );
    }
    let state = port_state(channels_for(port.selector));
    // SAFETY: a guest-supplied destination under the identity mapping, written for exactly
    // the measured extent. The source is a local array and cannot overlap guest memory.
    unsafe { std::ptr::copy_nonoverlapping(state.as_ptr(), into as *mut u8, PORT_STATE_BYTES) };
    0
}

/// `sceAudioOutSetVolume(handle, flags, levels)` - accepted, with nothing to set.
///
/// `090-audio/volume-flag` measures `0x0` for flags 1, 2 and 3. Refusing would stop a title
/// that sets a volume while opening a port; the call succeeds and nothing gets quieter.
fn audio_out_set_volume(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if port(args[0]).is_none() {
        return no_such_port();
    }
    0
}

/// The audio implementations this crate provides.
///
/// The `ajm`, `audio3d`, `audio_in` and `audio_out2` modules contribute none.
#[must_use]
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("sceAudioOutInit", audio_out_init),
        ("sceAudioOutOpen", audio_out_open),
        ("sceAudioOutClose", audio_out_close),
        ("sceAudioOutOutput", audio_out_output),
        ("sceAudioOutGetPortState", audio_out_get_port_state),
        ("sceAudioOutSetVolume", audio_out_set_volume),
    ]
}

#[cfg(test)]
mod tests {
    use orbistoun_core::GUEST_ARG_REGISTERS;

    /// An implementation by name, so a test cannot reach one a guest cannot.
    fn call(name: &str, args: [u64; GUEST_ARG_REGISTERS]) -> u64 {
        let (_, function) = super::implementations()
            .iter()
            .find(|(n, _)| *n == name)
            .unwrap_or_else(|| panic!("{name} is not served, so no guest can reach it"));
        function(&args)
    }

    /// `sceAudioOutOpen(user, type, index, length, frequency, param)`, as obSCEne calls it.
    fn open(length: u64, frequency: u64, selector: u64) -> u64 {
        call("sceAudioOutOpen", [0xFF, 0, 0, length, frequency, selector])
    }

    /// 48 kHz opens and 44.1 kHz does not, at every measured chunk size, refused with
    /// `0x8026_0008` (`090-audio/open-shapes`).
    #[test]
    fn the_console_splits_these_on_the_frequency_alone() {
        for chunk in [256, 512, 1024, 2048] {
            let handle = open(chunk, 48_000, 0);
            assert!(
                (handle as i64) >= 0,
                "48000 x {chunk} was accepted on hardware, and answered {handle:#x} here"
            );
            assert_eq!(call("sceAudioOutClose", [handle, 0, 0, 0, 0, 0]), 0);

            assert_eq!(
                open(chunk, 44_100, 0) as u32,
                super::UNSUPPORTED_FORMAT,
                "44100 x {chunk} was refused on hardware, with this code"
            );
        }
    }

    /// The port state is sixteen bytes, and the selector decides the channel count at offset two.
    #[test]
    fn the_port_state_carries_the_channel_count_the_selector_asked_for() {
        for (selector, channels) in [(0_u64, 1_u8), (1, 2), (2, 2)] {
            let handle = open(512, 48_000, selector);
            assert!((handle as i64) >= 0, "selector {selector} opens");

            let mut into = [0xAA_u8; 32];
            let rc = call(
                "sceAudioOutGetPortState",
                [
                    handle,
                    into.as_mut_ptr().expose_provenance() as u64,
                    0,
                    0,
                    0,
                    0,
                ],
            );
            assert_eq!(rc, 0, "the state of an open port is readable");
            assert_eq!(
                &into[..super::PORT_STATE_BYTES],
                super::port_state(channels).as_slice(),
                "selector {selector} was measured reporting {channels} channel(s)"
            );
            assert!(
                into[super::PORT_STATE_BYTES..].iter().all(|b| *b == 0xAA),
                "and nothing past the sixteen bytes measured"
            );
            assert_eq!(call("sceAudioOutClose", [handle, 0, 0, 0, 0, 0]), 0);
        }
    }

    /// Output takes at least as long as the samples take to play.
    ///
    /// Asserted as a floor: a loaded machine may take longer, while returning early lets a guest
    /// waiting on buffer completion spin.
    #[test]
    fn output_paces_itself_against_the_sample_rate() {
        let handle = open(512, 48_000, 0);
        assert!((handle as i64) >= 0);
        let silence = [0_u8; 512 * 2 * 2];
        let at = silence.as_ptr().expose_provenance() as u64;

        let started = std::time::Instant::now();
        for _ in 0..8 {
            call("sceAudioOutOutput", [handle, at, 0, 0, 0, 0]);
        }
        let elapsed = started.elapsed();

        assert!(
            elapsed >= std::time::Duration::from_millis(40),
            "eight 512-frame buffers at 48 kHz took {elapsed:?}, which reads as not blocking"
        );
        assert_eq!(call("sceAudioOutClose", [handle, 0, 0, 0, 0, 0]), 0);
    }

    /// A handle nobody opened is refused with the audio subsystem's own code, not a placeholder.
    #[test]
    fn a_handle_nobody_opened_is_refused_everywhere() {
        const NOBODYS: u64 = 0x7FFF;
        let mut into = [0xAA_u8; super::PORT_STATE_BYTES];
        let bad_handle =
            u64::from(orbistoun_core::GuestError::vendor_in(super::AUDIO_ERROR_BASE, 3).as_raw());

        assert_eq!(
            call("sceAudioOutClose", [NOBODYS, 0, 0, 0, 0, 0]),
            bad_handle
        );
        assert_eq!(
            call(
                "sceAudioOutGetPortState",
                [
                    NOBODYS,
                    into.as_mut_ptr().expose_provenance() as u64,
                    0,
                    0,
                    0,
                    0
                ]
            ),
            bad_handle
        );
        assert_eq!(
            call("sceAudioOutSetVolume", [NOBODYS, 3, 0, 0, 0, 0]),
            bad_handle
        );
        assert!(
            into.iter().all(|b| *b == 0xAA),
            "and the guest's buffer is untouched"
        );
    }

    /// Setting the volume on an open port is accepted (`090-audio/volume-flag`).
    #[test]
    fn setting_the_volume_on_an_open_port_is_accepted() {
        let handle = open(512, 48_000, 0);
        for flag in 1..=3 {
            assert_eq!(call("sceAudioOutSetVolume", [handle, flag, 0, 0, 0, 0]), 0);
        }
        assert_eq!(call("sceAudioOutClose", [handle, 0, 0, 0, 0, 0]), 0);
    }

    /// A closed handle does not come back, so a stale one cannot reach another port.
    #[test]
    fn a_closed_port_stays_closed() {
        let handle = open(512, 48_000, 0);
        assert_eq!(call("sceAudioOutClose", [handle, 0, 0, 0, 0, 0]), 0);
        assert_ne!(
            call("sceAudioOutClose", [handle, 0, 0, 0, 0, 0]),
            0,
            "closing twice is not a close"
        );
        assert_ne!(
            open(512, 48_000, 0),
            handle,
            "and the number is not reissued"
        );
    }
}
