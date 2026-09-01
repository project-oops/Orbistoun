//! Video output HLE - libSceVideoOut.
//!
//! Owns the swapchain and the flip queue: the guest registers buffers, then
//! submits flips and waits on their completion. Getting flip completion wrong is
//! the classic cause of a title that boots, renders one correct frame, and then
//! appears to freeze.
//!
//! Together with `orbistoun-gpu` this is the pair that produces the first visible
//! output, which makes it the natural first milestone after the loader.
//!
//! # Status
//!
//! Open, close and buffer registration; the flip queue as a counter that completes on submit,
//! which is what a headless emulator with no scanout can honestly model - enough for a guest that
//! polls flip completion to proceed rather than hang.

use orbistoun_hle::guest_module;

guest_module! {
    "libSceVideoOut" {
        "sceVideoOutOpen" => 4,
        "sceVideoOutClose" => 1,
        "sceVideoOutRegisterBuffers" => 6,
        "sceVideoOutSubmitFlip" => 4,
        "sceVideoOutSetFlipRate" => 2,
        "sceVideoOutGetFlipStatus" => 2,
        "sceVideoOutGetResolutionStatus" => 2,
        // Confirmed by hash against a real import (D167): the guest was calling this with
        // our unimplemented code as the port to register against.
        "sceVideoOutRegisterBuffers2" => 6,
    }
}

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError, GuestFn};

/// Successful return, as the guest reads it.
const OK: u64 = 0;

/// Video-out error codes, base `0x8029_0000`.
///
/// A family distinct from the kernel's `0x8002_00xx`, and measured on hardware where noted. A
/// video-out call that refuses answers one of these rather than a `GuestError` placeholder -
/// which, for a call whose result a guest tests against zero, would otherwise read as a valid
/// handle or a good status (D125).
mod video_error {
    /// A handle that names no open port. Measured: `080-video/flip-rate-rejects-bad-handle` records
    /// `sceVideoOutSetFlipRate` refusing a bad handle with `0x8029_000b`. Assumed uniform across the
    /// family until each call's own refusal is measured against hardware.
    pub(super) const INVALID_HANDLE: u64 = 0x8029_000b;
    /// The output is already open, so a second open of it is refused rather than handed a second
    /// handle. Measured: obSCEne's display path records `sceVideoOutOpen` of the held main output
    /// answering `0x8029_0009` - the handle-still-held trap (D169).
    pub(super) const ALREADY_OPEN: u64 = 0x8029_0009;
}

/// Display ports the guest can open.
///
/// A handle is an index into this, offset so zero is never a valid one - callers test a
/// video-out handle against zero and negative values, and a handle of zero would be read
/// as "not open" by code that is holding a perfectly good port.
mod port {
    use std::sync::{Mutex, OnceLock};

