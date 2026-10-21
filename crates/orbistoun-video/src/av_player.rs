//! `libSceAvPlayer` - the bundled media player, which decodes and presents a stream for a title.
//!
//! Served as obSCEne measured it. The lifecycle with no source comes from
//! `000-boot/avplayer-lifecycle` (`report-20261014-batch2.log` 1427-1452, REQ-av01). A real
//! source, its streams, frames, events and allocations come from REQ-20261020T1135Z-vd01
//! (`report-20261020-vd01-census.log`, section `000-boot/vd01-avplayer`), which played a
//! synthetic 320x240 H.264 + AAC clip. The title's own blocks fix the slots the probe could not:
//! PPSA02664 fills its InitEx block at image+0xf8cb09..0xf8cee2 and its AddSourceEx block at
//! image+0xf8d1db..0xf8d238, and reads its frame info at image+0xf8ee9d..0xf8eec2.
//!
//! The decoding is not this module's: a source is opened through [`crate::media`], and the
//! pictures and sound it decodes are placed here in the console's layout, in memory the title's
//! own callbacks allocate, and paced by the guest's clock.
//!
//! Every arity is `6`, the trampoline's full capture, not a claim about how many arguments a
//! function takes: a wrong arity only degrades a trace, while a wrong name is unreachable.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError};
use orbistoun_hle::guest_module;
use orbistoun_mem::guest;

use crate::media::{self, Media, OpenError, Picture, Sound, StreamDesc, StreamKind};

guest_module! {
    "libSceAvPlayer" {
        "sceAvPlayerAddSource" => 6,
        "sceAvPlayerAddSourceEx" => 6,
        "sceAvPlayerChangeStream" => 6,
        "sceAvPlayerClose" => 6,
        "sceAvPlayerCurrentTime" => 6,
        "sceAvPlayerEnableStream" => 6,
        "sceAvPlayerGetAudioData" => 6,
        "sceAvPlayerGetStreamInfo" => 6,
        "sceAvPlayerGetStreamInfoEx" => 6,
        "sceAvPlayerGetVideoData" => 6,
        "sceAvPlayerGetVideoDataEx" => 6,
        "sceAvPlayerInit" => 6,
        "sceAvPlayerInitEx" => 6,
        "sceAvPlayerIsActive" => 6,
        "sceAvPlayerJumpToTime" => 6,
        "sceAvPlayerPause" => 6,
        "sceAvPlayerPostInit" => 6,
        "sceAvPlayerResume" => 6,
        "sceAvPlayerSetAvSyncMode" => 6,
        "sceAvPlayerSetLogCallback" => 6,
        "sceAvPlayerSetLooping" => 6,
        "sceAvPlayerSetTrickSpeed" => 6,
        "sceAvPlayerStart" => 6,
        "sceAvPlayerStop" => 6,
        "sceAvPlayerStreamCount" => 6,
    }
}

const OK: u64 = 0;

/// What the player answers an argument it refuses: an init block smaller than
/// [`INIT_BLOCK_BYTES`] or with no allocate callback (`InitEx-size-0x100`, `InitEx-null-alloc-cb`),
/// a stream id past the last (`stream-2|get-stream-info-rc`), and a handle it did not open
/// (`stop-invalid-handle-rc`).
const INVALID_ARGUMENT: u64 = 0x806a_0001;

/// What the player answers an operation its state does not allow: `Stop` with no source
/// (`stop-after-failed-postinit-rc`), a source that is not there (`add-source-missing`), a second
/// `Pause` or `Resume` (`player2-pause|rc2`, `player2-resume|rc2`), and `EnableStream` once
/// playing (`player2-enable-stream|rc-1`).
const OPERATION_FAILED: u64 = 0x806a_0002;

/// What the player answers a trick speed it does not play (`player2-trick-speed|rc-200`).
const NOT_SUPPORTED: u64 = 0x806a_0004;

/// The size word at +0 of the init block the measured `InitEx` accepted.
const INIT_BLOCK_BYTES: u64 = 0x230;

/// Where the init block keeps the word the memory callbacks receive first (the probe stored it at
/// +0x08 and its allocate callback received it as its first argument).
const OBJECT_AT: u64 = 0x08;

/// The allocate callback's slot: the one `InitEx` called, once (`InitEx-first-cb0`, `id 0`).
const ALLOCATE_AT: u64 = 0x10;

/// The deallocate callback's slot, next to it. `Close` made one callback (`Close-first|callbacks
/// 1`); which slot is not rowed, and this one is the pair to the allocator by position.
const DEALLOCATE_AT: u64 = 0x18;

/// The texture allocate callback's slot: the second slot vd01's rows name (`origin 0x1`), which
/// allocated the frame buffers, and the slot PPSA02664 puts its second allocator in.
const ALLOCATE_TEXTURE_AT: u64 = 0x20;

/// The texture deallocate callback's slot, freeing what [`ALLOCATE_TEXTURE_AT`] allocated
/// (`dealloc-call|origin|0x1`), where PPSA02664 puts its pair.
const DEALLOCATE_TEXTURE_AT: u64 = 0x28;

/// The event callback's slot (`ev-slot|0x60`).
const EVENT_CALLBACK_AT: u64 = 0x60;

/// The word the event callback receives first, beside it: PPSA02664 stores its object there.
/// vd01's probe left it zero, and the callback received zero.
const EVENT_OBJECT_AT: u64 = 0x58;

/// How many frame buffers the player decodes into. PPSA02664 asks for 6 here; the probe left it
/// zero and was given six (frames 0x2e000 apart in a 0x118000 block, the 30th back in the first).
const FRAMEBUFFERS_AT: u64 = 0x228;

/// The frame buffers when the init block asks for none, as the probe's was given.
const DEFAULT_FRAMEBUFFERS: u64 = 6;

/// The allocate callback's second argument, as `InitEx` passed it (`InitEx-first-cb0|a1|0x20`).
const ALLOCATE_SECOND: u64 = 0x20;

/// The allocate callback's third argument, as `InitEx` passed it (`InitEx-first-cb0|a2|0x5c0`,
/// and `InitEx-second-cb0` the same; `report-20261020-gt23-census.log` 4808, 4831): the size of
/// the player's block, the second argument being its alignment.
const ALLOCATE_THIRD: u64 = 0x5c0;

