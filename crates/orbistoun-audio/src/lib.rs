//! Audio output HLE - libSceAudioOut.
//!
//! Audio is the subsystem most often stubbed to silence and left there, which is
//! a mistake worth naming: guests frequently block on audio-buffer completion, so
//! a stub that never signals a drained buffer hangs the title with no audio
//! symptom to point at it.
//!
//! # Status
//!
//! **The output ports work.** They were declarations only until obSCEne measured the contract
//! end to end - which formats open, what a refusal answers, what the port state holds, and that
//! output blocks for as long as the samples take. That last one is what made the rest safe to
//! write: the drain can be modelled from the sample rate rather than from a device (D672).
//!
//! `ajm`, `audio3d`, `audio_in` and `audio_out2` are still declarations. Arities are provisional
//! except where a measurement fixed one.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError, GuestFn, HandleAllocator};
use orbistoun_hle::guest_module;

pub mod ajm;
pub mod audio3d;
pub mod audio_in;
pub mod audio_out2;

/// The high half libSceAudioOut numbers its errors from, rather than the kernel's `0x8002_0000`.
///
/// **Measured.** obSCEne's `090-audio/close-rejects-bad-handle` answered `0x80260003` on hardware -
/// the audio subsystem's own `NO_SUCH`. A guest that checks for `SCE_AUDIO_OUT_ERROR_*` never matches
/// the kernel base or a `0x7fff…` placeholder, so the base belongs here where the shim knows it, the
/// same way [`orbistoun_input`]'s pad shim carries its own (D438).
const AUDIO_ERROR_BASE: u32 = 0x8026_0000;

guest_module! {
    "libSceAudioOut" {
        "sceAudioOutInit" => 0,
        "sceAudioOutOpen" => 6,
        "sceAudioOutClose" => 1,
        "sceAudioOutOutput" => 2,
        "sceAudioOutSetVolume" => 3,
        // Imported by obSCEne, which is a real module asking for it by name - the same
        // provenance every other name here has. Two arguments: the handle and the buffer the
        // state is written into.
        "sceAudioOutGetPortState" => 2,
    }
}

/// `sceAudioOutInit()` - initialise the audio-output library.
///
/// Initialising is the precondition a title checks before it opens a port, and left
/// unimplemented it fell to the placeholder - a non-zero code `090-audio/initialise` reads as a
/// failed init. The library is idempotent-init on hardware, so repeating it is also success.
///
/// This used to say the port calls behind it "then fail honestly". They do not fail any more
/// (D672).
fn audio_out_init(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    0
}

/// `sceAudioOutClose(handle)` - close an audio-output port.
///
/// **It closes a port now.** There was no `Open`, so every handle named nothing and this always
/// answered the audio subsystem's `NO_SUCH` - which was the honest answer to a call about a port
/// that could not exist, and is the wrong one now that ports do.
///
/// A handle nothing opened still gets `0x80260003`, which is what obSCEne measured for exactly
/// that (`090-audio/close-rejects-bad-handle`), and the number is not handed back to the
/// allocator: reuse is what turns a stale handle into a silent write to another port.
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
/// **Measured.** `090-audio/open-shapes` asked for 44.1 kHz at four chunk sizes and the console
/// answered `0x8026_0008` to every one - the audio subsystem's own base, code 8. What the 8
/// means is not established; that this is the refusal is.
pub(crate) const UNSUPPORTED_FORMAT: u32 = 0x8026_0008;

/// The one sample rate a port opens at.
///
/// **Measured, and the split is on this alone.** `0xbb80` was accepted at chunk `0x100`, `0x200`,
/// `0x400` and `0x800`; `0xac44` was refused at all four. Chunk size did not decide any measured
/// case, so nothing here decides on it either - a shim that also refused an unmeasured chunk
/// would be inventing a rule the data does not contain (D672).
const SERVED_FREQUENCY: u64 = 48_000;

/// How many bytes `sceAudioOutGetPortState` writes.
///
/// **Measured**: `090-audio/format-selector` reports `extent 16` for all three selectors.
pub(crate) const PORT_STATE_BYTES: usize = 16;

