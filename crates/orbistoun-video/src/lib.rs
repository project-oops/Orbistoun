//! Video output HLE - libSceVideoOut.
//!
//! Owns the display ports and the flip queue: the guest registers buffers, submits flips and
//! waits on their completion. With no scanout, a flip completes when it is accepted, so a guest
//! that polls flip completion proceeds rather than hangs (D516).

use orbistoun_hle::guest_module;

pub mod av_player;
pub mod recording;

guest_module! {
    "libSceVideoOut" {
        "sceVideoOutOpen" => 4,
        "sceVideoOutClose" => 1,
        "sceVideoOutRegisterBuffers" => 6,
        "sceVideoOutSubmitFlip" => 4,
        // Arity 3: an event-queue handle orbistoun issued, the port handle `sceVideoOutOpen`
        // answered, and the caller's opaque word.
        "sceVideoOutAddFlipEvent" => 3,
        "sceVideoOutSetFlipRate" => 2,
        "sceVideoOutConfigureOutput" => 4,
        "sceVideoOutGetFlipStatus" => 2,
        // Arity 1: the port handle, the same shape as `sceVideoOutClose`.
        "sceVideoOutIsFlipPending" => 1,
        "sceVideoOutGetResolutionStatus" => 2,
        "sceVideoOutRegisterBuffers2" => 6,
        // Declared at the trampoline's full arity 6 and not implemented, so the stub policy answers
        // them by name rather than as bare hashes.
        "sceVideoOutSetBufferAttribute2" => 6,
        "sceVideoOutGetOutputStatus" => 6,
    }
}

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError, GuestFn};
use orbistoun_mem::guest;

/// Successful return, as the guest reads it.
const OK: u64 = 0;

/// Video-out error codes, base `0x8029_0000`.
///
/// A family distinct from the kernel's `0x8002_00xx`. A refusing video-out call answers one of
/// these, not a `GuestError` placeholder, which a guest testing against zero would read as a valid
/// handle or status (D426).
mod video_error {
    /// A handle that names no open port. Measured: `080-video/flip-rate-rejects-bad-handle` records
    /// `sceVideoOutSetFlipRate` refusing a bad handle with `0x8029_000b`; assumed uniform across
    /// the family.
    pub(super) const INVALID_HANDLE: u64 = 0x8029_000b;
    /// The output is already open, so a second open is refused. Measured: obSCEne's display path
    /// records `sceVideoOutOpen` of the held main output answering `0x8029_0009`.
    pub(super) const ALREADY_OPEN: u64 = 0x8029_0009;
}

/// The shape of a registered buffer set, decoded from its attribute block.
///
/// Reading a flipped buffer as an image needs its extent, format and tiling, which the guest
/// writes into the attribute block with `sceVideoOutSetBufferAttribute2`. The fields sit at
/// hardware-measured offsets. Pitch (offsets `0x0`, `0x8`, `0x14`) reads zero in every measured
/// pass and is not modelled. Zeroed until a set is registered with a filled block.
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct BufferShape {
    /// Tiling mode: `0` tiled, `1` linear (attribute offset `0x4`).
    pub tiling: u32,
    /// Width in pixels (attribute offset `0xc`).
    pub width: u32,
    /// Height in pixels (attribute offset `0x10`).
    pub height: u32,
    /// Pixel format (attribute offset `0x20`).
    pub format: u64,
}

/// Display ports the guest can open.
///
/// A handle is an index into this, offset so zero is never valid: callers test a handle against
/// zero and negative values.
mod port {
    use std::sync::{Mutex, OnceLock};

    /// One display port.
    ///
    /// The bus and index identify the output, which the ownership check reads: a second open of an
    /// output that is already open is refused.
    #[derive(Default)]
    pub(super) struct Port {
        /// Which output bus this port opened.
        pub bus: u64,
        /// Which index within that bus.
        pub index: u64,
        /// Whether it is still open. A closed port's output can be opened again.
        pub open: bool,
        /// How many buffers have been registered against it.
        pub registered: u64,
        /// How many flips have completed - the count `sceVideoOutGetFlipStatus` reports and a guest
        /// polls.
        pub flips: u64,
        /// The guest addresses of the buffers registered against this port, in index order. Empty
        /// until the guest registers a set. These are the frames a renderer writes and a reader
        /// reads back.
        pub buffers: Vec<u64>,
        /// The `attribute` pointer (`arg4`) the guest passed to `sceVideoOutRegisterBuffers`,
        /// describing the buffers' pixel format, extent and tiling. Zero until a set is registered.
        pub attribute: u64,
        /// The buffer set's shape, decoded from the attribute block at register time, so a reader
        /// does not dereference the guest pointer again. Zeroed until a set with a filled block is
        /// registered.
        pub shape: super::BufferShape,
        /// The buffer index the guest last submitted a flip for: which of [`Self::buffers`] it last
        /// asked to present.
        pub last_flip: Option<u64>,
    }