/// What adding a source allocates first, through the allocate callback, before the file is
/// opened, and keeps until `Close` (`alloc-call|a1|0x40`, `a2|0x1000000`, before
/// `add-source-real` and before `add-source-missing` alike).
const SOURCE_BUFFER_ALIGN: u64 = 0x40;
const SOURCE_BUFFER_BYTES: u64 = 0x100_0000;

/// The AddSourceEx block: the URI's pointer at +0, its length at +8 and the source type at +0x50,
/// as PPSA02664 builds it. Types 0 and 1 both opened a file (`add-source-type0|rc|0x0`,
/// `add-source-real|rc|0x0`); 1 is the one PPSA02664 passes for a file path.
const URI_AT: u64 = 0x00;
const URI_LENGTH_AT: u64 = 0x08;
const SOURCE_TYPE_AT: u64 = 0x50;
const FILE_SOURCE_TYPES: [u32; 2] = [0, 1];

/// Event ids the callback receives second: stopped, ready, playing and paused. Adding a source
/// answers 2, `Start` and `Resume` 3, `Pause` 4, and `Stop` and `Close` 1.
const EVENT_STOPPED: u64 = 1;
const EVENT_READY: u64 = 2;
const EVENT_PLAYING: u64 = 3;
const EVENT_PAUSED: u64 = 4;

/// The stream type word `GetStreamInfo` writes at +0: 1 for the clip's video, 2 for its audio.
const STREAM_VIDEO: u32 = 1;
const STREAM_AUDIO: u32 = 2;

/// The texture allocate callback's alignment for the frame buffers (`alloc-call|origin|0x1`,
/// `a1|0x100`).
const FRAME_ALIGN: u64 = 0x100;

/// Bytes each row of a picture is padded to. The 320-pixel clip's pitch was 512 (`fi`+0x3c, and
/// `video-frame-0-yuv|pitch|0x200`); 256 is assumed as the alignment that gives it, and
/// REQ-20261021T0000Z-vd02 measures other widths.
const PITCH_ALIGN: u32 = 256;

/// Bytes one frame buffer occupies in the pool: the clip's frames were 0x2e000 apart for a
/// 0x2d000-byte picture. Rounding to 8 KiB is assumed as the rule that gives it (vd02).
const FRAME_STRIDE_ALIGN: u64 = 0x2000;

/// The largest sound handed over in one `GetAudioData`: an AAC frame of 1024 sample frames in
/// eight channels. Larger decodes are handed over in parts.
const SOUND_BYTES: usize = 1024 * 8 * 2;

/// Sound buffers each player rotates through, in memory the library owns: the clip's audio
/// pointers lay in none of the callbacks' allocations (`audio-frame-0|ai-0x100` +0).
const SOUND_BUFFERS: usize = 4;

/// The player's own calls into the title: its object word, the memory callbacks and the event
/// callback with its object.
#[derive(Clone, Copy, Debug, Default)]
struct Callbacks {
    object: u64,
    allocate: u64,
    deallocate: u64,
    allocate_texture: u64,
    deallocate_texture: u64,
    event_object: u64,
    event: u64,
}

impl Callbacks {
    fn allocate(&self, align: u64, size: u64) -> Option<u64> {
        if self.allocate == 0 {
            return None;
        }
        // SAFETY: the allocate callback the guest put in its init block, in its mapped code.
        unsafe { orbistoun_kernel::thread::call_guest(self.allocate, [self.object, align, size]) }
            .filter(|&pointer| pointer != 0)
    }

    fn allocate_texture(&self, align: u64, size: u64) -> Option<u64> {
        if self.allocate_texture == 0 {
            return None;
        }
        // SAFETY: the texture allocate callback the guest put in its init block.
        unsafe {
            orbistoun_kernel::thread::call_guest(self.allocate_texture, [self.object, align, size])
        }
        .filter(|&pointer| pointer != 0)
    }

    fn free(&self, callback: u64, pointer: u64) {
        if callback != 0 && pointer != 0 {
            // SAFETY: a deallocate callback the guest put in its init block, handed a pointer its
            // own allocator answered.
            let _ = unsafe {
                orbistoun_kernel::thread::call_guest(callback, [self.object, pointer, 0])
            };
        }
    }

    /// Calls the event callback with `(object, id, 0)`. The fourth argument the console passes
    /// (0 for every state event vd01 rowed) is not set: guest calls here carry three.
    fn event(&self, id: u64) {
        if self.event != 0 {
            // SAFETY: the event callback the guest put in its init block.
            let _ = unsafe {
                orbistoun_kernel::thread::call_guest(self.event, [self.event_object, id, 0])
            };
        }
    }
}

/// Where a player is in its life.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// Opened, or its source refused: nothing to play.
    Idle,
    /// A source is added and not started.
    Ready,
    /// Playing since the guest clock read `started_ms`, less `paused_ms` spent paused.
    Playing { started_ms: u64, paused_ms: u64 },
    /// Paused at `at_ms` on the guest clock.
    Paused {
        started_ms: u64,
        paused_ms: u64,
        at_ms: u64,
    },
    /// Stopped by the title.
    Stopped,
}

/// The decoded frames' destination: a pool of frame buffers from the texture allocator.
#[derive(Clone, Copy, Debug)]
struct Pool {
    base: u64,
    count: u64,
    stride: u64,
    pitch: u32,
}

/// A source being played.
struct Source {
    media: Box<dyn Media>,
    streams: Vec<StreamDesc>,
    pool: Option<Pool>,
    /// Pictures handed out so far: the next goes in slot `(delivered + 1) % count`.
    delivered: u64,
    /// The picture decoded and not yet due.
    waiting: Option<Picture>,
    /// The rest of a sound larger than one buffer.
    leftover: Option<Sound>,
    /// When the last sound handed out starts, and the last picture is shown.
    sound_ms: Option<u64>,
    picture_ms: Option<u64>,
    /// Whether every picture has been handed out.
    pictures_done: bool,
    /// Library-owned sound buffers, and which one goes out next.
    sound_base: u64,
    sound_next: usize,
}

/// An open player.
struct Player {
    callbacks: Callbacks,
    framebuffers: u64,
    /// The block adding a source allocated, kept until `Close`.
    source_buffer: u64,
    /// Whether a source was ever asked for: `Close` then reports the player stopped.
    asked_for_source: bool,
    phase: Phase,
    source: Option<Source>,
}

type Shared = Arc<Mutex<Player>>;

