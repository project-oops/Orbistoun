//! Audio output HLE - `libSceAudioOut`.
//!
//! Guests often block on audio-buffer completion, so a port that never drains hangs the
//! title with no audio symptom. The output ports follow the contract obSCEne's `090-audio`
//! checks measure on hardware: with a host [`Sink`] installed a port's samples play on a device
//! and the device paces it (D819); with none, a port paces itself by its sample rate.
//! `audio_out2` serves its contexts and ports, and `ajm` its context set-up (initialise,
//! register a codec, finalise) and instances; `ajm`'s batches, `audio3d` and `audio_in` are
//! declarations only.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError, GuestFn, HandleAllocator};
use orbistoun_hle::guest_module;

pub mod ajm;
pub mod audio3d;
pub mod audio_in;
pub mod audio_out2;
pub mod voice;

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
    if !removed {
        return no_such_port();
    }
    if let Some(sink) = SINK.get() {
        (sink.close)(raw);
    }
    0
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

/// The format selectors whose samples are 16-bit signed little-endian, interleaved.
///
/// Assumed from oops-sdk, whose `audio.c` opens its ports with selector 1 as "16-bit signed
/// interleaved stereo" and plays on hardware; selector 0 is taken as that format's one-channel
/// form, as the measured channel counts pair them. Selector 2's format is unmeasured, so its
/// samples reach no sink.
const S16_SELECTORS: [u64; 2] = [0, 1];

/// Where an output port's samples go once the guest hands them over: a host device, at guest
/// semantics - a port, its channel count, and samples at the served rate, `SERVED_FREQUENCY`.
#[derive(Debug, Clone, Copy)]
pub struct Sink {
    /// Plays one chunk of interleaved samples on a port. Answers how long the port should wait
    /// before it returns, so a guest is paced by the device rather than by a sleep.
    pub play: fn(port: u32, channels: u8, samples: &[i16]) -> std::time::Duration,
    /// Forgets a closed port and anything it still had queued.
    pub close: fn(port: u32),
}

static SINK: std::sync::OnceLock<Sink> = std::sync::OnceLock::new();