    /// The handle of the port that most recently completed a flip, or zero for none.
    ///
    /// `submit_flip` records it and `last_flipped_buffer` reads it, so a reader of the flipped
    /// frame need not guess the port.
    fn last_flipped_handle() -> &'static std::sync::atomic::AtomicU64 {
        static HANDLE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        &HANDLE
    }

    /// Records `handle` as the port that most recently flipped.
    pub(super) fn note_flipped(handle: u64) {
        last_flipped_handle().store(handle, std::sync::atomic::Ordering::Relaxed);
    }

    /// For the port that last flipped, the guest address of the buffer it presented and the decoded
    /// [`BufferShape`](super::BufferShape) registered with it, or `None` if none has flipped or the
    /// flipped index has no registered buffer.
    pub(super) fn last_flipped_buffer() -> Option<(u64, super::BufferShape)> {
        let handle = last_flipped_handle().load(std::sync::atomic::Ordering::Relaxed);
        with(handle, |p| {
            let index = usize::try_from(p.last_flip?).ok()?;
            let address = p.buffers.get(index).copied()?;
            Some((address, p.shape))
        })
        .flatten()
    }

    /// Every port ever opened. Index plus [`FIRST`] is the handle; a closed one stays, so handles
    /// are never renumbered.
    fn table() -> &'static Mutex<Vec<Port>> {
        static TABLE: OnceLock<Mutex<Vec<Port>>> = OnceLock::new();
        TABLE.get_or_init(|| Mutex::new(Vec::new()))
    }

    /// Every flip this process has completed, across every port ever opened.
    ///
    /// Closed ports are included: a guest that opened an output, presented and closed it did
    /// present.
    pub(super) fn flips() -> u64 {
        table()
            .lock()
            .map_or(0, |t| t.iter().map(|p| p.flips).sum())
    }

    /// The first handle handed out.
    ///
    /// Small and positive, and not an address: the guest compares it against zero and passes it
    /// back, and never dereferences it (D151).
    pub(super) const FIRST: u64 = 1;

    /// Why an open did not hand back a handle.
    pub(super) enum OpenFailure {
        /// The output is already open; the hardware refuses a second open of it.
        AlreadyOpen,
        /// The table could not be reached.
        Unavailable,
    }

    /// Opens an output, or says why not. An output already open is refused rather than handed a
    /// second handle, as the hardware does.
    pub(super) fn open(bus: u64, index: u64) -> Result<u64, OpenFailure> {
        let mut table = table().lock().map_err(|_| OpenFailure::Unavailable)?;
        if table
            .iter()
            .any(|p| p.open && p.bus == bus && p.index == index)
        {
            return Err(OpenFailure::AlreadyOpen);
        }
        table.push(Port {
            bus,
            index,
            open: true,
            ..Port::default()
        });
        Ok(FIRST + table.len() as u64 - 1)
    }

    /// Runs `f` against an open port.
    pub(super) fn with<R>(handle: u64, f: impl FnOnce(&mut Port) -> R) -> Option<R> {
        let index = usize::try_from(handle.checked_sub(FIRST)?).ok()?;
        table().lock().ok()?.get_mut(index).map(f)
    }
}

/// `sceVideoOutOpen(user, bus, index, param)`.
///
/// Answers a small positive handle, not an address, which the guest compares against zero and
/// passes back to later video-out calls. `(bus, index)` names an output, and opening one that is
/// already open answers [`video_error::ALREADY_OPEN`], as the hardware does.
fn video_out_open(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // Before the port table is touched: a category denied the scanout never opens a port, so
    // opening here and refusing later would leave a usable port (D662).
    if let Some(refused) = refuse_scanout(orbistoun_core::category::presented()) {
        return refused;
    }
    let (bus, index) = (args[1], args[2]);
    match port::open(bus, index) {
        Ok(handle) => handle,
        Err(port::OpenFailure::AlreadyOpen) => video_error::ALREADY_OPEN,
        Err(port::OpenFailure::Unavailable) => u64::from(GuestError::Unimplemented.as_raw()),
    }
}

/// What a category is refused the scanout with, or [`None`] where it may open one.
///
/// A pure decision separate from the call, so it is testable without a port table.
fn refuse_scanout(category: orbistoun_core::category::Category) -> Option<u64> {
    orbistoun_core::category::scanout_refusal(category).map(u64::from)
}

/// `sceVideoOutClose(handle)`.
///
/// The port stays in the table: handles are indices, and removing one would renumber the rest or
/// leave a hole a later open would reuse.
fn video_out_close(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // Closed, so its output can be opened again; the ownership check in `port::open` reads this.
    match port::with(args[0], |p| {
        p.registered = 0;
        p.open = false;
    }) {
        Some(()) => OK,
        None => video_error::INVALID_HANDLE,
    }
}

/// The most buffers one registration binds. A display set is a handful of frames, so this bounds
/// the read of the guest's address array against a garbage count. A defensive ceiling, not a
/// hardware maximum.
const MAX_REGISTERED_BUFFERS: usize = 16;