/// Players opened by `InitEx`, by handle.
static PLAYERS: Mutex<BTreeMap<u64, Shared>> = Mutex::new(BTreeMap::new());

fn players() -> MutexGuard<'static, BTreeMap<u64, Shared>> {
    PLAYERS.lock().unwrap_or_else(PoisonError::into_inner)
}

fn player(handle: u64) -> Option<Shared> {
    players().get(&handle).cloned()
}

fn lock(player: &Shared) -> MutexGuard<'_, Player> {
    player.lock().unwrap_or_else(PoisonError::into_inner)
}

fn placeholder() -> u64 {
    u64::from(GuestError::Unimplemented.as_raw())
}

/// The guest's clock, in milliseconds since it started.
fn guest_ms() -> u64 {
    (orbistoun_hle::clocks::since_start_nanos() / 1_000_000) as u64
}

/// The row pitch the player gives a picture `width` pixels wide.
fn pitch_for(width: u32) -> u32 {
    width.next_multiple_of(PITCH_ALIGN)
}

/// Bytes between frame buffers in the pool, for pictures at `pitch` and `height`.
fn frame_stride(pitch: u32, height: u32) -> u64 {
    (u64::from(pitch) * u64::from(height) * 3 / 2).next_multiple_of(FRAME_STRIDE_ALIGN)
}

/// The slot frame `index` goes in: the first frame went in the second buffer, and the thirtieth of
/// six back in the first (`video-frame-0-alloc|offset|0x2e000`, frame 29 at the pool's base).
fn slot_for(index: u64, count: u64) -> u64 {
    (index + 1) % count.max(1)
}

/// A packed picture laid out at `pitch`: luma rows, then the interleaved chroma rows at
/// `pitch * height` (`video-frame-0-yuv|chroma-0x40` read at 122880 for a 512-pitch, 240-row
/// picture). The padding bytes are zero.
fn placed(picture: &Picture, pitch: u32) -> Vec<u8> {
    let (width, height, pitch) = (
        picture.width as usize,
        picture.height as usize,
        pitch as usize,
    );
    let chroma_rows = height.div_ceil(2);
    let mut out = vec![0_u8; pitch * (height + chroma_rows)];
    for (row, line) in picture
        .nv12
        .chunks(width)
        .take(height + chroma_rows)
        .enumerate()
    {
        out[row * pitch..][..line.len()].copy_from_slice(line);
    }
    out
}

/// The 0x40 bytes `GetVideoDataEx` writes for a picture at `data` (`video-frame-0|fi-0x100`):
/// the data pointer, a zero word, the time in milliseconds, then the details - width, height, the
/// aspect ratio as a float, the language - a zero word, the crop on each side, and the pitch.
/// The right crop is the pitch's padding: PPSA02664 takes the pitch less both side crops as the
/// width.
fn video_info(data: u64, picture: &Picture, pitch: u32, language: [u8; 3]) -> [u8; 0x40] {
    let mut info = [0_u8; 0x40];
    info[0x00..0x08].copy_from_slice(&data.to_le_bytes());
    info[0x10..0x18].copy_from_slice(&picture.time_ms.to_le_bytes());
    info[0x18..0x1c].copy_from_slice(&picture.width.to_le_bytes());
    info[0x1c..0x20].copy_from_slice(&picture.height.to_le_bytes());
    // The clip's square pixels answered 1.0 (`0000803f`); every picture is taken as square.
    info[0x20..0x24].copy_from_slice(&1.0_f32.to_le_bytes());
    info[0x24..0x27].copy_from_slice(&language);
    info[0x30..0x34].copy_from_slice(&(pitch - picture.width).to_le_bytes());
    info[0x3c..0x40].copy_from_slice(&pitch.to_le_bytes());
    info
}

/// The 0x28 bytes `GetAudioData` writes for sound at `data` (`audio-frame-0|ai-0x100`): the data
/// pointer, a zero word, the time, then the channel count, the rate, the size in bytes and the
/// language. The probe's block was untouched past +0x28.
fn audio_info(data: u64, sound: &Sound, language: [u8; 3]) -> [u8; 0x28] {
    let mut info = [0_u8; 0x28];
    info[0x00..0x08].copy_from_slice(&data.to_le_bytes());
    info[0x10..0x18].copy_from_slice(&sound.time_ms.to_le_bytes());
    info[0x18..0x1a].copy_from_slice(&sound.channels.to_le_bytes());
    info[0x1c..0x20].copy_from_slice(&sound.sample_rate.to_le_bytes());
    info[0x20..0x24].copy_from_slice(&((sound.samples.len() * 2) as u32).to_le_bytes());
    info[0x24..0x27].copy_from_slice(&language);
    info
}

/// The writes `GetStreamInfo` makes for one stream, as `(offset, bytes)` (`stream-0` and
/// `stream-1|stream-info-0x40`): the type at +0, the details from +8 and the duration in
/// milliseconds at +0x18; +4 and the bytes past +0x20 stay as they were. A video stream's word at
/// +0x10 (479696 for the clip) is not yet explained, and is not written.
fn stream_info(stream: &StreamDesc) -> Vec<(u64, Vec<u8>)> {
    let mut language = [0_u8; 4];
    language[..3].copy_from_slice(&stream.language);
    let mut writes = Vec::new();
    match stream.kind {
        StreamKind::Video { width, height } => {
            writes.push((0x00, STREAM_VIDEO.to_le_bytes().to_vec()));
            writes.push((0x08, width.to_le_bytes().to_vec()));
            writes.push((0x0c, height.to_le_bytes().to_vec()));
        }
        StreamKind::Audio {
            channels,
            sample_rate,
        } => {
            writes.push((0x00, STREAM_AUDIO.to_le_bytes().to_vec()));
            writes.push((0x08, channels.to_le_bytes().to_vec()));
            writes.push((0x0c, sample_rate.to_le_bytes().to_vec()));
            writes.push((0x10, 0_u32.to_le_bytes().to_vec()));
        }
    }
    writes.push((0x14, language.to_vec()));
    writes.push((0x18, stream.duration_ms.to_le_bytes().to_vec()));
    writes
}

/// The language of the first stream of `kind`'s sort, or `und`.
fn language_of(streams: &[StreamDesc], video: bool) -> [u8; 3] {
    streams
        .iter()
        .find(|stream| matches!(stream.kind, StreamKind::Video { .. }) == video)
        .map_or(*b"und", |stream| stream.language)
}