/// The port-state image a console wrote, for a port of this many channels.
///
/// # Transcribed, not modelled
///
/// The three selectors were measured and differ in exactly one byte:
///
/// ```text
/// selector 0   81 00 01 c7 ff ff 05 00  00 00 00 00 00 00 00 00
/// selector 1   81 00 02 c7 ff ff 05 00  00 00 00 00 00 00 00 00
/// selector 2   81 00 02 c7 ff ff 05 00  00 00 00 00 00 00 00 00
/// ```
///
/// Offset two is the channel count - obSCEne reads it as one, which is what lets
/// `090-audio/format-selector` settle whether selector 0 is mono objectively rather than from a
/// header. Everything else is the same bytes for every port measured, so it is written back the
/// same way, and what `0x81`, `0xc7`, `0xffff` and `0x05` *mean* is not claimed here.
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
/// **Measured**: selector 0 answers one, selectors 1 and 2 answer two. A selector nothing
/// measured is treated as the stereo pair rather than refused, because refusing is the claim
/// the measurement does not support - three were tried and three opened.
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
/// Per-subsystem, for the reason the pad's is: a port handle and a pad handle sharing a number
/// space would hide the bug where a guest passes one to the other.
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
/// # Why this could be written at last
///
/// It was held back with the rest of the port calls: there is no audio backend, and a port that
/// opens but never drains is the failure this crate's own header names - a guest blocking on
/// buffer completion hangs with no audio symptom to point at it (D171).
///
/// obSCEne measured the whole contract since. Which formats open (`090-audio/open-shapes`), what
/// a refusal answers, what the port state contains (`090-audio/format-selector`), and - the part
/// that unblocks the rest - **that output blocks**, timed at eight 512-frame buffers
/// (`090-audio/blocking`). So the drain can be modelled from the sample rate rather than from a
/// device, which is what makes opening honest now (D672).
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
/// **Takes as long as the samples take to play**, which is the whole of the model. There is no
/// device; what a guest needs from this call is the *pacing*, and a shim that returned at once
/// would let an audio thread spin as fast as the processor allows - D171's warning with the
/// sign flipped.
///
/// Timed against hardware: `090-audio/blocking` sends eight 512-frame buffers at 48 kHz and
/// treats anything under 40 ms as not having blocked. Eight of those is 85 ms of audio, and
/// this sleeps for each one, so it lands above the floor rather than near it.
///
/// **The console returns sooner than real time** - 57 ms measured against 85 ms of audio - which
/// is a queue a few buffers deep absorbing the first calls. How deep is not measured, and
/// picking a number would be inventing the one thing this could get wrong, so there is no queue
/// here and every call waits.
///
/// The return is the frame count, and that is **assumed**: obSCEne discards it, so nothing has
/// seen what a console answers. A guest testing `< 0` reads it as success either way.
fn audio_out_output(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(port) = port(args[0]) else {
        return no_such_port();
    };
    if port.frequency != 0 && port.frames != 0 {
        let playing = std::time::Duration::from_micros(
            port.frames.saturating_mul(1_000_000) / port.frequency,
        );
        std::thread::sleep(playing);
        // **And the clock has to agree that it happened**, which is the whole of what a guest
        // timing this can see. The default clock is logical - it advances a microsecond per
        // read so a run repeats (D582) - so eight buffers that really slept 85 ms read as 8 µs
        // to the guest, and `090-audio/blocking` reports the port did not block. Which it did.
        //
        // The same wiring hazard `usleep` was fixed for and the same fix: every call that makes
        // time pass says so. Found by measuring, not by reading - the sleep was there and the
        // check still called it instant.
        orbistoun_hle::clocks::advance(playing.as_nanos());
    }
    port.frames
}

/// `sceAudioOutGetPortState(handle, state)` - what a port is set to.
///
/// Writes the sixteen bytes a console wrote, with the channel count the selector asked for. See
/// [`port_state`] for why it is bytes rather than a structure.
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
    // SAFETY: a guest-supplied destination under the identity mapping (D014), written for
    // exactly the extent the console was measured writing. The source is a local array of that
    // length and cannot overlap guest memory.
    unsafe { std::ptr::copy_nonoverlapping(state.as_ptr(), into as *mut u8, PORT_STATE_BYTES) };
    0
}

/// `sceAudioOutSetVolume(handle, flags, levels)` - accepted, and there is nothing to set.
///
/// **Measured**: `090-audio/volume-flag` sets flags 1, 2 and 3 and the console answers `0x0` to
/// each. Nothing here has a volume, and refusing would stop a title that sets one while opening
/// a port - the same reasoning the pad's vibration takes, and the same honesty: the call
/// succeeds and nothing gets quieter.
fn audio_out_set_volume(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if port(args[0]).is_none() {
        return no_such_port();
    }
    0
}

/// The audio implementations this crate provides.
///
/// **The port calls are here now.** They were held back because there is no backend and a port
/// that opens without draining hangs a title (D171) - and the drain turned out to be measurable
/// without one: output blocks, and for how long is arithmetic on the sample rate (D672).
///
/// Still absent: everything in `ajm`, `audio3d`, `audio_in` and `audio_out2`. Nothing has
/// measured those.
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

    /// **48 kHz opens and 44.1 kHz does not, at every chunk size measured.**
    ///
    /// obSCEne's `090-audio/open-shapes` tried two frequencies against four chunk sizes and the
    /// console split them on the frequency alone: `0xbb80` accepted at `0x100`, `0x200`, `0x400`
    /// and `0x800`, `0xac44` refused at all four with `0x8026_0008`.
    ///
    /// So the rule is the frequency, and this says nothing about chunk size - because the
    /// measurement says nothing about it (D672).
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

    /// **The port state is sixteen bytes, and the selector decides the channel count.**
    ///
    /// Measured across all three selectors: `81 00 01 c7 ff ff 05 00` then zero for selector 0,
    /// and the same with `02` at offset two for selectors 1 and 2. The channel count is the only
    /// byte that moves, which is what makes `090-audio/format-selector` able to say whether
    /// selector 0 is mono or stereo objectively.
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

    /// **Output takes as long as the samples take to play.**
    ///
    /// `090-audio/blocking` sends eight 512-frame buffers at 48 kHz and times them: under 40 ms
    /// it reports the port did not block, which is amber, because the interface promises it
    /// does. Eight of 512 at 48 kHz is 85 ms of audio.
    ///
    /// Asserted as a floor rather than a window: a loaded machine can take longer and that is
    /// not a fault, while returning early is exactly the defect D171 named - a guest waiting on
    /// buffer completion spins as fast as the CPU allows.
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
    ///
    /// The negative case. `090-audio/close-rejects-bad-handle` measured `0x80260003`, and a shim
    /// that accepted any handle would pass every test above while telling a guest a port it
    /// never opened is playing.
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

    /// Setting the volume on an open port is accepted.
    ///
    /// Measured: `090-audio/volume-flag` sets flags 1, 2 and 3 and the console answers `0x0` to
    /// each. Nothing here has a volume to set, and saying so by refusing would stop a title that
    /// sets one while opening.
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