/// `sceVideoOutRegisterBuffers(handle, index, addresses, count, attribute)`.
///
/// Records the guest addresses of the buffers at the indices the guest names, and the attribute
/// pointer describing their format, extent and tiling. `index` (`args[1]`) is where in the set
/// they land; `addresses` (`args[2]`) is the array of `count` guest addresses.
///
/// Answers success, so a guest proceeds to submit flips. A null address array is refused with the
/// video-out error, since it registers buffers nothing could read.
fn video_out_register_buffers(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (handle, start_index, addresses, count, attribute) =
        (args[0], args[1], args[2], args[3], args[4]);
    if addresses == 0 {
        return video_error::INVALID_HANDLE;
    }
    // v1 hands a raw `void*[]` at arg2: one guest address every eight bytes.
    // SAFETY: the guest's address array, `count` entries by the call's contract, clamped.
    let read = unsafe { read_buffer_addresses(addresses, clamp_count(start_index, count), 8) };
    // SAFETY: the guest's attribute block, by the call's contract.
    let shape = unsafe { decode_attribute(attribute) };
    register_buffer_set(handle, start_index, &read, count, attribute, shape)
}

/// `sceVideoOutRegisterBuffers2(handle, _, _, buffers, count, attribute, ...)`.
///
/// The v2 registration, a different shape from v1: arg2 is zero, and arg3 is an array of
/// `SceVideoOutBuffer` structs `{ data, metadata, reserved[2] }`, 32 bytes each with the buffer's
/// address in `data` at offset 0. The count is arg4 and the attribute arg5. The addresses are read
/// from the structs' `data` fields.
fn video_out_register_buffers2(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (handle, start_index, buffers, count, attribute) =
        (args[0], args[1], args[3], args[4], args[5]);
    if buffers == 0 {
        return video_error::INVALID_HANDLE;
    }
    // v2 hands a `SceVideoOutBuffer[]`: the address is the `data` field at offset zero of each
    // 32-byte struct.
    // SAFETY: the guest's buffer array, `count` entries by the call's contract, clamped.
    let read = unsafe {
        read_buffer_addresses(
            buffers,
            clamp_count(start_index, count),
            SCE_VIDEO_OUT_BUFFER_SIZE,
        )
    };
    // SAFETY: the guest's attribute block, by the call's contract.
    let shape = unsafe { decode_attribute(attribute) };
    register_buffer_set(handle, start_index, &read, count, attribute, shape)
}

/// Bytes one `SceVideoOutBuffer` occupies: `{ data, metadata, reserved[2] }`, the buffer's address
/// in `data` at offset 0 (agc_display.c).
const SCE_VIDEO_OUT_BUFFER_SIZE: u64 = 32;

/// How many buffers a register call maps, clamped so a guest cannot read past the end of its own
/// set or the port's capacity.
fn clamp_count(start_index: u64, count: u64) -> usize {
    let start = usize::try_from(start_index)
        .unwrap_or(MAX_REGISTERED_BUFFERS)
        .min(MAX_REGISTERED_BUFFERS);
    usize::try_from(count)
        .unwrap_or(0)
        .min(MAX_REGISTERED_BUFFERS.saturating_sub(start))
}

/// Reads `count` buffer addresses from a guest array at `array`, one every `stride` bytes with the
/// address at each element's start - a raw `void*[]` (stride 8) for `RegisterBuffers`, or a
/// `SceVideoOutBuffer[]` (stride 32, address in `data` at offset 0) for `RegisterBuffers2`.
///
/// # Safety
///
/// Each element's first eight bytes are under the `orbistoun_mem::guest` contract.
unsafe fn read_buffer_addresses(array: u64, count: usize, stride: u64) -> Vec<u64> {
    (0..count)
        // SAFETY: the caller's contract, per element.
        .map(|i| unsafe { guest::read_u64(array.wrapping_add(i as u64 * stride)) }.unwrap_or(0))
        .collect()
}

/// Records a registered buffer set against `handle`: the addresses from `start_index`, the count
/// on the port, and the shape decoded from `attribute`. The shared body of both register calls.
///
/// A zero pointer, or a block the guest never filled, decodes to a zeroed shape rather than a
/// fault.
fn register_buffer_set(
    handle: u64,
    start_index: u64,
    addresses: &[u64],
    count_field: u64,
    attribute: u64,
    shape: BufferShape,
) -> u64 {
    let start = usize::try_from(start_index)
        .unwrap_or(MAX_REGISTERED_BUFFERS)
        .min(MAX_REGISTERED_BUFFERS);
    match port::with(handle, |p| {
        p.registered = count_field;
        p.attribute = attribute;
        p.shape = shape;
        let end = (start + addresses.len()).min(MAX_REGISTERED_BUFFERS);
        if p.buffers.len() < end {
            p.buffers.resize(end, 0);
        }
        p.buffers[start..end].copy_from_slice(&addresses[..end - start]);
    }) {
        Some(()) => OK,
        None => video_error::INVALID_HANDLE,
    }
}