/// The presentation clock: the guest clock since `Start`, less time spent paused, and never behind
/// the sound handed out. vd01's pictures came as the sound pulled ahead of real time reached
/// their times, and `CurrentTime` followed the sound (`video-frame-3|current-time|0x6a` beside
/// sound frame 5 at 106 ms).
fn clock_ms(phase: Phase, now_ms: u64, sound_ms: Option<u64>) -> Option<u64> {
    let elapsed = match phase {
        Phase::Playing {
            started_ms,
            paused_ms,
        } => now_ms.saturating_sub(started_ms).saturating_sub(paused_ms),
        Phase::Paused {
            started_ms,
            paused_ms,
            at_ms,
        } => at_ms.saturating_sub(started_ms).saturating_sub(paused_ms),
        _ => return None,
    };
    Some(sound_ms.map_or(elapsed, |sound| elapsed.max(sound)))
}

/// `sceAvPlayerInitEx(init, handle_out)`: checks the init block, takes the player's memory through
/// its allocate callback, and writes the pointer that answered as the handle - the two are equal
/// (`InitEx-handle|equals-alloc-ptr 1`, `offset-from-alloc 0`). A block smaller than 0x230 and a
/// null allocate slot are refused with [`INVALID_ARGUMENT`], as measured; a larger block, a null
/// pointer and an allocation that fails are not measured and answer the placeholder.
pub(crate) fn init_ex(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (init, handle_out) = (args[0], args[1]);
    if init == 0 || handle_out == 0 {
        return placeholder();
    }
    let word = |offset: u64| {
        // SAFETY: a field of the guest's init block, read through the checked accessor.
        unsafe { guest::read_u64(init + offset) }
    };
    let (Some(size), Some(object), Some(allocate), Some(deallocate)) = (
        word(0),
        word(OBJECT_AT),
        word(ALLOCATE_AT),
        word(DEALLOCATE_AT),
    ) else {
        return placeholder();
    };
    if size < INIT_BLOCK_BYTES || allocate == 0 {
        return INVALID_ARGUMENT;
    }
    if size > INIT_BLOCK_BYTES {
        return placeholder();
    }
    let callbacks = Callbacks {
        object,
        allocate,
        deallocate,
        allocate_texture: word(ALLOCATE_TEXTURE_AT).unwrap_or(0),
        deallocate_texture: word(DEALLOCATE_TEXTURE_AT).unwrap_or(0),
        event_object: word(EVENT_OBJECT_AT).unwrap_or(0),
        event: word(EVENT_CALLBACK_AT).unwrap_or(0),
    };
    // SAFETY: a field of the guest's init block, read through the checked accessor.
    let framebuffers =
        unsafe { guest::read_u32(init + FRAMEBUFFERS_AT) }.map_or(DEFAULT_FRAMEBUFFERS, u64::from);
    let Some(handle) = callbacks.allocate(ALLOCATE_SECOND, ALLOCATE_THIRD) else {
        return placeholder();
    };
    // SAFETY: the guest's out-parameter, written through the checked accessor.
    if !unsafe { guest::write_u64(handle_out, handle) } {
        return placeholder();
    }
    let player = Player {
        callbacks,
        framebuffers: if framebuffers == 0 {
            DEFAULT_FRAMEBUFFERS
        } else {
            framebuffers
        },
        source_buffer: 0,
        asked_for_source: false,
        phase: Phase::Idle,
        source: None,
    };
    players().insert(handle, Arc::new(Mutex::new(player)));
    OK
}

/// `sceAvPlayerClose(handle)`: reports the player stopped when a source was ever asked for (vd01's
/// players each answered event 1 at `Close`, even the one whose file was missing), then frees the
/// frame buffers, the source's block and the player's own memory, in that order, and answers 0
/// (`player1-close`, `Close-first`). A handle `InitEx` did not open answers the placeholder.
pub(crate) fn close(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let handle = args[0];
    let Some(shared) = players().remove(&handle) else {
        return placeholder();
    };
    let (callbacks, asked, pool, source_buffer) = {
        let mut player = lock(&shared);
        let pool = player.source.take().and_then(|source| source.pool);
        player.phase = Phase::Idle;
        (
            player.callbacks,
            player.asked_for_source,
            pool,
            player.source_buffer,
        )
    };
    if asked {
        callbacks.event(EVENT_STOPPED);
    }
    if let Some(pool) = pool {
        callbacks.free(callbacks.deallocate_texture, pool.base);
    }
    callbacks.free(callbacks.deallocate, source_buffer);
    callbacks.free(callbacks.deallocate, handle);
    OK
}

/// `sceAvPlayerIsActive(handle)`: true - the low byte 1 - for a player with no source, one with a
/// source before and during play (`IsActive-before-source`, `is-active-before-start`); false once
/// the last picture has been handed out (`is-active-after-playback`, false as frame 29 went). A
/// handle `InitEx` did not open answers false.
pub(crate) fn is_active(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(shared) = player(args[0]) else {
        return 0;
    };
    let player = lock(&shared);
    let finished = player
        .source
        .as_ref()
        .is_some_and(|source| source.pictures_done);
    u64::from(!finished && player.phase != Phase::Stopped)
}

/// `sceAvPlayerPostInit(handle, post)`: a player with no source refuses it with
/// [`INVALID_ARGUMENT`], for the block `{0x800000 at +0, 1 at +8, 4 at +0x1c}` (`PostInit`) and for
/// an all-zero one (`PostInit-all-zeros`) alike. Both were measured on the probe's mostly-zero
/// init block; PPSA02664's own blocks are REQ-20261021T0000Z-vd02 arm 1. A handle `InitEx` did not
/// open answers the placeholder.
pub(crate) fn post_init(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    match player(args[0]) {
        Some(shared) if lock(&shared).source.is_none() => INVALID_ARGUMENT,
        _ => placeholder(),
    }
}