/// Installs where output ports' samples go. First install wins; with none, output is paced by
/// the sample rate and played nowhere.
pub fn install_sink(sink: Sink) {
    let _ = SINK.set(sink);
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
/// With a [`Sink`] installed, the samples go to it and the call waits as long as it says. With
/// none, the call sleeps for as long as the samples take to play: what a guest needs is the
/// pacing, and returning at once would let an audio thread spin. `090-audio/blocking` treats
/// eight 512-frame buffers at 48 kHz (85 ms of audio) returning in under 40 ms as not blocking.
/// The hardware returns sooner than real time, through a queue of unmeasured depth; a sink
/// keeps a queue of its own, the sleep keeps none. A null buffer plays nothing and is paced
/// alike. The frame-count return is assumed.
fn audio_out_output(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Ok(raw) = u32::try_from(args[0]) else {
        return no_such_port();
    };
    let Some(port) = port(args[0]) else {
        return no_such_port();
    };
    if port.frequency != 0 && port.frames != 0 {
        let playing = std::time::Duration::from_micros(
            port.frames.saturating_mul(1_000_000) / port.frequency,
        );
        let wait = SINK
            .get()
            .and_then(|sink| {
                let samples = samples(args[1], port)?;
                Some((sink.play)(raw, channels_for(port.selector), &samples))
            })
            .unwrap_or(playing);
        std::thread::sleep(wait);
        // The guest clock is logical by default (D582), so the sleep must also advance it, or a
        // guest timing the output sees no time pass.
        orbistoun_hle::clocks::advance(playing.as_nanos());
    }
    port.frames
}

/// The chunk of samples a port's output names at `at`, or `None` for a null buffer or a format
/// whose samples are not known to be 16-bit ([`S16_SELECTORS`]).
fn samples(at: u64, port: Port) -> Option<Vec<i16>> {
    if at == 0 || !S16_SELECTORS.contains(&port.selector) {
        return None;
    }
    let count = usize::try_from(port.frames)
        .ok()?
        .checked_mul(usize::from(channels_for(port.selector)))?;
    let mut bytes = vec![0_u8; count.checked_mul(2)?];
    // SAFETY: the output call's contract: a port's buffer holds one chunk, its frames times its
    // channels of 16-bit samples, readable for the call.
    if !unsafe { orbistoun_mem::guest::read_into(at, &mut bytes) } {
        return None;
    }
    Some(
        bytes
            .chunks_exact(2)
            .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
            .collect(),
    )
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
/// The `ajm` module contributes its context set-up and instances, and `audio_out2` its user create
/// and destroy;
/// `audio3d` and `audio_in` none.
#[must_use]
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("sceAudioOutInit", audio_out_init),
        ("sceAudioOutOpen", audio_out_open),
        ("sceAudioOutClose", audio_out_close),
        ("sceAudioOutOutput", audio_out_output),
        ("sceAudioOutGetPortState", audio_out_get_port_state),
        ("sceAudioOutSetVolume", audio_out_set_volume),
        ("sceAjmInitialize", ajm::ajm_initialize),
        ("sceAjmModuleRegister", ajm::ajm_module_register),
        ("sceAjmFinalize", ajm::ajm_finalize),
        ("sceAjmInstanceCreate", ajm::ajm_instance_create),
        ("sceAjmInstanceDestroy", ajm::ajm_instance_destroy),
        ("sceAjmBatchInitialize", ajm::ajm_batch_initialize),
        ("sceAjmBatchJobDecode", ajm::ajm_batch_job_decode),
        ("sceAjmBatchStart", ajm::ajm_batch_start),
        ("sceAjmBatchWait", ajm::ajm_batch_wait),
        ("sceVoiceInit", voice::voice_init),
        ("sceVoiceSetThreadsParams", voice::voice_set_threads_params),
        ("sceAudioOut2Initialize", audio_out2::initialize),
        ("sceAudioOut2UserCreate", audio_out2::user_create),
        ("sceAudioOut2UserDestroy", audio_out2::user_destroy),
        (
            "sceAudioOut2ContextResetParam",
            audio_out2::context_reset_param,
        ),
        (
            "sceAudioOut2ContextQueryMemory",
            audio_out2::context_query_memory,
        ),
        ("sceAudioOut2ContextCreate", audio_out2::context_create),
        ("sceAudioOut2ContextAdvance", audio_out2::context_advance),
        ("sceAudioOut2ContextPush", audio_out2::context_push),
        (
            "sceAudioOut2ContextGetQueueLevel",
            audio_out2::context_get_queue_level,
        ),
        ("sceAudioOut2PortCreate", audio_out2::port_create),
        ("sceAudioOut2PortDestroy", audio_out2::port_destroy),
        (
            "sceAudioOut2PortSetAttributes",
            audio_out2::port_set_attributes,
        ),
        ("sceAudioOut2PortGetState", audio_out2::port_get_state),
        ("sceAudioOut2GetSpeakerInfo", audio_out2::get_speaker_info),
        (
            "sceAudioOut2GetSpeakerArrayMemorySize",
            audio_out2::get_speaker_array_memory_size,
        ),
    ]
}

#[cfg(test)]
mod tests {
    use orbistoun_core::GUEST_ARG_REGISTERS;

    /// `sceAudioOut2UserCreate` writes an 8-byte handle for the signed-in user and for `0xff`, and
    /// `sceAudioOut2UserDestroy` answers 0 for a handle it made and `0x80268010` for 0 (REQ-ao04,
    /// `20261010-231900-eboot.obs.log` 30-56, a fresh process; REQ-cn10 arm 2 for 0).
    #[test]
    fn an_audio_out2_user_is_created_for_the_signed_in_user_as_measured() {
        let user = u64::from(orbistoun_systemservice::signed_in_user());
        let mut out = [0xa5_u8; 0x40];
        let at = out.as_mut_ptr() as u64;
        assert_eq!(call("sceAudioOut2UserCreate", [user, at, 0, 0, 0, 0]), 0);
        let handle = u64::from_le_bytes(out[..8].try_into().expect("eight bytes"));
        assert_ne!(handle, 0, "a handle");
        assert!(
            out[8..].iter().all(|&b| b == 0xa5),
            "exactly 8 bytes written"
        );
        assert_eq!(call("sceAudioOut2UserDestroy", [handle, 0, 0, 0, 0, 0]), 0);
        assert_eq!(
            call("sceAudioOut2UserDestroy", [0, 0, 0, 0, 0, 0]),
            0x8026_8010
        );
        let mut primary = [0xa5_u8; 8];
        let at = primary.as_mut_ptr() as u64;
        assert_eq!(call("sceAudioOut2UserCreate", [0xff, at, 0, 0, 0, 0]), 0);
        assert_ne!(primary, [0xa5; 8], "0xff names the primary user");
    }