/// Decodes the shape a guest wrote into an attribute block with
/// [`sceVideoOutSetBufferAttribute2`](video_out_set_buffer_attribute2), at the measured offsets.
/// A zero pointer yields a zeroed [`BufferShape`].
///
/// # Safety
///
/// A non-zero `attribute` is a block under the `orbistoun_mem::guest` contract.
unsafe fn decode_attribute(attribute: u64) -> BufferShape {
    // SAFETY: the caller's contract, here and for each field below; null reads as zero.
    let word = |at: u64| unsafe { guest::read_u32(at) }.unwrap_or(0);
    if attribute == 0 {
        return BufferShape::default();
    }
    BufferShape {
        tiling: word(attribute + 0x4),
        width: word(attribute + 0xc),
        height: word(attribute + 0x10),
        // SAFETY: the caller's contract: the format field at 0x20.
        format: unsafe { guest::read_u64(attribute + 0x20) }.unwrap_or(0),
    }
}

/// `sceVideoOutSetBufferAttribute2(attr, pixelformat, tiling, width, height, option, [dcc_control,
/// dcc_clear_color])`.
///
/// Fills the caller's attribute block at the hardware-measured offsets: tiling at `0x4`, width at
/// `0xc`, height at `0x10`, option at `0x18`, format at `0x20`. It writes only those bytes. The
/// pitch bytes (`0x0`, `0x8`, `0x14`) and anything past the measured 80-byte extent are left
/// untouched. The DCC fields (`0x28`, `0x30`) are the seventh and eighth arguments, on the guest
/// stack past the six captured registers, so they are left as the caller had them.
fn video_out_set_buffer_attribute2(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (attr, pixelformat, tiling, width, height, option) =
        (args[0], args[1], args[2], args[3], args[4], args[5]);
    if attr == 0 {
        return video_error::INVALID_HANDLE;
    }
    let fields = [(0x4, tiling), (0xc, width), (0x10, height)];
    for (offset, value) in fields {
        // SAFETY: a field inside the guest's 80-byte attribute block, by the call's contract.
        unsafe { guest::write_u32(attr + offset, value as u32) };
    }
    // SAFETY: as above.
    unsafe { guest::write_u64(attr + 0x18, option) };
    // SAFETY: as above.
    unsafe { guest::write_u64(attr + 0x20, pixelformat) };
    OK
}

/// `sceVideoOutSubmitFlip(handle, buffer_index, flip_mode, flip_arg)`.
///
/// On hardware a flip is queued and completes at the next vertical blank. There is no scanout
/// here, so a flip completes as soon as it is accepted and the count advances now (D516). A guest
/// polling [`video_out_get_flip_status`] sees it move and proceeds.
///
/// `buffer_index` is recorded as the port's last-flipped index. `flip_mode` is not modelled:
/// when a flip is shown is a property of a scanout that does not exist.
fn video_out_submit_flip(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (handle, buffer_index, flip_arg) = (args[0], args[1], args[3]);
    if port::with(handle, |p| {
        p.flips += 1;
        p.last_flip = Some(buffer_index);
    })
    .is_none()
    {
        return video_error::INVALID_HANDLE;
    }
    // Record which port flipped, so a reader of the presented frame need not guess.
    port::note_flipped(handle);
    // The installed observer is handed the buffer the guest asked to scan out.
    if let Some(observe) = FLIP_OBSERVER.get()
        && let Some((address, shape)) = last_flipped_buffer()
    {
        observe(address, shape);
    }
    // Post the completion to the queues registered for this port, so a guest waiting on a flip
    // event is told.
    //
    // `ORBISTOUN_FLIP_TO_ALL` posts to every queue instead, to ask what a guest blocked on an
    // unfed queue does when its wait completes. It is off by default and recorded as an
    // intervention, so a verdict under it is not a measurement (D227).
    let completion = orbistoun_kernel::sync::PendingEvent {
        ident: handle,
        // The filter identifying a video-out completion is unestablished and left at zero rather
        // than invented (D010).
        filter: 0,
        flags: 0,
        fflags: 0,
        // The caller's flip argument, the only per-flip value the guest supplies. Carrying it in
        // `data` is an assumption.
        data: i64::from_ne_bytes(flip_arg.to_ne_bytes()),
        // Replaced by the registration's own word on delivery.
        udata: 0,
    };
    if std::env::var_os(orbistoun_env::FLIP_TO_ALL.name).is_some() {
        orbistoun_kernel::sync::post_event_everywhere(completion);
        return OK;
    }
    // Routed by the port handle, which is what `sceVideoOutAddFlipEvent` registered.
    orbistoun_kernel::sync::post_event(handle, completion);
    OK
}