/// Opens the source at `path` for the player at `handle`: allocates the source's block first, as
/// the console did for every source, then opens. A missing file answers [`OPERATION_FAILED`]
/// (`add-source-missing`); a source that opens answers 0 and reports the player ready
/// (`add-source-real`, event 2). A second source on one player, and a file that will not play, are
/// not measured and answer the placeholder.
fn add_source(handle: u64, path: &str) -> u64 {
    let Some(shared) = player(handle) else {
        return placeholder();
    };
    let callbacks = {
        let mut player = lock(&shared);
        if player.source.is_some() || player.asked_for_source {
            return placeholder();
        }
        player.asked_for_source = true;
        player.callbacks
    };
    let Some(buffer) = callbacks.allocate(SOURCE_BUFFER_ALIGN, SOURCE_BUFFER_BYTES) else {
        return placeholder();
    };
    lock(&shared).source_buffer = buffer;
    let media = match media::open(path) {
        Ok(media) => media,
        Err(OpenError::NotFound) => return OPERATION_FAILED,
        Err(OpenError::Unreadable(why)) => {
            tracing::warn!(path, why, "libSceAvPlayer: the source does not play");
            return placeholder();
        }
    };
    let sound_base = orbistoun_mem::blocks::block(SOUND_BYTES * SOUND_BUFFERS / 8);
    {
        let mut player = lock(&shared);
        player.source = Some(Source {
            streams: media.streams().to_vec(),
            media,
            pool: None,
            delivered: 0,
            waiting: None,
            leftover: None,
            sound_ms: None,
            picture_ms: None,
            pictures_done: false,
            sound_base,
            sound_next: 0,
        });
        player.phase = Phase::Ready;
    }
    callbacks.event(EVENT_READY);
    OK
}

/// `sceAvPlayerAddSourceEx(handle, 0, details)`: the source named by the details block's URI
/// (pointer at +0, length at +8), for a source type that names a file (0 or 1). Other types are
/// not measured and answer the placeholder.
pub(crate) fn add_source_ex(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (handle, details) = (args[0], args[2]);
    if details == 0 {
        return placeholder();
    }
    // SAFETY: fields of the guest's details block, read through the checked accessor.
    let uri = unsafe { guest::read_u64(details + URI_AT) };
    // SAFETY: as above.
    let length = unsafe { guest::read_u32(details + URI_LENGTH_AT) };
    // SAFETY: as above.
    let kind = unsafe { guest::read_u32(details + SOURCE_TYPE_AT) };
    let (Some(uri), Some(length), Some(kind)) = (uri, length, kind) else {
        return placeholder();
    };
    if !FILE_SOURCE_TYPES.contains(&kind) || uri == 0 {
        return placeholder();
    }
    let mut bytes = vec![0_u8; length as usize];
    // SAFETY: the URI the guest's block points at, `length` bytes, read through the checked
    // accessor.
    if !unsafe { guest::read_into(uri, &mut bytes) } {
        return placeholder();
    }
    match String::from_utf8(bytes) {
        Ok(path) => add_source(handle, &path),
        Err(_) => placeholder(),
    }
}

/// `sceAvPlayerAddSource(handle, path)`: the file at the NUL-terminated `path`. Assumed to behave
/// as [`add_source_ex`] with a file type: PPSA04263 calls it (image+0x30413ca), and no probe has
/// yet.
pub(crate) fn add_source_plain(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // SAFETY: the guest's path argument, read through the checked accessor.
    match unsafe { guest::read_path(args[1]) } {
        Some(path) => add_source(args[0], &path),
        None => placeholder(),
    }
}

/// `sceAvPlayerStreamCount(handle)`: the source's streams (`stream-count|count|0x2` for video and
/// sound), none before a source (`StreamCount-before-source 0`). A handle `InitEx` did not open
/// answers the placeholder.
pub(crate) fn stream_count(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    match player(args[0]) {
        Some(shared) => lock(&shared)
            .source
            .as_ref()
            .map_or(0, |source| source.streams.len() as u64),
        None => placeholder(),
    }
}

/// `sceAvPlayerGetStreamInfo(handle, id, out)`: writes the stream's [`stream_info`] and answers 0;
/// an id past the last answers [`INVALID_ARGUMENT`] and writes nothing (`stream-2`).
pub(crate) fn get_stream_info(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (handle, id, out) = (args[0], args[1], args[2]);
    let Some(shared) = player(handle) else {
        return placeholder();
    };
    let player = lock(&shared);
    let Some(source) = player.source.as_ref() else {
        return placeholder();
    };
    let Some(stream) = usize::try_from(id)
        .ok()
        .and_then(|id| source.streams.get(id))
    else {
        return INVALID_ARGUMENT;
    };
    for (offset, bytes) in stream_info(stream) {
        // SAFETY: the guest's out block, written through the checked accessor.
        if !unsafe { guest::write_bytes(out + offset, &bytes) } {
            return placeholder();
        }
    }
    OK
}

/// `sceAvPlayerEnableStream(handle, id)`: before `Start` every stream of the source is accepted
/// (`player1-enable-stream0` and `stream1`, 0); once started the player refuses with
/// [`OPERATION_FAILED`] (`player2-enable-stream`). Both streams play either way.
pub(crate) fn enable_stream(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (handle, id) = (args[0], args[1]);
    let Some(shared) = player(handle) else {
        return placeholder();
    };
    let player = lock(&shared);
    match (&player.source, player.phase) {
        (Some(source), Phase::Ready) if (id as usize) < source.streams.len() => OK,
        (Some(_), Phase::Playing { .. } | Phase::Paused { .. }) => OPERATION_FAILED,
        _ => placeholder(),
    }
}

/// `sceAvPlayerStart(handle)`: allocates the frame buffers through the texture allocator, starts
/// the clock and reports the player playing (event 3), answering 0 (`start`). The console also
/// allocated its decoder's working memory here (0x495300, 0xc1f980, 0x2e0300 and 0x1d9800 bytes
/// for the clip); this player decodes on the host and asks only for the frame buffers. A player
/// with no source, or one already started, is not measured and answers the placeholder.
pub(crate) fn start(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(shared) = player(args[0]) else {
        return placeholder();
    };
    let (callbacks, framebuffers, shape) = {
        let player = lock(&shared);
        let Some(source) = player.source.as_ref() else {
            return placeholder();
        };
        if player.phase != Phase::Ready {
            return placeholder();
        }
        let shape = source.streams.iter().find_map(|stream| match stream.kind {
            StreamKind::Video { width, height } => Some((width, height)),
            StreamKind::Audio { .. } => None,
        });
        (player.callbacks, player.framebuffers, shape)
    };
    let pool = match shape {
        Some((width, height)) => {
            let pitch = pitch_for(width);
            let stride = frame_stride(pitch, height);
            let Some(base) = callbacks.allocate_texture(FRAME_ALIGN, stride * framebuffers) else {
                return placeholder();
            };
            Some(Pool {
                base,
                count: framebuffers,
                stride,
                pitch,
            })
        }
        None => None,
    };
    {
        let mut player = lock(&shared);
        if let Some(source) = player.source.as_mut() {
            source.pool = pool;
        }
        player.phase = Phase::Playing {
            started_ms: guest_ms(),
            paused_ms: 0,
        };
    }
    callbacks.event(EVENT_PLAYING);
    OK
}