    /// One display port.
    ///
    /// The bus and index are recorded because **ownership reads them**: a console refuses a second
    /// open of an output that is already open, and answering that needs to know which output a
    /// handle stands for. An earlier version recorded the bus for trace-readability alone and
    /// dropped it when nothing read it; this brings it back for a reader that exists.
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
        /// How many flips have completed - the count `sceVideoOutGetFlipStatus` reports, and what
        /// a guest polls to learn a submitted flip has been picked up.
        pub flips: u64,
    }

    /// Every port ever opened. Index plus [`FIRST`] is the handle; a closed one stays, so handles
    /// are never renumbered (D169's reasoning, unchanged).
    fn table() -> &'static Mutex<Vec<Port>> {
        static TABLE: OnceLock<Mutex<Vec<Port>>> = OnceLock::new();
        TABLE.get_or_init(|| Mutex::new(Vec::new()))
    }

    /// The first handle handed out.
    ///
    /// Small and positive. Unlike a thread or file handle this one is **not** an address:
    /// the guest passes it back as an integer and compares it against zero, and nothing
    /// observed dereferences it (D169).
    pub(super) const FIRST: u64 = 1;

    /// Why an open did not hand back a handle.
    pub(super) enum OpenFailure {
        /// The output is already open; a console refuses a second open of it.
        AlreadyOpen,
        /// The table could not be reached.
        Unavailable,
    }

    /// Opens an output, or says why not.
    ///
    /// An output already open is **refused** rather than handed a second handle, which is what a
    /// console does (D169) - so a guest that opens the same output twice sees the second refused,
    /// exactly as it would on hardware.
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
/// # Why this is implemented before anything can display
///
/// Not to show a frame - nothing here draws. To stop the guest carrying an error code
/// around as a display handle.
///
/// Unimplemented, this answered `0x7FFF0001`, and the trace showed the guest passing that
/// straight into `sceVideoOutRegisterBuffers2` as the port to register against. That is
/// D125's failure with a display attached: our own placeholder travelling through the
/// guest as data, arriving somewhere with no relation to where it came from.
///
/// A small positive integer is the right shape here, and deliberately **not** an address.
/// Thread, lock and file handles are addresses because the guest dereferences them
/// (D151); a video-out handle is compared against zero and passed back, and nothing
/// observed reads through it.
///
/// # A second open of the same output is refused
///
/// `(bus, index)` names an output, and opening one that is already open answers
/// [`video_error::ALREADY_OPEN`] rather than a fresh handle - which is what a console does, and
/// what a guest that already holds the output expects. It is why obSCEne's resolution probe, which
/// opens the main output a second time while the display path still holds it, is refused on
/// hardware and now here too (D169).
fn video_out_open(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (bus, index) = (args[1], args[2]);
    match port::open(bus, index) {
        Ok(handle) => handle,
        Err(port::OpenFailure::AlreadyOpen) => video_error::ALREADY_OPEN,
        Err(port::OpenFailure::Unavailable) => u64::from(GuestError::Unimplemented.as_raw()),
    }
}

/// `sceVideoOutClose(handle)`.
///
/// The port is left in the table rather than removed: handles are indices, and removing
/// one would either renumber the rest or leave a hole that a later open would reuse -
/// and a reused display handle is a bug nobody would look for.
fn video_out_close(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // Closed, so its output can be opened again - the ownership check in `port::open` reads this.
    match port::with(args[0], |p| {
        p.registered = 0;
        p.open = false;
    }) {
        Some(()) => OK,
        None => video_error::INVALID_HANDLE,
    }
}

/// `sceVideoOutRegisterBuffers(handle, index, addresses, count, attribute)`.
///
/// Records how many buffers a port has been given and answers success. **Nothing is done
/// with the addresses**, which is the honest state: there is no output surface, so a
/// buffer registered here is a buffer nobody will read.
///
/// Answering success rather than refusing is a deliberate choice and a reversible one. A
/// guest that cannot register buffers stops setting up its display; one that believes it
/// can proceeds to submit flips, which is where the GPU layer will eventually be reached.
/// Getting further is the point, and the alternative is a wall with nothing behind it.
fn video_out_register_buffers(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (handle, count) = (args[0], args[3]);
    match port::with(handle, |p| p.registered = count) {
        Some(()) => OK,
        None => video_error::INVALID_HANDLE,
    }
}

/// `sceVideoOutSubmitFlip(handle, buffer_index, flip_mode, flip_arg)`.
///
/// # A flip completes the instant it is submitted
///
/// On hardware a flip is *queued*, and its completion count moves when a presenter picks it up at
/// the next vertical blank. There is no presenter and no vblank here - nothing scans a buffer out -
/// so the honest model for a headless run is that a flip completes as soon as it is accepted: the
/// count advances now. A guest that submits a flip and then polls [`video_out_get_flip_status`] for
/// the count to move past what it was sees it move, and proceeds, rather than spinning on a frame
/// that a real display would have shown and this one never will.
///
/// `buffer_index`, `flip_mode` and `flip_arg` are accepted and not modelled: which buffer is shown
/// and when are properties of a scanout that does not exist, and inventing a `flip_arg` echo nothing
/// reads would be state carried for no reader. What advancing the count buys - the guest getting
/// past its present loop - is the whole point (as with buffer registration above).
fn video_out_submit_flip(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    match port::with(args[0], |p| p.flips += 1) {
        Some(()) => OK,
        None => video_error::INVALID_HANDLE,
    }
}