/// `sceVideoOutAddFlipEvent(equeue, handle, udata)`.
///
/// Registers a flip completion against an event queue, keyed by the port handle, so
/// [`video_out_submit_flip`] has somewhere to post. Both handles are checked: success for a port
/// or queue orbistoun never issued would promise a delivery that cannot happen.
fn video_out_add_flip_event(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (equeue, handle, udata) = (args[0], args[1], args[2]);
    if port::with(handle, |_| ()).is_none() {
        return video_error::INVALID_HANDLE;
    }
    if orbistoun_kernel::sync::register_event_with_udata(equeue, handle, udata) {
        OK
    } else {
        video_error::INVALID_HANDLE
    }
}

/// `sceVideoOutIsFlipPending(handle)`.
///
/// Always zero for an open port: [`video_out_submit_flip`] completes a flip when it is accepted,
/// so no flip is ever queued and not yet presented (D516). The answer is a count, so a
/// placeholder would read as a huge number of pending flips. A bad handle answers the port error,
/// since zero would tell a guest that a port it does not have is idle.
fn video_out_is_flip_pending(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    match port::with(args[0], |_| ()) {
        // Nothing is ever queued, so nothing is ever pending.
        Some(()) => 0,
        None => video_error::INVALID_HANDLE,
    }
}

/// `sceVideoOutConfigureOutput(handle, ...)`.
///
/// Accepted against a real port and not modelled, like [`video_out_set_flip_rate`]: an output's
/// configuration is a property of a scanout that does not exist here. The handle is checked. It
/// is a setter that writes nothing into caller memory, so success promises nothing unperformed.
fn video_out_configure_output(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    match port::with(args[0], |_| ()) {
        Some(()) => OK,
        None => video_error::INVALID_HANDLE,
    }
}

/// `sceVideoOutSetFlipRate(handle, rate)`.
///
/// With no scanout there is nothing to pace, so the rate is accepted against a real port and not
/// modelled.
fn video_out_set_flip_rate(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    match port::with(args[0], |_| ()) {
        Some(()) => OK,
        None => video_error::INVALID_HANDLE,
    }
}

/// The offset of the completed-flip count in the flip-status structure.
///
/// Zero: the count is the documented first field of `SceVideoOutFlipStatus`, and obSCEne reads a
/// `uint64` from `status[0..8]`. The rest of the structure has no established layout.
const FLIP_COUNT_OFFSET: usize = 0;

/// `sceVideoOutGetFlipStatus(handle, status)`.
///
/// Writes the port's completed-flip count at [`FLIP_COUNT_OFFSET`]; with flips completing on
/// submit, this is the number of flips submitted. Only the count is written: the structure's other
/// fields have no citable offsets (D010), so a caller reading them gets what it left there.
fn video_out_get_flip_status(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (handle, status) = (args[0], args[1]);
    let Some(flips) = port::with(handle, |p| p.flips) else {
        return video_error::INVALID_HANDLE;
    };
    let Ok(at) = usize::try_from(status) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    if at == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    // SAFETY: a guest-supplied status buffer in identity-mapped guest memory, passed for this
    // write; written unaligned because the guest promises no alignment.
    unsafe {
        std::ptr::write_unaligned(
            std::ptr::with_exposed_provenance_mut::<u64>(at + FLIP_COUNT_OFFSET),
            flips,
        );
    }
    OK
}

/// The display resolution this run presents, in pixels (D425).
///
/// 1920x1080, the resolution the hardware brings its framebuffer up at for obSCEne's display
/// runs: a size the hardware drives, not any panel's native size.
const PRESENTED_WIDTH: u32 = 1920;
/// Companion to [`PRESENTED_WIDTH`].
const PRESENTED_HEIGHT: u32 = 1080;

/// `sceVideoOutGetResolutionStatus(handle, status)`.
///
/// Fills the caller's status structure with the resolution this run presents (D425). `width` and
/// `height` are the documented leading two `uint32`s of `SceVideoOutResolutionStatus`, assumed from
/// public documentation: no hardware dump of the structure exists, because obSCEne's display path
/// holds the main output and its second open is refused. The rest of the structure is left as the
/// caller prepared it.
fn video_out_get_resolution_status(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (handle, status) = (args[0], args[1]);
    if port::with(handle, |_| ()).is_none() {
        return video_error::INVALID_HANDLE;
    }
    let Ok(at) = usize::try_from(status) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    if at == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    // SAFETY: a guest-supplied status buffer in identity-mapped guest memory; `width` at its
    // documented offset 0, unaligned because the guest promises no alignment.
    unsafe {
        std::ptr::write_unaligned(
            std::ptr::with_exposed_provenance_mut::<u32>(at),
            PRESENTED_WIDTH,
        );
    }
    // SAFETY: the same buffer, `height` at its documented offset 4, inside the structure.
    unsafe {
        std::ptr::write_unaligned(
            std::ptr::with_exposed_provenance_mut::<u32>(at + 4),
            PRESENTED_HEIGHT,
        );
    }
    OK
}