/// `sceAvPlayerGetVideoDataEx(handle, info)`: hands out the next picture once the clock reaches
/// its time, written into the next frame buffer at the player's pitch, with its [`video_info`];
/// answers true (the low byte 1, `video-frame-0|rax`). Before its time, before `Start` and after
/// the last picture it answers false and writes nothing.
pub(crate) fn get_video_data_ex(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (handle, out) = (args[0], args[1]);
    let Some(shared) = player(handle) else {
        return 0;
    };
    let mut player = lock(&shared);
    let phase = player.phase;
    let Some(source) = player.source.as_mut() else {
        return 0;
    };
    let Some(clock) = clock_ms(phase, guest_ms(), source.sound_ms) else {
        return 0;
    };
    if matches!(phase, Phase::Paused { .. }) {
        return 0;
    }
    if source.waiting.is_none() {
        match source.media.next_picture() {
            Ok(Some(picture)) => source.waiting = Some(picture),
            Ok(None) => {
                source.pictures_done = source.media.pictures_ended();
                return 0;
            }
            Err(why) => {
                tracing::warn!(why, "libSceAvPlayer: the video stream stopped");
                source.pictures_done = true;
                return 0;
            }
        }
    }
    let Some(pool) = source.pool else {
        return 0;
    };
    match &source.waiting {
        Some(picture) if picture.time_ms <= clock => {}
        _ => return 0,
    }
    let Some(picture) = source.waiting.take() else {
        return 0;
    };
    let data = pool.base + slot_for(source.delivered, pool.count) * pool.stride;
    let language = language_of(&source.streams, true);
    // SAFETY: the frame buffer the title's texture allocator answered for this pool, at most
    // `stride` bytes of it; then the guest's info block.
    let written = unsafe { guest::write_bytes(data, &placed(&picture, pool.pitch)) }
        // SAFETY: as above.
        && unsafe { guest::write_bytes(out, &video_info(data, &picture, pool.pitch, language)) };
    if !written {
        return 0;
    }
    source.delivered += 1;
    source.picture_ms = Some(picture.time_ms);
    // IsActive turned false as the last picture went, with sound still to come
    // (`is-active-turned-false` at frame 29's time, before sound frames 46 and 47).
    source.pictures_done = source.media.pictures_ended();
    1
}

/// `sceAvPlayerGetAudioData(handle, info)`: hands out the next decoded sound at once, in a buffer
/// the library owns, with its [`audio_info`]; answers true (`audio-frame-0|rax`, low byte 1). The
/// console gave sound as fast as the probe asked (48 frames in 0.69 s), and the pictures followed
/// it.
pub(crate) fn get_audio_data(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (handle, out) = (args[0], args[1]);
    let Some(shared) = player(handle) else {
        return 0;
    };
    let mut player = lock(&shared);
    if !matches!(player.phase, Phase::Playing { .. }) {
        return 0;
    }
    let Some(source) = player.source.as_mut() else {
        return 0;
    };
    let sound = match source.leftover.take() {
        Some(sound) => sound,
        None => match source.media.next_sound() {
            Ok(Some(sound)) => sound,
            Ok(None) => return 0,
            Err(why) => {
                tracing::warn!(why, "libSceAvPlayer: the audio stream stopped");
                return 0;
            }
        },
    };
    let (sound, rest) = split_sound(sound, SOUND_BYTES / 2);
    source.leftover = rest;
    let data = source.sound_base + (source.sound_next * SOUND_BYTES) as u64;
    source.sound_next = (source.sound_next + 1) % SOUND_BUFFERS;
    let bytes: Vec<u8> = sound.samples.iter().flat_map(|s| s.to_le_bytes()).collect();
    let language = language_of(&source.streams, false);
    // SAFETY: a sound buffer this library owns, `SOUND_BYTES` long; then the guest's info block.
    let written = unsafe { guest::write_bytes(data, &bytes) }
        // SAFETY: as above.
        && unsafe { guest::write_bytes(out, &audio_info(data, &sound, language)) };
    if !written {
        return 0;
    }
    source.sound_ms = Some(sound.time_ms);
    1
}

/// `sound` cut to at most `limit` samples, whole sample frames, and what remains, timed after it.
fn split_sound(mut sound: Sound, limit: usize) -> (Sound, Option<Sound>) {
    let width = usize::from(sound.channels.max(1));
    let keep = limit - limit % width;
    if sound.samples.len() <= keep {
        return (sound, None);
    }
    let rest = sound.samples.split_off(keep);
    let frames = (keep / width) as u64;
    let rest = Sound {
        time_ms: sound.time_ms + frames * 1000 / u64::from(sound.sample_rate.max(1)),
        channels: sound.channels,
        sample_rate: sound.sample_rate,
        samples: rest,
    };
    (sound, Some(rest))
}

/// `sceAvPlayerCurrentTime(handle)`: where playback is, in milliseconds - the time of the last
/// sound handed out (`video-frame-N|current-time` tracked the sound), or of the last picture for a
/// source with no sound; 0 before `Start` (`current-time-before-start`).
pub(crate) fn current_time(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(shared) = player(args[0]) else {
        return placeholder();
    };
    let player = lock(&shared);
    player
        .source
        .as_ref()
        .and_then(|source| source.sound_ms.or(source.picture_ms))
        .unwrap_or(0)
}

/// `sceAvPlayerPause(handle)`: stops the clock and reports the player paused (event 4), answering
/// 0; a player already paused answers [`OPERATION_FAILED`] (`player2-pause|rc1`, `rc2`). Pausing
/// before `Start` is not measured.
pub(crate) fn pause(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(shared) = player(args[0]) else {
        return placeholder();
    };
    let callbacks = {
        let mut player = lock(&shared);
        match player.phase {
            Phase::Playing {
                started_ms,
                paused_ms,
            } => {
                player.phase = Phase::Paused {
                    started_ms,
                    paused_ms,
                    at_ms: guest_ms(),
                };
                player.callbacks
            }
            Phase::Paused { .. } => return OPERATION_FAILED,
            _ => return placeholder(),
        }
    };
    callbacks.event(EVENT_PAUSED);
    OK
}