/// `sceVideoOutSetFlipRate(handle, rate)`.
///
/// The rate at which queued flips are presented. With no scanout there is nothing to pace, so the
/// rate is accepted against a real port and not modelled - a guest that sets it goes on to submit
/// flips, which is where getting further leads.
fn video_out_set_flip_rate(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    match port::with(args[0], |_| ()) {
        Some(()) => OK,
        None => video_error::INVALID_HANDLE,
    }
}

/// The offset of the completed-flip count in the flip-status structure.
///
/// Zero: it is the structure's first field, and a guest reads it there - obSCEne assembles a
/// `uint64` from `status[0..8]`. It is the one field with a citable position (the count is the
/// documented head of `SceVideoOutFlipStatus`); the rest of the structure is not modelled, because
/// no lawful source here establishes its layout and nothing observed reads past the count.
const FLIP_COUNT_OFFSET: usize = 0;

/// `sceVideoOutGetFlipStatus(handle, status)`.
///
/// Writes the port's completed-flip count into the caller's status structure, at
/// [`FLIP_COUNT_OFFSET`]. That is the field a guest polls to learn a submitted flip has been picked
/// up; with [`video_out_submit_flip`] completing a flip on submit, the count a caller reads back is
/// the number of flips it has submitted.
///
/// **Only the count is written.** The structure has more fields on hardware - a timestamp, the
/// current buffer - but no citable source here gives their offsets, and writing a guessed layout is
/// the invention this project refuses (principle 3). A caller reading a field this does not write
/// gets whatever it left there, which for obSCEne is the zero it cleared the buffer to.
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
    // SAFETY: a guest-supplied status buffer under the identity mapping (D014), which the guest
    // passed for exactly this write; written unaligned because the guest promises no alignment
    // (obSCEne reads the field back a byte at a time for the same reason).
    unsafe {
        std::ptr::write_unaligned(
            std::ptr::with_exposed_provenance_mut::<u64>(at + FLIP_COUNT_OFFSET),
            flips,
        );
    }
    OK
}

/// The display resolution this run presents, in pixels.
///
/// 1080p, which is what obSCEne itself renders at and calls "the resolution every output supports";
/// its hardware runs bring a `1920x1080` framebuffer up and present a frame through it. So this is a
/// size the console demonstrably drives, not a guess - a concrete target a title can lay a render
/// buffer out against, without claiming to be any particular panel's native size. Held as constants
/// rather than a machine-profile field for now, because one resolution is enough to get a title past
/// its display setup; it moves to the profile the moment a run needs a different one, the way the
/// firmware and software versions already do.
const PRESENTED_WIDTH: u32 = 1920;
/// Companion to [`PRESENTED_WIDTH`].
const PRESENTED_HEIGHT: u32 = 1080;

/// `sceVideoOutGetResolutionStatus(handle, status)`.
///
/// Fills the caller's status structure with the resolution this run presents: a title reads it to
/// size a render target, and obSCEne dumps it as a layout probe.
///
/// # The value is corroborated; the structure layout is assumed
///
/// obSCEne's `130-layout/resolution-status` skips on every hardware run on record - but *not*
/// because the console is headless. The display comes up (`OBS|display|ready|1920x1080`) and a frame
/// reaches it; the test skips because obSCEne's own display path already opened and holds the main
/// output, so the test's *second* `sceVideoOutOpen` is refused (`0x8029_0009`) and it never reaches
/// this call. So there is no byte dump of the structure to hold a layout against. `width` and
/// `height` are the documented leading two `uint32`s of `SceVideoOutResolutionStatus` in the open
/// homebrew SDKs, and those are the fields a title reads; the rest of the structure has no lawful
/// layout here and is left as the caller prepared it, exactly as [`video_out_get_flip_status`]
/// writes only the count. The `1920x1080` value is corroborated by the hardware display header; the
/// field offsets remain `assumed` from a public document until a run that does not already hold the
/// output can dump the real structure.
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
    // SAFETY: a guest-supplied status buffer under the identity mapping (D014); `width` is written
    // at its documented offset 0, unaligned because the guest promises no alignment.
    unsafe {
        std::ptr::write_unaligned(
            std::ptr::with_exposed_provenance_mut::<u32>(at),
            PRESENTED_WIDTH,
        );
    }
    // SAFETY: the same buffer, `height` at its documented offset 4 - inside the structure the guest
    // passed, one field past the width just written.
    unsafe {
        std::ptr::write_unaligned(
            std::ptr::with_exposed_provenance_mut::<u32>(at + 4),
            PRESENTED_HEIGHT,
        );
    }
    OK
}