/// How many flips the guest has had accepted against a real port.
///
/// A submission with a handle this crate never issued is refused and not counted, so reaching a
/// flip means the guest opened an output and registered buffers. It is not a count of frames
/// displayed: nothing scans out, and a flip completes when accepted.
#[must_use]
pub fn flips_accepted() -> u64 {
    port::flips()
}

/// For the port that last completed a flip, the guest address of the buffer it presented and the
/// decoded [`BufferShape`] registered with it - `(address, shape)`.
///
/// The address locates the frame's bytes and the shape gives their extent, format and tiling. The
/// shape is decoded at register time, so no guest pointer is dereferenced here. `None` until a
/// flip is submitted against a port whose flipped index has a registered buffer.
#[must_use]
pub fn last_flipped_buffer() -> Option<(u64, BufferShape)> {
    port::last_flipped_buffer()
}

/// What sees each presented frame: the flipped buffer's guest address and shape, handed over as
/// the guest submits the flip. Installed by whoever can show a frame, so this crate names no
/// display of its own.
pub type FlipObserver = fn(u64, BufferShape);

static FLIP_OBSERVER: std::sync::OnceLock<FlipObserver> = std::sync::OnceLock::new();

/// Installs the observer every flip is shown to. First install wins.
pub fn install_flip_observer(observer: FlipObserver) {
    let _ = FLIP_OBSERVER.set(observer);
}

/// Implementations this crate provides, by symbol name. Names rather than hashes, so the table can
/// be read and checked against the declarations.
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("sceVideoOutOpen", video_out_open),
        ("sceVideoOutClose", video_out_close),
        ("sceVideoOutRegisterBuffers", video_out_register_buffers),
        ("sceVideoOutRegisterBuffers2", video_out_register_buffers2),
        (
            "sceVideoOutSetBufferAttribute2",
            video_out_set_buffer_attribute2,
        ),
        ("sceVideoOutSubmitFlip", video_out_submit_flip),
        ("sceVideoOutAddFlipEvent", video_out_add_flip_event),
        ("sceVideoOutConfigureOutput", video_out_configure_output),
        ("sceVideoOutIsFlipPending", video_out_is_flip_pending),
        ("sceVideoOutSetFlipRate", video_out_set_flip_rate),
        ("sceVideoOutGetFlipStatus", video_out_get_flip_status),
        (
            "sceVideoOutGetResolutionStatus",
            video_out_get_resolution_status,
        ),
    ]
}

#[cfg(test)]
mod tests {

    /// A category denied the scanout is refused with its exact code before a port is opened (D662).
    ///
    /// No title in the corpus reaches this branch, so the test constructs the case.
    #[test]
    fn a_category_without_the_scanout_cannot_open_a_port() {
        use orbistoun_core::category::Category;

        assert_eq!(
            super::refuse_scanout(Category::SystemApp),
            Some(u64::from(orbistoun_core::category::SCANOUT_DENIED)),
            "a system app is refused, with the documented code"
        );
        assert_eq!(
            super::refuse_scanout(Category::BigApp),
            None,
            "a big app is not refused, so the open proceeds"
        );
    }
    use super::{
        BufferShape, GUEST_ARG_REGISTERS, PRESENTED_HEIGHT, PRESENTED_WIDTH, port, video_error,
        video_out_get_flip_status, video_out_get_resolution_status, video_out_is_flip_pending,
        video_out_open, video_out_register_buffers, video_out_register_buffers2,
        video_out_set_buffer_attribute2, video_out_set_flip_rate, video_out_submit_flip,
    };

    fn args(values: [u64; 4]) -> [u64; GUEST_ARG_REGISTERS] {
        let mut a = [0_u64; GUEST_ARG_REGISTERS];
        a[..4].copy_from_slice(&values);
        a
    }