/// `sceAvPlayerResume(handle)`: starts the clock again and reports the player playing (event 3),
/// answering 0; a player not paused answers [`OPERATION_FAILED`] (`player2-resume|rc1`, `rc2`).
pub(crate) fn resume(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(shared) = player(args[0]) else {
        return placeholder();
    };
    let callbacks = {
        let mut player = lock(&shared);
        match player.phase {
            Phase::Paused {
                started_ms,
                paused_ms,
                at_ms,
            } => {
                player.phase = Phase::Playing {
                    started_ms,
                    paused_ms: paused_ms + guest_ms().saturating_sub(at_ms),
                };
                player.callbacks
            }
            Phase::Playing { .. } => return OPERATION_FAILED,
            _ => return placeholder(),
        }
    };
    callbacks.event(EVENT_PLAYING);
    OK
}

/// `sceAvPlayerSetTrickSpeed(handle, speed)`: normal speed, 100, is accepted while playing
/// (`player2-trick-speed|rc-100`); 200 is refused with [`NOT_SUPPORTED`] (`rc-200`). Other speeds
/// are not measured.
pub(crate) fn set_trick_speed(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if player(args[0]).is_none() {
        return placeholder();
    }
    match args[1] as i32 {
        100 => OK,
        200 => NOT_SUPPORTED,
        _ => placeholder(),
    }
}

/// `sceAvPlayerSetAvSyncMode(handle, mode)`: modes 0 and 1 are accepted while playing
/// (`player2-sync-mode|rc-0`, `rc-1`). Pictures follow the sound in either; other modes are not
/// measured.
pub(crate) fn set_av_sync_mode(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if player(args[0]).is_none() {
        return placeholder();
    }
    match args[1] {
        0 | 1 => OK,
        _ => placeholder(),
    }
}

/// `sceAvPlayerChangeStream(handle, old, new)`: accepted while playing (`player2-change-stream`,
/// `(1, 1)`), the one pair measured; the source's single sound stream keeps playing.
pub(crate) fn change_stream(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(shared) = player(args[0]) else {
        return placeholder();
    };
    let playing = matches!(
        lock(&shared).phase,
        Phase::Playing { .. } | Phase::Paused { .. }
    );
    if playing && args[1] == args[2] {
        OK
    } else {
        placeholder()
    }
}