/// Implementations this crate provides, by symbol name.
///
/// Names rather than hashes: the hash is derived from the name, so a table written in
/// hashes could not be read by a person or checked against the declarations above.
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("sceVideoOutOpen", video_out_open),
        ("sceVideoOutClose", video_out_close),
        ("sceVideoOutRegisterBuffers", video_out_register_buffers),
        ("sceVideoOutRegisterBuffers2", video_out_register_buffers),
        ("sceVideoOutSubmitFlip", video_out_submit_flip),
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
    use super::{
        GUEST_ARG_REGISTERS, PRESENTED_HEIGHT, PRESENTED_WIDTH, port, video_error,
        video_out_get_flip_status, video_out_get_resolution_status, video_out_open,
        video_out_set_flip_rate, video_out_submit_flip,
    };

    fn args(values: [u64; 4]) -> [u64; GUEST_ARG_REGISTERS] {
        let mut a = [0_u64; GUEST_ARG_REGISTERS];
        a[..4].copy_from_slice(&values);
        a
    }

    /// Opens a distinct output so the shared, process-wide port table cannot make two tests
    /// collide on ownership. `[user, bus, index, param]`; each test picks its own bus.
    fn open_on(bus: u64) -> u64 {
        video_out_open(&args([0, bus, 0, 0]))
    }

    /// **A submitted flip completes now, and the status reports the count a guest polls.**
    ///
    /// This is the whole reason the section exists: a guest submits a flip and waits for the count
    /// to move past what it was. If it never moves, the guest spins on a frame that will never be
    /// shown - which is what an unimplemented `GetFlipStatus` did, and where obSCEne's video run
    /// faulted. Asserted through the count a caller reads back, at offset 0, because that is the
    /// field obSCEne assembles from `status[0..8]`.
    #[test]
    fn a_flip_completes_on_submit_and_the_count_is_readable() {
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

    /// **Every flip call refuses a handle that was never opened with the video-out code**, not a
    /// placeholder a caller reads as a count. `080-video/flip-rate-rejects-bad-handle` measured the
    /// console answering `0x8029_000b`; a `GuestError` placeholder (`0x7fff_0003`) both reads wrong
    /// and refuses with the wrong code, the D125 shape this crate exists to avoid.
    #[test]
    fn the_flip_calls_refuse_an_unopened_handle_with_the_video_code() {
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

    /// **A second open of the same output is refused with the console's already-open code.**
    ///
    /// obSCEne's display path holds the main output, and its resolution probe opens it again; on
    /// hardware that second open is refused (`0x8029_0009`) and the probe skips. Without this,
    /// orbistoun handed out a second handle and the probe ran where hardware could not (D169, D425).
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

    /// **The resolution status writes the presented width and height at their documented offsets.**
    ///
    /// The two fields a title reads to size a render target, at 0 and 4. Assumed from public docs -
    /// no hardware run has ever reached this call - so the guard is only that the two known fields
    /// are what this run presents, not a claim about the rest of the structure. An unopened handle
    /// is refused, not answered with a placeholder that reads as a dimension.
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