    /// Serialises the tests that submit a flip, since the last flipped port is process-global. A
    /// poisoned lock is recovered rather than cascading a panic.
    fn serial() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The attribute block fills, decodes, and its shape reads back through the flipped buffer.
    ///
    /// Two passes, 1920x1080 tiled and 3840x2160 linear. A byte past the 80 the call writes stays
    /// unchanged.
    #[test]
    fn the_attribute_block_fills_decodes_and_reads_back_its_shape() {
        let _guard = serial();

        // A 256-byte block the guest owns, addressed directly through the identity mapping.
        // Pre-poisoned so an untouched byte is recognisable.
        let mut block = [0x5a_u8; 256];
        let attr = block.as_mut_ptr() as usize as u64;
        let format = 0x8000_0000_0000_0000_u64;

        // Fill: 1920x1080, tiled (0). args[4] height, args[5] option.
        let mut fill = args([attr, format, 0, 1920]);
        fill[4] = 1080;
        assert_eq!(
            video_out_set_buffer_attribute2(&fill),
            0,
            "the attribute block is filled"
        );

        // Bus 5: other tests hold ports on buses 1/3/4/6/7, and the second pass uses bus 8, so the
        // already-open refusal cannot fire in a parallel suite.
        let handle = open_on(5);
        let addresses: [u64; 3] = [0x2_0100_0000, 0x2_0101_0000, 0x2_0102_0000];
        let mut reg = args([handle, 0, addresses.as_ptr() as u64, 3]);
        reg[4] = attr;
        assert_eq!(
            video_out_register_buffers(&reg),
            0,
            "registered with the filled attribute"
        );
        assert_eq!(
            port::with(handle, |p| p.attribute).expect("the port"),
            attr,
            "the attribute pointer is still kept on the port"
        );

        assert_eq!(video_out_submit_flip(&args([handle, 1, 0, 0])), 0);
        assert_eq!(
            super::last_flipped_buffer(),
            Some((
                0x2_0101_0000,
                BufferShape {
                    tiling: 0,
                    width: 1920,
                    height: 1080,
                    format,
                },
            )),
            "the flipped buffer's address and decoded shape read back together"
        );

        // A byte outside the 80 the call writes is unchanged - still the poison it was set to.
        assert_eq!(
            block[0xa0], 0x5a,
            "a byte past the written extent is untouched"
        );

        // Second pass: 3840x2160, linear (1), on a fresh block and port.
        let mut block2 = [0x5a_u8; 256];
        let attr2 = block2.as_mut_ptr() as usize as u64;
        let format2 = 0x1_u64;
        let mut fill2 = args([attr2, format2, 1, 3840]);
        fill2[4] = 2160;
        assert_eq!(video_out_set_buffer_attribute2(&fill2), 0);

        let handle2 = open_on(8);
        let addresses2: [u64; 2] = [0x2_0200_0000, 0x2_0201_0000];
        let mut reg2 = args([handle2, 0, addresses2.as_ptr() as u64, 2]);
        reg2[4] = attr2;
        assert_eq!(video_out_register_buffers(&reg2), 0);
        assert_eq!(video_out_submit_flip(&args([handle2, 0, 0, 0])), 0);
        assert_eq!(
            super::last_flipped_buffer(),
            Some((
                0x2_0200_0000,
                BufferShape {
                    tiling: 1,
                    width: 3840,
                    height: 2160,
                    format: format2,
                },
            )),
            "linear 4K reads back width 3840, height 2160, tiling 1"
        );
    }

    /// RegisterBuffers2 reads addresses from the `SceVideoOutBuffer` struct array, not v1's
    /// arguments.
    ///
    /// arg2 is zero, where v1 keeps its address array; the addresses must come back in order from
    /// the structs' `data` fields.
    #[test]
    fn register_buffers2_reads_addresses_from_the_struct_array() {
        let _guard = serial();
        let handle = open_on(9);
        // Two SceVideoOutBuffer structs laid out as [data, metadata, reserved0, reserved1]; only
        // the data field, the first quadword of each, is read.
        let structs: [u64; 8] = [0x2_0300_0000, 0, 0, 0, 0x2_0301_0000, 0, 0, 0];
        let buffers = structs.as_ptr() as usize as u64;

        let mut a = [0_u64; GUEST_ARG_REGISTERS];
        a[0] = handle; // arg2 stays zero, as the SDK calls it
        a[3] = buffers;
        a[4] = 2;
        // arg5 (attribute) zero: decode yields a zeroed shape without a fault.
        assert_eq!(
            video_out_register_buffers2(&a),
            0,
            "registered through the struct array rather than refused on a zero arg2"
        );
        assert_eq!(
            port::with(handle, |p| p.buffers.clone()).expect("the port"),
            vec![0x2_0300_0000, 0x2_0301_0000],
            "the two data addresses read back in order"
        );
    }

    /// Opens a distinct output so the shared port table cannot make two tests collide on
    /// ownership. `[user, bus, index, param]`; each test picks its own bus.
    fn open_on(bus: u64) -> u64 {
        video_out_open(&args([0, bus, 0, 0]))
    }

    /// Registering buffers stores their addresses in order, a flip records which one, and a null
    /// address array is refused rather than stored as zeros.
    #[test]
    fn registering_buffers_stores_their_addresses_and_a_flip_records_the_index() {
        let _guard = serial();
        let handle = open_on(6);
        assert!(handle >= port::FIRST, "a port opened");

        // Guest memory is identity-mapped, so this array's pointer is a guest address.
        let addresses: [u64; 3] = [0x2_0000_0000, 0x2_0001_0000, 0x2_0002_0000];
        let ptr = addresses.as_ptr() as u64;
        assert_eq!(
            video_out_register_buffers(&args([handle, 0, ptr, 3])),
            0,
            "registering three buffers succeeds"
        );
        assert_eq!(
            port::with(handle, |p| p.buffers.clone()).expect("the port"),
            vec![0x2_0000_0000, 0x2_0001_0000, 0x2_0002_0000],
            "the three addresses are stored in index order"
        );

        // A flip of buffer 2 records that index.
        assert_eq!(video_out_submit_flip(&args([handle, 2, 0, 0])), 0);
        assert_eq!(
            port::with(handle, |p| p.last_flip).expect("the port"),
            Some(2),
            "the flip recorded which buffer it presented"
        );

        // A null address array is refused, not stored as zeros.
        let before = port::with(handle, |p| p.buffers.clone()).expect("the port");
        assert_eq!(
            video_out_register_buffers(&args([handle, 0, 0, 3])),
            video_error::INVALID_HANDLE,
            "a null address array is refused"
        );
        assert_eq!(
            port::with(handle, |p| p.buffers.clone()).expect("the port"),
            before,
            "and it stored nothing"
        );
    }