/// `sceAvPlayerSetLogCallback(callback, object, level)`: accepted (`SetLogCallback|rc|0x0`); the
/// console's player made no call to it in the measured lifecycle (`callbacks 0`), and this one
/// makes none.
pub(crate) fn set_log_callback(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

/// `sceAvPlayerStop(handle)`: a player with a source stops - during play or after its end - and
/// reports itself stopped (event 1), answering 0 (`player1-stop-after-end`, `player2-stop-midplay`).
/// A player with no source answers [`OPERATION_FAILED`] on every call and stays open; a handle
/// `InitEx` did not open is refused with [`INVALID_ARGUMENT`] (REQ-av01,
/// `report-20261015-batch1.log` 889-894). PPSA02664 stops its player after `PostInit` fails.
pub(crate) fn stop(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(shared) = player(args[0]) else {
        return INVALID_ARGUMENT;
    };
    let callbacks = {
        let mut player = lock(&shared);
        if player.source.is_none() {
            return OPERATION_FAILED;
        }
        player.phase = Phase::Stopped;
        player.callbacks
    };
    callbacks.event(EVENT_STOPPED);
    OK
}

#[cfg(test)]
mod tests {
    use orbistoun_core::GUEST_ARG_REGISTERS;

    use super::{Phase, Picture, Sound, StreamDesc, StreamKind};

    fn bytes(hex: &str) -> Vec<u8> {
        (0..hex.len())
            .step_by(2)
            .map(|at| u8::from_str_radix(&hex[at..at + 2], 16).unwrap())
            .collect()
    }

    /// The init block the measured `InitEx` accepted: its size word, the object word at +0x08 and a
    /// callback in each slot named here.
    fn block(size: u64, allocate: u64, deallocate: u64) -> Vec<u8> {
        let mut block = vec![0_u8; 0x230];
        block[..8].copy_from_slice(&size.to_le_bytes());
        block[8..16].copy_from_slice(&0x5a5a_0000_u64.to_le_bytes());
        block[0x10..0x18].copy_from_slice(&allocate.to_le_bytes());
        block[0x18..0x20].copy_from_slice(&deallocate.to_le_bytes());
        block
    }

    fn init(block: &[u8], handle: &mut u64) -> u64 {
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = block.as_ptr() as u64;
        args[1] = std::ptr::from_mut(handle).expose_provenance() as u64;
        super::init_ex(&args)
    }

    fn open_player(handle: u64) {
        super::players().insert(
            handle,
            std::sync::Arc::new(std::sync::Mutex::new(super::Player {
                callbacks: super::Callbacks::default(),
                framebuffers: 6,
                source_buffer: 0,
                asked_for_source: false,
                phase: Phase::Idle,
                source: None,
            })),
        );
    }

    /// The clip's picture 0 at 320x240, its time and size; the bytes do not matter here.
    fn picture(time_ms: u64) -> Picture {
        Picture {
            time_ms,
            width: 320,
            height: 240,
            nv12: vec![0x50; 320 * 240 * 3 / 2],
        }
    }

    /// A block smaller than 0x230, and one with no allocate callback, are refused with
    /// `0x806a0001` before anything is allocated, and the handle is left as it was.
    #[test]
    fn a_short_block_or_a_missing_allocator_is_refused() {
        let mut handle = 0xcc_u64;
        assert_eq!(init(&block(0x100, 0x1234, 0), &mut handle), 0x806a_0001);
        assert_eq!(init(&block(0x230, 0, 0), &mut handle), 0x806a_0001);
        assert_eq!(handle, 0xcc);
    }

    /// `Stop` on a player with no source - PPSA02664's, after its `PostInit` failed - answers
    /// `0x806a0002`, again on a second call, and the player stays open; on a handle `InitEx` did not
    /// open it answers `0x806a0001` (REQ-av01, `report-20261015-batch1.log`,
    /// `000-boot/sd02-savedata-avplayer`).
    #[test]
    fn stop_on_a_player_with_no_source_answers_as_measured() {
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = 0x5157_0000;
        open_player(args[0]);
        assert_eq!(super::stop(&args), 0x806a_0002);
        assert_eq!(super::stop(&args), 0x806a_0002);
        assert_eq!(super::is_active(&args), 1);
        args[0] = 0x5157_1000;
        assert_eq!(super::stop(&args), 0x806a_0001);
    }

    /// A handle `InitEx` never opened is not active, has no stream count and does not close.
    #[test]
    fn an_unopened_handle_is_inactive() {
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = 0xdead_0000;
        assert_eq!(super::is_active(&args), 0);
        assert_ne!(super::stream_count(&args), 0);
        assert_ne!(super::post_init(&args), 0x806a_0001);
        assert_ne!(super::close(&args), 0);
    }

    /// The clip's frame info, byte for byte (`video-frame-0|fi-0x100`, the 0x40 bytes rowed): the
    /// pointer 0x202efe000, time 0, 320x240, aspect 1.0, `und`, right crop 192 and pitch 512.
    /// Frame 29's differs only in its pointer and its time, 966 ms.
    #[test]
    fn frame_info_is_the_measured_block() {
        let info = super::video_info(0x2_02ef_e000, &picture(0), 512, *b"und");
        assert_eq!(
            info.to_vec(),
            bytes(
                "00e0ef02020000000000000000000000000000000000000040010000f00000000000803f756e64000000000000000000c0000000000000000000000000020000"
            )
        );
        let info = super::video_info(0x2_02ed_0000, &picture(966), 512, *b"und");
        assert_eq!(
            info.to_vec(),
            bytes(
                "0000ed02020000000000000000000000c60300000000000040010000f00000000000803f756e64000000000000000000c0000000000000000000000000020000"
            )
        );
    }

    /// The clip's sound info for its second frame (`audio-frame-1|ai-0x100`): the pointer, time
    /// 21 ms, 2 channels at 48 kHz, 4096 bytes, `und`.
    #[test]
    fn sound_info_is_the_measured_block() {
        let sound = Sound {
            time_ms: 21,
            channels: 2,
            sample_rate: 48_000,
            samples: vec![0; 2048],
        };
        assert_eq!(
            super::audio_info(0x2_00d8_0020, &sound, *b"und").to_vec(),
            bytes(
                "2000d80002000000000000000000000015000000000000000200000080bb000000100000756e6400"
            )
        );
    }

    /// Stream info as measured for the clip's two streams: what is written, applied over a block
    /// of `0xcc`, gives the rowed 0x40 bytes - except the video stream's word at +0x10, which is
    /// left as it was.
    #[test]
    fn stream_info_is_the_measured_block() {
        let apply = |stream: &StreamDesc| {
            let mut out = vec![0xcc_u8; 0x40];
            for (offset, bytes) in super::stream_info(stream) {
                out[offset as usize..][..bytes.len()].copy_from_slice(&bytes);
            }
            out
        };
        let video = StreamDesc {
            kind: StreamKind::Video {
                width: 320,
                height: 240,
            },
            duration_ms: 1000,
            language: *b"und",
        };
        let mut measured = bytes(
            "01000000cccccccc40010000f0000000d0510700756e6400e803000000000000cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
        );
        measured[0x10..0x14].copy_from_slice(&[0xcc; 4]);
        assert_eq!(apply(&video), measured);
        let audio = StreamDesc {
            kind: StreamKind::Audio {
                channels: 2,
                sample_rate: 48_000,
            },
            duration_ms: 1000,
            language: *b"und",
        };
        assert_eq!(
            apply(&audio),
            bytes(
                "02000000cccccccc0200cccc80bb000000000000756e6400e803000000000000cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
            )
        );
    }

    /// The clip's frame pool: a 512 pitch for 320 pixels, frames 0x2e000 apart, frame 0 in the
    /// second buffer and frame 29 back in the first of six.
    #[test]
    fn the_frame_pool_is_laid_out_as_measured() {
        assert_eq!(super::pitch_for(320), 512);
        assert_eq!(super::frame_stride(512, 240), 0x2e000);
        assert_eq!(super::slot_for(0, 6), 1);
        assert_eq!(super::slot_for(1, 6), 2);
        assert_eq!(super::slot_for(29, 6), 0);
    }

    /// A picture placed at the pitch: luma rows at the pitch, chroma rows from `pitch * height`,
    /// padding zero.
    #[test]
    fn a_picture_is_placed_at_the_pitch() {
        let mut picture = picture(0);
        picture.nv12[0] = 0x4e;
        picture.nv12[320] = 0x4b;
        picture.nv12[320 * 240] = 0xd9;
        let placed = super::placed(&picture, 512);
        assert_eq!(placed.len(), 512 * 360);
        assert_eq!(
            (placed[0], placed[512], placed[512 * 240]),
            (0x4e, 0x4b, 0xd9)
        );
        assert_eq!(placed[320], 0);
        assert_eq!(placed[512 + 1], 0x50);
    }

    /// The clock runs from `Start`, stops while paused, and never falls behind the sound handed
    /// out.
    #[test]
    fn the_clock_follows_play_pause_and_the_sound() {
        let playing = Phase::Playing {
            started_ms: 1000,
            paused_ms: 0,
        };
        assert_eq!(super::clock_ms(playing, 1100, None), Some(100));
        assert_eq!(super::clock_ms(playing, 1100, Some(400)), Some(400));
        let paused = Phase::Paused {
            started_ms: 1000,
            paused_ms: 50,
            at_ms: 1300,
        };
        assert_eq!(super::clock_ms(paused, 5000, None), Some(250));
        assert_eq!(super::clock_ms(Phase::Ready, 5000, None), None);
    }

    /// A decode longer than one buffer is handed over in parts, whole sample frames each, the
    /// second timed after the first.
    #[test]
    fn a_long_sound_is_split() {
        let sound = Sound {
            time_ms: 100,
            channels: 2,
            sample_rate: 48_000,
            samples: vec![1; 4800 * 2],
        };
        let (first, rest) = super::split_sound(sound, 4801);
        assert_eq!(first.samples.len(), 4800);
        let rest = rest.expect("a remainder");
        assert_eq!(rest.samples.len(), 4800);
        assert_eq!(rest.time_ms, 150);
    }
}