    /// The context set-up PPSA04263 makes, as a fresh process answered it (REQ-ao04,
    /// `20261010-231900-eboot.obs.log` 30-56): the reset parameters, the memory the title's
    /// parameters need, and a context created in it.
    #[test]
    fn an_audio_out2_context_is_set_up_as_measured() {
        let mut param = [0xa5_u8; 0x40];
        let at = param.as_mut_ptr() as u64;
        assert_eq!(
            call("sceAudioOut2ContextResetParam", [at, 0, 0, 0, 0, 0]),
            0
        );
        let word = |b: &[u8], i: usize| u32::from_le_bytes(b[i..i + 4].try_into().expect("four"));
        assert_eq!(
            (word(&param, 0), word(&param, 4), word(&param, 8)),
            (8, 0, 0)
        );
        assert_eq!(
            (word(&param, 0xc), word(&param, 0x10), word(&param, 0x14)),
            (1, 0x100, 0)
        );
        assert!(
            param[0x18..].iter().all(|&b| b == 0xa5),
            "the title's block is 0x18 bytes"
        );

        for (i, value) in [4_u32, 0x48, 0, 2, 0x100, 1].into_iter().enumerate() {
            param[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        let mut size = [0xa5_u8; 8];
        let size_at = size.as_mut_ptr() as u64;
        assert_eq!(
            call("sceAudioOut2ContextQueryMemory", [at, size_at, 0, 0, 0, 0]),
            0
        );
        assert_eq!(u64::from_le_bytes(size), 0x45698);

        let mut context = [0xa5_u8; 8];
        let context_at = context.as_mut_ptr() as u64;
        assert_eq!(
            call(
                "sceAudioOut2ContextCreate",
                [at, 0x1000, 0x45698, context_at, 0, 0]
            ),
            0
        );
        assert_eq!(context, [0; 8], "the console's first context is 0");

        // Its ports (REQ-ao05, `report-1791713884.txt` 54-99): 2 channels then 8, for a user
        // `UserCreate(0xff)` gave, numbered from 0.
        let mut user = [0_u8; 8];
        let user_at = user.as_mut_ptr() as u64;
        assert_eq!(
            call("sceAudioOut2UserCreate", [0xff, user_at, 0, 0, 0, 0]),
            0
        );
        let mut ports = Vec::new();
        for channels in [2_u32, 8] {
            let mut block = [0_u8; 0x40];
            block[4..8].copy_from_slice(&(channels << 8).to_le_bytes());
            block[8..12].copy_from_slice(&48_000_u32.to_le_bytes());
            block[12..16].copy_from_slice(&2_u32.to_le_bytes());
            block[16..24].copy_from_slice(&user);
            let mut port = [0xa5_u8; 8];
            let args = [0, block.as_ptr() as u64, port.as_mut_ptr() as u64, 0, 0, 0];
            assert_eq!(
                call("sceAudioOut2PortCreate", args),
                0,
                "{channels} channels"
            );
            ports.push(u64::from_le_bytes(port));
        }
        assert_eq!(ports, [0, 1]);
        ports_by_type_and_state(user);
        mono_ports(user);
        // Six channels, which the title rounds up to eight before it calls, is unmeasured.
        let mut block = [0_u8; 0x40];
        block[4..8].copy_from_slice(&(6_u32 << 8).to_le_bytes());
        block[8..12].copy_from_slice(&48_000_u32.to_le_bytes());
        block[12..16].copy_from_slice(&2_u32.to_le_bytes());
        block[16..24].copy_from_slice(&user);
        let mut port = [0xa5_u8; 8];
        let unmeasured = [0, block.as_ptr() as u64, port.as_mut_ptr() as u64, 0, 0, 0];
        assert_ne!(call("sceAudioOut2PortCreate", unmeasured), 0);
        assert_eq!(port, [0xa5; 8], "an unmeasured call writes nothing");
    }

    /// PPSA04263's pool of mono ports as REQ-ap01 measured it (`report-1791818993.txt` 440-573):
    /// thirty-six numbered `0x80000000` on, the block untouched; both attribute shapes refused
    /// `0x80268001`, on a made port and on `-1`; a port destroyed once answers 0, then `0x80268009`,
    /// and `-1` `0x80268001`.
    fn mono_ports(user: [u8; 8]) {
        let mut block = [0_u8; 0x38];
        for (i, word) in [0x100_u32, 0x100, 48_000, 0].into_iter().enumerate() {
            block[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        block[16..24].copy_from_slice(&user);
        let before = block;
        let mut numbers = Vec::new();
        for _ in 0..36 {
            let mut port = [0xcc_u8; 8];
            let args = [0, block.as_ptr() as u64, port.as_mut_ptr() as u64, 0, 0, 0];
            assert_eq!(call("sceAudioOut2PortCreate", args), 0);
            numbers.push(u64::from_le_bytes(port));
        }
        assert_eq!(numbers.first(), Some(&0x8000_0000));
        assert_eq!(numbers.last(), Some(&0x8000_0023));
        assert_eq!(block, before, "the block is untouched");

        let record = |id: u32, value: u64, size: u64| {
            let mut r = [0_u8; 0x18];
            r[0..4].copy_from_slice(&id.to_le_bytes());
            r[8..16].copy_from_slice(&value.to_le_bytes());
            r[16..24].copy_from_slice(&size.to_le_bytes());
            r
        };
        let (v, a, b) = ([0_u8; 8], [0_u8; 4], [0_u8; 4]);
        let one = record(0, v.as_ptr() as u64, 8);
        let mut three = [0_u8; 0x48];
        three[..0x18].copy_from_slice(&one);
        three[0x18..0x30].copy_from_slice(&record(8, a.as_ptr() as u64, 4));
        three[0x30..].copy_from_slice(&record(4, b.as_ptr() as u64, 4));
        let first = numbers[0];
        let set = |port: u64, records: &[u8], count: u64| {
            call(
                "sceAudioOut2PortSetAttributes",
                [port, records.as_ptr() as u64, count, 0, 0, 0],
            )
        };
        assert_eq!(set(first, &one, 1), 0x8026_8001);
        assert_eq!(set(first, &three, 3), 0x8026_8001);
        assert_eq!(set(u64::MAX, &one, 1), 0x8026_8001);
        assert_eq!((v, a, b), ([0; 8], [0; 4], [0; 4]), "nothing written");

        let destroy = |port: u64| call("sceAudioOut2PortDestroy", [port, 0, 0, 0, 0, 0]);
        assert_eq!(destroy(first), 0);
        assert_eq!(destroy(first), 0x8026_8009);
        assert_eq!(destroy(u64::MAX), 0x8026_8001);
    }

    /// The rest of the port sequence REQ-ao07 measured, after the two ports of
    /// [`an_audio_out2_context_is_set_up_as_measured`]: the port types, and a made port's state.
    fn ports_by_type_and_state(user: [u8; 8]) {
        // The port types REQ-ao07 measured after those (`report-1791758178.txt` 54-82): eight
        // channels take types 1, 2 and 3 and the numbering goes on; type 4 is refused 0x80268001,
        // type 0 and two channels of type 3 0x80268002, each writing nothing.
        let create = |channels: u32, kind: u32| {
            let mut block = [0_u8; 0x40];
            block[4..8].copy_from_slice(&(channels << 8).to_le_bytes());
            block[8..12].copy_from_slice(&48_000_u32.to_le_bytes());
            block[12..16].copy_from_slice(&kind.to_le_bytes());
            block[16..24].copy_from_slice(&user);
            let mut port = [0xa5_u8; 8];
            let args = [0, block.as_ptr() as u64, port.as_mut_ptr() as u64, 0, 0, 0];
            (call("sceAudioOut2PortCreate", args), port)
        };
        assert_eq!(create(8, 3), (0, 2_u64.to_le_bytes()), "type 3");
        assert_eq!(create(8, 1), (0, 3_u64.to_le_bytes()), "type 1");
        assert_eq!(create(8, 4), (0x8026_8001, [0xa5; 8]), "type 4");
        assert_eq!(create(8, 0), (0x8026_8002, [0xa5; 8]), "type 0");
        assert_eq!(
            create(2, 3),
            (0x8026_8002, [0xa5; 8]),
            "two channels of type 3"
        );
        // A made port's state is the measured record, zero to 0x40 (D799); one never made is
        // unmeasured and writes nothing.
        let mut state = [0xa5_u8; 0x48];
        let state_at = state.as_mut_ptr() as u64;
        assert_eq!(
            call("sceAudioOut2PortGetState", [1, state_at, 0, 0, 0, 0]),
            0
        );
        assert_eq!(state[..8], [0x81, 0, 2, 0, 0xff, 0xff, 5, 0]);
        assert!(state[8..0x40].iter().all(|&b| b == 0), "zero to 0x40");
        assert!(state[0x40..].iter().all(|&b| b == 0xa5), "nothing past it");
        let mut untouched = [0xa5_u8; 0x40];
        let untouched_at = untouched.as_mut_ptr() as u64;
        assert_ne!(
            call("sceAudioOut2PortGetState", [99, untouched_at, 0, 0, 0, 0]),
            0
        );
        assert_eq!(untouched, [0xa5; 0x40], "an unmade port writes nothing");
    }

    /// `sceAudioOut2GetSpeakerArrayMemorySize` answers the measured byte counts (REQ-ao06,
    /// `report-1791727748.txt` 92-124), and the placeholder for an unmeasured shape.
    #[test]
    fn the_speaker_array_size_is_as_measured() {
        let size = |a, b, c| call("sceAudioOut2GetSpeakerArrayMemorySize", [a, b, c, 0, 0, 0]);
        assert_eq!(size(8, 0, 1), 0xae00, "PPSA04263's shape");
        assert_eq!(size(2, 0, 1), 0xad40);
        assert_eq!(size(8, 0, 0), 0x2e0);
        assert_eq!(size(4, 0, 1), 0xf7ff_0001, "unmeasured");
    }

    /// `sceAudioOut2Initialize` answers 0 the first time in a process and `0x80268004` every time
    /// after, writing nothing (REQ-ao03, `20261010-125000-eboot.obs.log` 30-42; REQ-ao02's rows,
    /// taken after a first call, answered `0x80268004` throughout).
    #[test]
    fn audio_out2_initializes_once_per_process() {
        let mut buffer = [0xa5_u8; 0x40];
        let at = buffer.as_mut_ptr() as u64;
        assert_eq!(call("sceAudioOut2Initialize", [0; 6]), 0);
        assert_eq!(call("sceAudioOut2Initialize", [0; 6]), 0x8026_8004);
        assert_eq!(
            call("sceAudioOut2Initialize", [at, 0, 0, 0, 0, 0]),
            0x8026_8004
        );
        assert!(buffer.iter().all(|&b| b == 0xa5), "nothing written");
    }

    #[test]
    fn an_audio_out2_user_destroy_answers_as_measured() {
        assert_eq!(
            call("sceAudioOut2UserDestroy", [0x8026_8001, 0, 0, 0, 0, 0]),
            0x8026_8010
        );
        assert_eq!(
            call("sceAudioOut2UserDestroy", [0, 0, 0, 0, 0, 0]),
            0x8026_8010
        );
    }

    /// `sceAjmInitialize(0, &ctx)` answers 0 and writes a 32-bit context identifier, `0x15` and then
    /// the next, over a slot filled `0xff` whose other bytes it leaves; a non-zero first argument or
    /// a null slot answers `0x80930005` (REQ-cn10 arm 1, `20261009-151440-eboot.obs.log`; the
    /// census's pattern and zero arguments).
    #[test]
    fn an_ajm_context_is_numbered_as_measured() {
        let mut ctx = [0xff_u8; 8];
        let at = ctx.as_mut_ptr() as u64;
        assert_eq!(call("sceAjmInitialize", [0, at, 0, 0, 0, 0]), 0);
        assert_eq!(ctx, [0x15, 0, 0, 0, 0xff, 0xff, 0xff, 0xff]);
        assert_eq!(call("sceAjmInitialize", [0, at, 0, 0, 0, 0]), 0);
        assert_eq!(ctx[..4], [0x16, 0, 0, 0]);
        assert_eq!(call("sceAjmInitialize", [1, at, 0, 0, 0, 0]), 0x8093_0005);
        assert_eq!(call("sceAjmInitialize", [0, 0, 0, 0, 0, 0]), 0x8093_0005);

        // On the first context, in this test because contexts are numbered for the whole process:
        // `sceAjmInstanceCreate(ctx, 0, 1, &out)` - PPSA04263's call - writes the first instance
        // identifier as four bytes over a slot filled `0xff`, the other four left, once codec 0 is
        // registered, and refuses before (REQ-ai01, `report-20261015-batch1.log`).
        let ctx = 0x15;
        let mut slot = [0xff_u8; 8];
        let at = slot.as_mut_ptr() as u64;
        assert_eq!(
            call("sceAjmInstanceCreate", [ctx, 0, 1, at, 0, 0]),
            0x8093_000a
        );
        assert_eq!(slot, [0xff; 8]);
        assert_eq!(call("sceAjmModuleRegister", [ctx, 0, 0, 0, 0, 0]), 0);
        assert_eq!(call("sceAjmInstanceCreate", [ctx, 0, 1, at, 0, 0]), 0);
        assert_eq!(slot, [1, 0, 0, 0, 0xff, 0xff, 0xff, 0xff]);
        assert_eq!(call("sceAjmInstanceDestroy", [ctx, 1, 0, 0, 0, 0]), 0);
        assert_eq!(
            call("sceAjmInstanceDestroy", [ctx, 1, 0, 0, 0, 0]),
            0x8093_0003
        );

        // PPSA04263's batch: `sceAjmBatchInitialize(buffer, 0x40, &batch)` over 0x80 bytes of
        // `0xcc` writes 32 bytes - zero but the size at +8 - and nothing past them, then one
        // `sceAjmBatchJobDecode(&batch, id, ...)` on it answers 0 (REQ-ai02).
        assert_eq!(call("sceAjmInstanceCreate", [ctx, 0, 1, at, 0, 0]), 0);
        let id = u64::from(slot[0]);
        let mut buffer = [0xcc_u8; 0x80];
        let mut batch = 0_u64;
        let (buffer_at, batch_at) = (
            buffer.as_mut_ptr() as u64,
            std::ptr::from_mut(&mut batch).expose_provenance() as u64,
        );
        assert_eq!(
            call(
                "sceAjmBatchInitialize",
                [buffer_at, 0x40, batch_at, 0, 0, 0]
            ),
            0
        );
        let mut header = [0_u8; 0x20];
        header[8] = 0x40;
        assert_eq!(buffer[..0x20], header);
        assert!(
            buffer[0x20..].iter().all(|&b| b == 0xcc),
            "32 bytes and no more"
        );
        assert_eq!(batch, buffer_at);
        assert_eq!(
            call("sceAjmBatchJobDecode", [batch_at, id, 0, 960, 0, 9216]),
            0
        );
    }

    /// PPSA21564's context block, as the console answered it (REQ-ac01, `report-1791826929.txt`
    /// 189-212): it needs 0x78e9c bytes, and a port of 8 channels, of type 0 or 2, on a handle no
    /// context was made as is refused.
    #[test]
    fn ppsa21564s_audio_out2_block_is_answered_as_measured() {
        let mut param = [0_u8; 0x40];
        for (i, value) in [0x14_u32, 0x200, 0, 1, 0x200, 1].into_iter().enumerate() {
            param[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        let at = param.as_ptr() as u64;
        let mut size = [0xcc_u8; 8];
        let size_at = size.as_mut_ptr() as u64;
        assert_eq!(
            call("sceAudioOut2ContextQueryMemory", [at, size_at, 0, 0, 0, 0]),
            0
        );
        assert_eq!(u64::from_le_bytes(size), 0x78e9c);
        for port_type in [0_u32, 2] {
            let mut block = [0_u8; 0x40];
            block[4..8].copy_from_slice(&(8_u32 << 8).to_le_bytes());
            block[8..12].copy_from_slice(&48_000_u32.to_le_bytes());
            block[12..16].copy_from_slice(&port_type.to_le_bytes());
            let mut port = [0xcc_u8; 8];
            let args = [
                u64::from_le_bytes([0xcc; 8]),
                block.as_ptr() as u64,
                port.as_mut_ptr() as u64,
                0,
                0,
                0,
            ];
            assert_eq!(
                call("sceAudioOut2PortCreate", args),
                0x8026_8001,
                "type {port_type}"
            );
            assert_eq!(port, [0xcc; 8]);
        }
    }

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

    /// What the test sink was handed: `(port, channels, samples)` per chunk, and closed ports.
    static PLAYED: std::sync::Mutex<Vec<(u32, u8, Vec<i16>)>> = std::sync::Mutex::new(Vec::new());
    static CLOSED: std::sync::Mutex<Vec<u32>> = std::sync::Mutex::new(Vec::new());

    /// Records a chunk and asks for the wait the sample rate gives, so the pacing tests hold
    /// whichever test installs it first.
    fn test_play(port: u32, channels: u8, samples: &[i16]) -> std::time::Duration {
        PLAYED
            .lock()
            .expect("unpoisoned")
            .push((port, channels, samples.to_vec()));
        let frames = samples.len() as u64 / u64::from(channels);
        std::time::Duration::from_micros(frames * 1_000_000 / super::SERVED_FREQUENCY)
    }

    fn test_close(port: u32) {
        CLOSED.lock().expect("unpoisoned").push(port);
    }

    /// A port's samples reach the sink as the guest wrote them, with the selector's channel count;
    /// a format not known to be 16-bit and a null buffer reach it with nothing, and a close is
    /// passed on.
    #[test]
    fn output_hands_its_samples_to_the_sink() {
        super::install_sink(super::Sink {
            play: test_play,
            close: test_close,
        });
        let stereo = open(4, 48_000, 1);
        let chunk: [i16; 8] = [1, -1, 2, -2, 3, -3, i16::MAX, i16::MIN];
        let at = chunk.as_ptr().expose_provenance() as u64;
        assert_eq!(call("sceAudioOutOutput", [stereo, at, 0, 0, 0, 0]), 4);
        assert_eq!(call("sceAudioOutOutput", [stereo, 0, 0, 0, 0, 0]), 4);
        let unmeasured = open(4, 48_000, 2);
        call("sceAudioOutOutput", [unmeasured, at, 0, 0, 0, 0]);

        let played = PLAYED.lock().expect("unpoisoned").clone();
        let of = |handle: u64| {
            played
                .iter()
                .filter(|(port, ..)| u64::from(*port) == handle)
                .cloned()
                .collect::<Vec<_>>()
        };
        assert_eq!(of(stereo), [(stereo as u32, 2, chunk.to_vec())]);
        assert!(
            of(unmeasured).is_empty(),
            "selector 2's format is unmeasured"
        );

        assert_eq!(call("sceAudioOutClose", [stereo, 0, 0, 0, 0, 0]), 0);
        assert!(
            CLOSED
                .lock()
                .expect("unpoisoned")
                .contains(&(stereo as u32))
        );
        assert_eq!(call("sceAudioOutClose", [unmeasured, 0, 0, 0, 0, 0]), 0);
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

    /// The speaker information is the measured record: byte 0 zero and three at +4, in the first
    /// 16 bytes, whatever the flag; nothing past them is written (REQ-so01).
    #[test]
    fn the_speaker_information_is_the_measured_record() {
        for flag in [0, 1] {
            let mut info = [0xcc_u8; 0x20];
            assert_eq!(
                call(
                    "sceAudioOut2GetSpeakerInfo",
                    [info.as_mut_ptr() as u64, flag, 0, 0, 0, 0]
                ),
                0
            );
            assert_eq!(info[..16], [0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            assert_eq!(info[16..], [0xcc; 16], "the title's own fields after it");
        }
    }
}