    /// Nothing is ever pending, including straight after a submit.
    ///
    /// Asserted after a submit as well as before, because the property is that the model completes
    /// on submit, not that nothing was submitted.
    #[test]
    fn nothing_is_ever_pending_because_a_flip_completes_on_submit() {
        let _guard = serial();
        let handle = open_on(4);
        assert!(handle >= port::FIRST, "a port opened");

        assert_eq!(
            video_out_is_flip_pending(&args([handle, 0, 0, 0])),
            0,
            "an idle port has nothing pending"
        );
        assert_eq!(video_out_submit_flip(&args([handle, 0, 1, 0])), 0);
        assert_eq!(
            video_out_is_flip_pending(&args([handle, 0, 0, 0])),
            0,
            "and still nothing after a submit, because the submit completed it"
        );
    }

    /// A handle no port answers is refused, not told it is idle.
    #[test]
    fn a_bad_handle_is_refused_rather_than_reported_idle() {
        assert_eq!(
            video_out_is_flip_pending(&args([0xDEAD_BEEF, 0, 0, 0])),
            video_error::INVALID_HANDLE
        );
    }

    /// A submitted flip completes now, and the status reports the count a guest polls at offset 0.
    #[test]
    fn a_flip_completes_on_submit_and_the_count_is_readable() {
        let _guard = serial();
        let handle = open_on(1);
        assert!(handle >= port::FIRST, "a port opened");

        let mut status = [0_u64; 8];
        let status_ptr = status.as_mut_ptr() as usize as u64;

        assert_eq!(
            video_out_get_flip_status(&args([handle, status_ptr, 0, 0])),
            0
        );
        let before = status[0];

        assert_eq!(
            video_out_submit_flip(&args([handle, 0, 1, 0])),
            0,
            "flip accepted"
        );
        assert_eq!(
            video_out_get_flip_status(&args([handle, status_ptr, 0, 0])),
            0
        );
        assert_eq!(
            status[0],
            before + 1,
            "the completed-flip count advanced by one"
        );
    }

    /// Every flip call refuses a handle that was never opened with the measured video-out code
    /// `0x8029_000b`, not a placeholder a caller reads as a count.
    #[test]
    fn the_flip_calls_refuse_an_unopened_handle_with_the_video_code() {
        let _guard = serial();
        let bogus = port::FIRST + 9999;
        let mut status = [0_u64; 8];
        let status_ptr = status.as_mut_ptr() as usize as u64;
        assert_eq!(
            video_out_set_flip_rate(&args([bogus, 60, 0, 0])),
            video_error::INVALID_HANDLE,
            "the measured video-out bad-handle code, not a kernel placeholder"
        );
        assert_eq!(
            video_out_submit_flip(&args([bogus, 0, 1, 0])),
            video_error::INVALID_HANDLE
        );
        assert_eq!(
            video_out_get_flip_status(&args([bogus, status_ptr, 0, 0])),
            video_error::INVALID_HANDLE
        );
    }

    /// A second open of the same output is refused with the already-open code `0x8029_0009`.
    #[test]
    fn a_second_open_of_a_held_output_is_refused() {
        let first = open_on(7);
        assert!(first >= port::FIRST, "the first open succeeds");
        assert_eq!(
            open_on(7),
            video_error::ALREADY_OPEN,
            "the second open of the same output is refused, not handed another handle"
        );
    }

    /// The resolution status writes the presented width and height at offsets 0 and 4, and an
    /// unopened handle is refused (D425).
    #[test]
    fn the_resolution_status_reports_the_presented_size() {
        let handle = open_on(3);
        let mut status = [0_u32; 8];
        let status_ptr = status.as_mut_ptr() as usize as u64;
        assert_eq!(
            video_out_get_resolution_status(&args([handle, status_ptr, 0, 0])),
            0
        );
        assert_eq!(status[0], PRESENTED_WIDTH, "width at offset 0");
        assert_eq!(status[1], PRESENTED_HEIGHT, "height at offset 4");

        assert_ne!(
            video_out_get_resolution_status(&args([port::FIRST + 9999, status_ptr, 0, 0])),
            0,
            "an unopened handle is refused"
        );
    }
}
