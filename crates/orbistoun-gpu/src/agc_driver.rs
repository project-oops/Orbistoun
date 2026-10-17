//! `libSceAgcDriver` - the submission side of the current generation's graphics interface.
//!
//! Separate from [`super::agc`] because the platform separates them: a guest builds command
//! buffers with `libSceAgc` and hands them over with `libSceAgcDriver`, two distinct libraries in
//! an import table. Names come from real import tables; arities are unestablished, as
//! [`super::agc`] states.

use crate::cp;
use crate::pipeline::{GuestMemory, Pipeline, Queue, Submission, SubmissionReport};
use crate::registers::{ColourTarget, ComponentSwap};
use crate::tiling;
use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};
use orbistoun_hle::guest_module;
use orbistoun_translate::{Fidelity, Strategy, Width};
use std::sync::{Arc, Mutex, OnceLock};

guest_module! {
    "libSceAgcDriver" {
        "sceAgcDriverAddEqEvent" => 6,
        "sceAgcDriverCreateQueue" => 3,
        // (queue): refused as measured.
        "sceAgcDriverDestroyQueue" => 1,
        "sceAgcDriverGetDefaultOwner" => 6,
        // (event): a kevent `sceKernelWaitEqueue` delivered.
        "sceAgcDriverGetEqContextId" => 1,
        "sceAgcDriverGetEqEventType" => 1,
        "sceAgcDriverGetResourceRegistrationMaxNameLength" => 6,
        "sceAgcDriverInitResourceRegistration" => 6,
        "sceAgcDriverQueryResourceRegistrationUserMemoryRequirements" => 6,
        "sceAgcDriverRegisterDefaultOwner" => 6,
        "sceAgcDriverRegisterOwner" => 6,
        "sceAgcDriverRegisterResource" => 6,
        "sceAgcDriverSetHsOffchipParam" => 6,
        "sceAgcDriverSetTFRing" => 6,
        "sceAgcDriverSubmitAcb" => 6,
        "sceAgcDriverSubmitCommandBuffer" => 2,
        "sceAgcDriverSubmitDcb" => 6,
    }
}

/// `sceAgcDriverCreateQueue(type, out_queue, flags)`.
///
/// Returns `0`, the measured success code for the compute queue (`type` 3) and the graphics queue
/// (`type` 0) in obSCEne's `166-agc/driver-create-queue` and `166-agc/primitive-draw`.
///
/// It writes a queue handle into `*out_queue`: guests gate their hardware path on a non-null
/// queue. The handle is orbistoun's own opaque object at its own address, like every handle this
/// project hands out, not an invented hardware pointer. A submit reads the descriptor, never the
/// queue; the object's bytes are the header obSCEne measured, so a guest that validates the
/// handle finds a real object.
fn create_queue(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let out_queue = args[1];
    if out_queue != 0
        && let Ok(dest) = usize::try_from(out_queue)
    {
        let handle = queue_object();
        // SAFETY: `out_queue` is the guest's own identity-mapped `void**`, the stack local it
        // passed for the answer, so the eight-byte handle written there is in bounds and owned by
        // the guest for the call.
        unsafe {
            std::ptr::write_unaligned(std::ptr::with_exposed_provenance_mut::<u64>(dest), handle);
        }
    }
    0
}

/// What `sceAgcDriverDestroyQueue` answered for a queue `sceAgcDriverCreateQueue` had just made, of
/// each of the types 0 to 4 (`166-agc/driver-create-queue`, `driver-queue-types`,
/// `20261009-151440-eboot.obs.log`).
const DESTROY_QUEUE_REFUSED: u64 = 0x8a6d_0003;

/// `sceAgcDriverDestroyQueue(queue)`: refused with `0x8a6d0003`, as the console refused every queue
/// it was handed; the queue stays usable.
fn destroy_queue(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    DESTROY_QUEUE_REFUSED
}

/// The opaque queue object orbistoun hands a guest, one per process, its address written into
/// `*out_queue` by [`create_queue`].
///
/// A single leaked block: the guest keeps the pointer for as long as it runs. Filled with the
/// header obSCEne measured for `166-agc/driver-create-queue` (`38 00 00 00 03 00 00 00 00 00 02
/// 00`), so a guest that reads it finds measured bytes rather than zeros.
fn queue_object() -> u64 {
    static QUEUE: OnceLock<u64> = OnceLock::new();
    *QUEUE.get_or_init(|| {
        let mut object = vec![0u8; 64];
        object[..12].copy_from_slice(&[0x38, 0, 0, 0, 0x03, 0, 0, 0, 0, 0, 0x02, 0]);
        Box::leak(object.into_boxed_slice()).as_ptr() as usize as u64
    })
}

/// `SCE_AGC_ERROR_RESOURCE_REGISTRATION_NOT_SUPPORTED`.
///
/// The resource-registration subsystem is a stub on the hardware: `sceAgcDriverRegisterOwner`,
/// `RegisterResource` and `InitResourceRegistration` return `0x8a6c9018` and change no byte of
/// their caller buffers. orbistoun returns the same measured value, so the guest gets the "not
/// supported" the hardware gives it.
const RESOURCE_REGISTRATION_NOT_SUPPORTED: u64 = 0x8a6c_9018;

/// `sceAgcDriverGetDefaultOwner(..)` and `sceAgcDriverGetResourceRegistrationMaxNameLength(..)`:
/// the subsystem's two queries, taken to answer its measured `0x8a6c9018` and write nothing, as
/// its three measured calls do. Assumed from those siblings (REQ-cn01 asks).
fn resource_registration_query(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    RESOURCE_REGISTRATION_NOT_SUPPORTED
}

/// `sceAgcDriverRegisterOwner(owner_buf)` - stub, returns `0x8a6c9018`, writes nothing.
fn register_owner(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    RESOURCE_REGISTRATION_NOT_SUPPORTED
}

/// `sceAgcDriverRegisterResource(res, ...)` - stub, returns `0x8a6c9018`, writes nothing.
fn register_resource(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    RESOURCE_REGISTRATION_NOT_SUPPORTED
}

/// Registers the driver's end-of-pipe event `id` with `udata` against an event queue, answering
/// whether the queue exists - installed by the worker, since this crate does not reach the kernel.
pub type QueueRegistration = fn(u64, u64, u64) -> bool;

fn queue_registration() -> &'static OnceLock<QueueRegistration> {
    static REGISTER: OnceLock<QueueRegistration> = OnceLock::new();
    &REGISTER
}

/// Installs what `sceAgcDriverAddEqEvent` registers through. First install wins.
pub fn install_queue_registration(register: QueueRegistration) {
    let _ = queue_registration().set(register);
}

/// `SCE_KERNEL_ERROR_EBADF`: what `sceAgcDriverAddEqEvent` answers for a handle that names no
/// event queue (obSCEne REQ-20260928T1338Z-5b78, `add-eq-bad-queue`).
const BAD_QUEUE: u64 = 0x8002_0009;

/// `sceAgcDriverAddEqEvent(queue, id, udata)`: registers the driver's end-of-pipe event against a
/// guest event queue, which every interrupting release then posts to (D762). Measured (obSCEne
/// REQ-20260928T1338Z-5b78): `0` for a queue the guest created, with the second argument `0` or
/// `1`; `0x80020009` for a handle that names no queue.
fn add_eq_event(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    match queue_registration().get() {
        Some(register) if register(args[0], args[1], args[2]) => 0,
        Some(_) => BAD_QUEUE,
        None => u64::from(orbistoun_core::GuestError::Unimplemented.as_raw()),
    }
}

/// Where a delivered kevent keeps its `data`: after `ident` (8 bytes), `filter`, `flags` and
/// `fflags` (8 together).
const KEVENT_DATA: u64 = 0x10;

/// A delivered kevent's `data`, or `None` for an event the guest cannot hand over.
fn kevent_data(event: u64) -> Option<u64> {
    if event == 0 {
        return None;
    }
    // SAFETY: the guest's kevent, read through the checked accessor.
    unsafe { orbistoun_mem::guest::read_u64(event + KEVENT_DATA) }
}

/// `sceAgcDriverGetEqEventType(event)`: the kevent's `data`, all 64 bits (obSCEne
/// REQ-20261013T1000Z-eq01, `166-agc/driver-add-eq-event`, `report-20261014-004800.log` 9119-9180:
/// `0xff00` for a release under context 0, `0x101ff00` under `0x101`, and every synthetic `data`
/// as it was). A null event is unmeasured and answers the placeholder.
fn get_eq_event_type(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    kevent_data(args[0])
        .unwrap_or_else(|| u64::from(orbistoun_core::GuestError::Unimplemented.as_raw()))
}

/// `sceAgcDriverGetEqContextId(event)`: the kevent's `data` above its low sixteen bits, the
/// release's context id as `post_end_of_pipe` writes it (`-eq01`: `0x101` for `0x101ff00`,
/// `0x8000101` for `0x8000101ff00`, `0xdead` for `0xdeadbeef`, 0 for 0). PPSA25872's end-of-pipe
/// thread indexes a table with its low three bits. A null event is unmeasured and answers the
/// placeholder.
fn get_eq_context_id(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    kevent_data(args[0]).map_or_else(
        || u64::from(orbistoun_core::GuestError::Unimplemented.as_raw()),
        |data| data >> 16,
    )
}

/// `sceAgcDriverSetHsOffchipParam(...)`: sets the hull-shader off-chip parameter, the driver's
/// `VGT_HS_OFFCHIP_PARAM` (Mesa `ac_cmdbuf_cp.c:374`). Answers `0` for PPSA02664's arguments
/// `(0, 0x1ff, 0x7400, 0, 3, 0x1e44)` and for all zeros (obSCEne REQ-20260927T1130Z-b3c2). It
/// writes no guest memory; the register only shapes tessellation, which draws here do not reach.
fn set_hs_offchip_param(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    0
}

/// `sceAgcDriverSetTFRing(ring, size, ring >> 32, base, ..)`: sets the tessellation-factor ring,
/// answering 0 and writing nothing - in PPSA04263's shape `(B + 0x200, 0x3fff8, B >> 32, B, ..)`,
/// with size `0x40000` at `B`, on a repeat, and with every argument zero (`166-agc/driver-set-tf-ring`,
/// `20261009-215800-eboot.log`; the census). The ring only feeds tessellation, which draws here do
/// not reach.
fn set_tf_ring(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    0
}

/// `sceAgcDriverInitResourceRegistration(...)` - stub, returns `0x8a6c9018`.
fn init_resource_registration(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    RESOURCE_REGISTRATION_NOT_SUPPORTED
}

/// `sceAgcDriverQueryResourceRegistrationUserMemoryRequirements(...)` - stub, returns `0x8a6c9018`.
///
/// The hardware leaves the caller's size sentinel at `0`, and a guest that gets the error does not
/// read the size, so nothing is written.
fn query_resource_registration_user_memory_requirements(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    RESOURCE_REGISTRATION_NOT_SUPPORTED
}

/// The measured `rc-submit`: `0x0` on every `166-agc/driver-submit-*` check and on the
/// `primitive-draw` submission.
const SUBMIT_OK: u64 = 0x0;

/// A ceiling on how many dwords a submit reads out of guest memory, so a wild `size` field in the
/// descriptor cannot walk the read into unmapped memory. Generous (16 MiB): it refuses an absurd
/// descriptor, not a real buffer.
const MAX_DCB_DWORDS: u32 = 1 << 22;

/// The guest's readable memory regions (`start`, `end` half-open), set by the worker before a run.
fn guest_regions() -> &'static Mutex<Vec<(u64, u64)>> {
    static REGIONS: OnceLock<Mutex<Vec<(u64, u64)>>> = OnceLock::new();
    REGIONS.get_or_init(|| Mutex::new(Vec::new()))
}

/// Record regions a submit may read from beside the live lookup - guest virtual spans, which a
/// harness holding its own memory sets.
///
/// A shader address inside a region is read and counted resolved; one outside reads `None` and is
/// counted unresolved (D130). Neither faults the host. Empty (the default), only the live lookup
/// resolves.
pub fn set_guest_regions(regions: Vec<(u64, u64)>) {
    if let Ok(mut slot) = guest_regions().lock() {
        *slot = regions;
    }
}

/// Whether a submit would read `[address, address + length)` as guest memory: inside a region set at
/// entry, or one the installed live lookup vouches for.
#[must_use]
pub fn guest_readable(address: u64, length: usize) -> bool {
    MappedRegions::current().contains(address, length)
}

/// Whether a range is readable guest memory as the run stands now - installed by the worker, which
/// owns the kernel's mapping tables this crate does not depend on.
type RegionLookup = fn(u64, u64) -> bool;

fn region_lookup() -> &'static OnceLock<RegionLookup> {
    static LOOKUP: OnceLock<RegionLookup> = OnceLock::new();
    &LOOKUP
}

/// Installs the live readability check a submit consults beside the regions set at entry.
///
/// The entry list is taken before the guest runs, and a GL context maps its command buffer, fence
/// and render targets afterwards; this serves any range the guest mapped before the submit. First
/// install wins.
pub fn install_region_lookup(lookup: RegionLookup) {
    let _ = region_lookup().set(lookup);
}

fn write_lookup() -> &'static OnceLock<RegionLookup> {
    static LOOKUP: OnceLock<RegionLookup> = OnceLock::new();
    &LOOKUP
}

/// Installs the live writability check the command processor's memory work consults before it
/// writes guest memory (a fill, a copy, a fence). Nothing is written until one is installed.
pub fn install_write_lookup(lookup: RegionLookup) {
    let _ = write_lookup().set(lookup);
}

/// Carries out a submission's draws over its colour target, given the target's contents before
/// them as linear `Rgba8` words (red in the low byte). Answers whether it carried out every draw
/// exactly; the frame they leave is kept, and read with the installed [`FrameReader`] when it is
/// written back (D714).
///
/// Installed by the worker, which owns the graphics device this crate does not depend on.
/// [`Before::Held`] means the target holds exactly the frame the executor last drew or was handed,
/// so it starts from what it already has.
pub type DrawExecutor = fn(&Submission, Before<'_>) -> bool;

/// What a drawer is handed of a target's contents before its draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Before<'a> {
    /// The drawer already holds them: nothing wrote the target since it last drew or was handed it.
    Held,
    /// These, as linear `Rgba8` words.
    Words(&'a [u32]),
    /// This linear `Rgba8` word in every pixel - a clear. One word, so a drawer can fill its target
    /// on the device rather than be handed megabytes of the same four bytes.
    Uniform(u32),
}

impl Before<'_> {
    /// The contents as `pixels` linear `Rgba8` words - a uniform word written out that many times -
    /// or `None` when the drawer holds them already.
    #[must_use]
    pub fn to_words(self, pixels: usize) -> Option<Vec<u32>> {
        match self {
            Self::Held => None,
            Self::Words(words) => Some(words.to_vec()),
            Self::Uniform(word) => Some(vec![word; pixels]),
        }
    }
}

fn draw_executor() -> &'static OnceLock<DrawExecutor> {
    static EXECUTOR: OnceLock<DrawExecutor> = OnceLock::new();
    &EXECUTOR
}

/// Installs the executor a submit carries out its draws with. Without one, a draw stops the command
/// processor. First install wins.
pub fn install_draw_executor(executor: DrawExecutor) {
    let _ = draw_executor().set(executor);
}

/// Seeds the colour attachments after the first before a submission's draws are carried out
/// (D825): each by the id `SetRenderTargets` names it by and its extent, with what it holds as
/// linear `Rgba8` words. `false` when the device could not take them.
pub type MoreTargetsSeeder = fn(&[(crate::ResourceId, (u32, u32), &[u32])]) -> bool;

/// What a colour attachment after the first holds once the draws ran, as linear `Rgba8` words, by
/// its id (D825); `None` when it could not be read.
pub type TargetReader = fn(crate::ResourceId) -> Option<Vec<u32>>;

fn more_targets_io() -> &'static OnceLock<(MoreTargetsSeeder, TargetReader)> {
    static IO: OnceLock<(MoreTargetsSeeder, TargetReader)> = OnceLock::new();
    &IO
}

/// Installs how the colour attachments after the first reach the device and come back (D825).
/// Without them a submission with more than one is refused. First install wins.
pub fn install_more_targets(seed: MoreTargetsSeeder, read: TargetReader) {
    let _ = more_targets_io().set((seed, read));
}

/// A colour attachment after the first that its draws can be carried out into and written back to
/// exactly, by the rules [`writable_target`] holds the first to: a modelled layout for its tiling
/// at its element's size, a format a frame can be written back in, and no compression. It must
/// also have the first's extent, the one the device's render pass covers, and not be the first.
fn writable_more_target(
    more: &crate::pipeline::MoreColourTarget,
    first: ColourTarget,
) -> Option<(ColourTarget, ComponentSwap)> {
    let target = more.target?;
    let format = more.format?;
    let bytes_per_texel = format.written_back_bytes()?;
    (more.dcc.is_none()
        && (target.width, target.height) == (first.width, first.height)
        && target.base != first.base
        && more
            .tiling
            .and_then(tiling::SurfaceLayout::of)
            .and_then(|layout| layout.at_bytes_per_texel(bytes_per_texel))
            == Some(target.layout)
        && target.layout.models(target.pipe_bank_xor))
    .then_some((target, format.swap))
}

/// The one colour target a submission's draws can be carried out into and written back to exactly:
/// a single base, a single resident target of the same extent, a modelled layout for its tiling at
/// its element's size, and either an `8_8_8_8` `UNORM` element in an order `CB_COLOR0_INFO` names
/// or an `8` `UNORM` one holding red. Anything else is `None`, and the draws stay unexecuted.
fn writable_target(submission: &Submission) -> Result<(ColourTarget, ComponentSwap), String> {
    let target = submission
        .colour_target
        .ok_or("no colour target is placed")?;
    let format = submission
        .colour_target_format
        .ok_or("its format was never written")?;
    let one_target = submission.colour_target_bases == 1
        && submission.targets.len() == 1
        && submission
            .targets
            .values()
            .all(|extent| (extent.width, extent.height) == (target.width, target.height));
    if !one_target {
        return Err(format!(
            "it draws into {} base(s) and {} extent(s), and one target is written back",
            submission.colour_target_bases,
            submission.targets.len()
        ));
    }
    let bytes_per_texel = format.written_back_bytes().ok_or_else(|| {
        format!(
            "its format, COLOR code {} with number type {} and {:?} swap, is not one a frame is written back into",
            format.format, format.number_type, format.swap
        )
    })?;
    let tiled = submission
        .colour_target_tiling
        .and_then(tiling::SurfaceLayout::of)
        .and_then(|layout| layout.at_bytes_per_texel(bytes_per_texel));
    if tiled != Some(target.layout) {
        return Err(format!(
            "its tiling {:?} at {bytes_per_texel} bytes a texel is not its placed layout {:?}",
            submission.colour_target_tiling, target.layout
        ));
    }
    if !target.layout.models(target.pipe_bank_xor) {
        return Err(format!(
            "its pipe-bank XOR {} is not modelled in its layout {:?}",
            target.pipe_bank_xor, target.layout
        ));
    }
    Ok((target, format.swap))
}

/// A texel as `target` holds it in memory - a word in its byte order, or a byte - as the linear
/// `Rgba8` word a drawer works in. A one-byte texel is red, with green and blue zero and alpha
/// one: the drawer writes red alone ([`crate::RenderCommand::SetWriteMask`]), so alpha stays one,
/// which is what blending reads as a one-channel target's destination alpha.
const fn loaded(texel: u32, target: ColourTarget, swap: ComponentSwap) -> u32 {
    match target.layout.texel_log2() {
        // An alpha target's byte is the frame's alpha, the colour channels zero (D854).
        0 if matches!(swap, ComponentSwap::Other(3)) => (texel & 0xFF) << 24,
        0 => 0xFF00_0000 | (texel & 0xFF),
        // A two-byte texel is one 16-bit integer, drawn as itself through a 32-bit integer
        // attachment (D852).
        1 => texel & 0xFFFF,
        _ => swapped(texel, swap),
    }
}

/// [`loaded`]'s inverse: a drawer's linear `Rgba8` word as `target` holds it in memory.
const fn stored(word: u32, target: ColourTarget, swap: ComponentSwap) -> u32 {
    match target.layout.texel_log2() {
        0 if matches!(swap, ComponentSwap::Other(3)) => word >> 24,
        0 => word & 0xFF,
        1 => word & 0xFFFF,
        _ => swapped(word, swap),
    }
}

/// Moves a word between the target's memory order and `Rgba8` order - its own inverse, since
/// `SWAP_ALT` exchanges the first and third bytes and `SWAP_STD` exchanges nothing.
const fn swapped(word: u32, swap: ComponentSwap) -> u32 {
    match swap {
        ComponentSwap::Alternate => {
            (word & 0xff00_ff00) | ((word & 0x00ff_0000) >> 16) | ((word & 0x0000_00ff) << 16)
        }
        _ => word,
    }
}

/// Guest memory as the command processor sees it: reads through [`MappedRegions`], writes only
/// where the installed write lookup vouches for the whole range - and the submission whose draws
/// [`cp::CpMemory::run_draws`] carries out.
struct GuestCp<'a> {
    memory: MappedRegions,
    /// `None` for a write-back outside any submission - at a flip (D714).
    submission: Option<&'a Submission>,
}

/// The stream as one segment's draws see it (D729): every packet up to the segment's last draw,
/// with each draw and each dispatch before its first replaced by a `NOP` of the same length. Every
/// register write before the segment still runs, so its draws see the register state the stream
/// gives them; no earlier segment's draw is drawn again, no dispatch the command processor already
/// carried out is dispatched again, and nothing after the segment is read.
fn segment_stream(segment: &cp::DrawSegment<'_>) -> Vec<u8> {
    let end = (segment.end as usize).min(segment.stream.len());
    let mut bytes = segment.stream[..end].to_vec();
    for packet in &crate::packet::walk(&bytes).packets {
        let crate::packet::PacketKind::Command { opcode } = packet.kind else {
            continue;
        };
        let dispatch = opcode == crate::packet::build::DISPATCH_DIRECT;
        if packet.offset >= segment.first || !(cp::is_draw(opcode) || dispatch) {
            continue;
        }
        let header = crate::packet::build::command_header(
            crate::packet::build::measured::NOP,
            packet.body_length() / 4,
        );
        let at = packet.offset as usize;
        bytes[at..at + 4].copy_from_slice(&header.to_le_bytes());
    }
    bytes
}

/// The shaders a submission could not prepare, as a clause for a refusal - empty when none failed.
fn shader_failures(submission: &Submission) -> String {
    use std::fmt::Write as _;
    let mut clause = String::new();
    for failure in &submission.report.failures {
        let _ = write!(
            clause,
            "; its {} shader at {:#x} was not prepared: {}",
            failure.stage, failure.address, failure.reason
        );
    }
    clause
}

/// Why a submission's draws cannot be drawn as the guest asked, from what preparing them found:
/// positions that are not clip space, memory reads with nowhere to read from, textures or buffers
/// that could not be bound, or a stage with no shader. `None` when nothing stands in the way.
fn unmodelled_draws(submission: &Submission) -> Option<String> {
    let report = &submission.report;
    if report.unmodelled_viewports > 0 {
        return Some(format!(
            concat!(
                "{} draw(s) turn the viewport transform off, and positions that are not ",
                "clip space are not modelled"
            ),
            report.unmodelled_viewports
        ));
    }
    if let (count @ 1.., first) = &report.unmodelled_layers {
        return Some(format!(
            "{count} draw(s) are not drawn into the layers the hardware would draw them into{}",
            first
                .as_ref()
                .map_or_else(String::new, |first| format!(": {first}"))
        ));
    }
    if report.unmasked_one_byte_draws > 0 {
        return Some(format!(
            concat!(
                "{} draw(s) into a one-byte colour target set no CB_TARGET_MASK, so which ",
                "channels they write is not known"
            ),
            report.unmasked_one_byte_draws
        ));
    }
    if report.unexported_channel_draws > 0 {
        return Some(format!(
            concat!(
                "{} draw(s) write a colour channel their pixel shader does not export, which ",
                "would hold whatever its output held"
            ),
            report.unexported_channel_draws
        ));
    }
    if report.unbound_export_draws > 0 {
        return Some(format!(
            concat!(
                "{} draw(s) run a pixel shader that exports to a colour target no colour buffer ",
                "is bound for, which would draw it nowhere"
            ),
            report.unbound_export_draws
        ));
    }
    if let (count @ 1.., access) = &report.unwindowed_draws {
        return Some(format!(
            concat!(
                "{} draw(s) run a shader that reads guest memory, and no window is mapped ",
                "for it to read through{}"
            ),
            count,
            access
                .as_ref()
                .map_or_else(String::new, |access| format!(": {access}"))
        ));
    }
    if report.unbound_textures > 0 {
        return Some(format!(
            concat!(
                "{} sampled texture slot(s) had no texture bound, and the placeholder is ",
                "not the guest's picture{}"
            ),
            report.unbound_textures,
            report
                .first_unbound_texture
                .as_ref()
                .map_or_else(String::new, |first| format!(" - the first: {first}"))
        ));
    }
    if report.unshaded_draws > 0 {
        return Some(format!(
            concat!(
                "{} draw(s) have a stage whose shader was not prepared, and would run the ",
                "shader bound before them{}"
            ),
            report.unshaded_draws,
            shader_failures(submission)
        ));
    }
    if let (refused @ 1.., why) = &report.unbound_buffers {
        return Some(format!(
            "{refused} draw(s) read through a buffer that could not be bound: {}",
            why.as_deref().unwrap_or("no reason was kept")
        ));
    }
    None
}

/// The first draw segment a run refused, as prepared, so the worker can list its commands beside
/// the refusal: the last submission is often not the one that stopped.
fn refused_submission() -> &'static Mutex<Option<Submission>> {
    static REFUSED: OnceLock<Mutex<Option<Submission>>> = OnceLock::new();
    REFUSED.get_or_init(|| Mutex::new(None))
}

/// Passes `drawn` through, keeping `submission` when it was refused and none was kept before.
///
/// The first refusal's reason is logged: the command processor stops at the segment's first draw
/// as needing the GPU and keeps no reason, and every submission after it waits behind it, so a
/// run whose later frames are all empty is explained by this one line.
fn keeping_refused(submission: &Submission, drawn: Result<(), String>) -> Result<(), String> {
    if let Err(why) = &drawn {
        let mut kept = refused_submission()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if kept.is_none() {
            tracing::warn!("a draw segment was refused, and nothing after it retires: {why}");
            *kept = Some(submission.clone());
        }
    }
    drawn
}

/// The first draw segment the run refused, as prepared, if one was; taking it empties the store.
#[must_use]
pub fn take_refused_submission() -> Option<Submission> {
    refused_submission()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
}

/// Why the most recent draw segment was not carried out, for the run report.
fn draw_refusal() -> &'static Mutex<Option<String>> {
    static REFUSAL: Mutex<Option<String>> = Mutex::new(None);
    &REFUSAL
}

fn note_draw_refusal(why: String) {
    *draw_refusal()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(why);
}

/// Why the most recent draw segment was refused, if one was.
#[must_use]
pub fn last_draw_refusal() -> Option<String> {
    draw_refusal()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

impl GuestCp<'_> {
    /// Carries out one segment of the stream's draws (D729): a segment holding every draw is the
    /// submission as prepared; any other is prepared from [`segment_stream`] through the live
    /// pipeline, carrying the modules the whole submission brought for the backend.
    fn draw_segment(&mut self, segment: &cp::DrawSegment<'_>) -> Result<(), String> {
        let whole = self
            .submission
            .ok_or("no submission is being carried out")?;
        // Prepared ahead of the draws it samples, a submission is drawn as a segment is (D882).
        if segment.whole && !whole.read_ahead_of_draws {
            let whole = with_bound_modules(whole);
            return keeping_refused(&whole, self.draw_noting_delivery(&whole));
        }
        // A segment that draws into more than one colour target is drawn a run at a time, each run
        // into its own target.
        for (first, end) in self.target_runs(segment, whole)? {
            self.draw_run(
                &cp::DrawSegment {
                    stream: segment.stream,
                    first,
                    end,
                    whole: false,
                },
                whole,
            )?;
        }
        Ok(())
    }

    /// The spans of a segment's runs of draws into one colour target ([`colour_target_runs`]),
    /// from the registers in force as the stream was submitted.
    fn target_runs(
        &self,
        segment: &cp::DrawSegment<'_>,
        whole: &Submission,
    ) -> Result<Vec<(u32, u32)>, String> {
        let walked = crate::packet::walk(segment.stream);
        let writes = {
            let live = live_pipeline()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let pipeline = live.as_ref().ok_or("no pipeline is live")?;
            pipeline
                .writes_as_submitted(
                    whole,
                    &walked,
                    segment.stream,
                    Some(&whole.tables.over(&self.memory)),
                )
                .0
        };
        let draws: Vec<(u32, u32)> = walked
            .packets
            .iter()
            .filter(|packet| {
                matches!(packet.kind, crate::packet::PacketKind::Command { opcode } if cp::is_draw(opcode))
                    && (segment.first..segment.end).contains(&packet.offset)
            })
            .map(|packet| (packet.offset, packet.offset + packet.length))
            .collect();
        let runs = crate::registers::colour_target_runs(&writes, &draws);
        Ok(if runs.is_empty() {
            vec![(segment.first, segment.end)]
        } else {
            runs
        })
    }

    /// Prepares one run of a segment's draws from [`segment_stream`] and draws it into its target.
    fn draw_run(
        &mut self,
        segment: &cp::DrawSegment<'_>,
        whole: &Submission,
    ) -> Result<(), String> {
        let bytes = segment_stream(segment);
        let prepared = {
            let mut live = live_pipeline()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let pipeline = live.as_mut().ok_or("no pipeline is live")?;
            // The registers as submitted: the tables read then, not what the guest has since
            // written to them.
            // And over the register state the whole was prepared over, not what has been carried
            // since (D821).
            let prepared = pipeline.submit_over(
                whole.carried_in.clone(),
                &bytes,
                Queue::Draw,
                &[],
                &whole.tables.over(&SettledReads(&self.memory)),
            );
            keep_new_translations(pipeline);
            note_made(&prepared);
            prepared
        };
        let prepared = with_bound_modules(&prepared);
        let drawn = self.draw_noting_delivery(&prepared);
        keeping_refused(&prepared, drawn)
    }

    /// [`Self::draw_into_target`], and when it drew, the modules it carried are the backend's.
    fn draw_noting_delivery(&mut self, submission: &Submission) -> Result<(), String> {
        let drawn = self.draw_into_target(submission);
        if drawn.is_ok() {
            note_delivered(submission);
        }
        drawn
    }

    /// Leaves every block of a DCC target stored uncompressed, with its key saying so, before its
    /// draws run. The frame is then written back as uncompressed blocks under uncompressed keys - a
    /// state the hardware's own writes leave - and nothing here compresses. Keys all uncompressed
    /// already need nothing; keys all cleared to `0000` mean every bit of every block is zero, so
    /// the surface is filled with zeros and its keys marked uncompressed, which is exactly what
    /// expanding them leaves. Any other key state needs per-block key addressing and is refused,
    /// with nothing written. A mip chain's keys and blocks are taken whole, every level's
    /// together, so the level drawn needs no per-level key addressing either. `layout` is the
    /// target's, which sizes its keys and blocks by its texels.
    fn expand_dcc(
        &mut self,
        target: crate::registers::ColourTargetDcc,
        layout: tiling::SurfaceLayout,
    ) -> Result<(), String> {
        let crate::registers::ColourTargetDcc {
            dcc,
            surface,
            extent,
            levels,
            clear,
            pipe_bank_xor,
            ..
        } = target;
        let start = crate::dcc::meta_start(dcc);
        let length = crate::dcc::texel_chain_meta_bytes(
            extent.width,
            extent.height,
            levels,
            layout,
            dcc.pipe_aligned,
        );
        let keys = usize::try_from(length)
            .ok()
            .and_then(|length| cp::CpMemory::read(self, start, length))
            .ok_or("its DCC keys are not readable guest memory")?;
        // A fast clear to the register's colour fills every block with it, as a clear to `0000`
        // fills them with zeros.
        let fill = match crate::dcc::classify(&keys) {
            crate::dcc::Keys::Uncompressed => return Ok(()),
            crate::dcc::Keys::Clear0000 => Some(0),
            crate::dcc::Keys::Other => {
                crate::dcc::cleared_fill(&keys, clear, 1 << layout.texel_log2())
            }
        };
        match fill {
            Some(word) => {
                let span = usize::try_from(layout.chain_bytes(extent.width, extent.height, levels))
                    .map_err(|_| "its surface is larger than this host addresses".to_owned())?;
                let keys_uncompressed = u32::from_le_bytes([crate::dcc::KEY_UNCOMPRESSED; 4]);
                if cp::CpMemory::fill(self, surface, word, span)
                    && cp::CpMemory::fill(self, start, keys_uncompressed, keys.len())
                {
                    Ok(())
                } else {
                    Err("its cleared DCC blocks could not be expanded in guest memory".to_owned())
                }
            }
            // Block by block, each by its own key (D873).
            None => self
                .expand_dcc_blocks(
                    (&keys, start, dcc),
                    (surface, extent, levels, pipe_bank_xor),
                    (layout, clear),
                )
                .map_err(|why| {
                    format!(
                        "{why} ({}; clear words {clear:x?})",
                        crate::dcc::census(&keys)
                    )
                }),
        }
    }

    /// A DCC target's blocks expanded one at a time (D873): each compress block's key found by the
    /// GFX10 key equation, a block cleared to `0000` written as zeros, one cleared to the register
    /// as the clear colour, one uncompressed kept, and every key then marked uncompressed. Refused,
    /// with nothing written, where a key says the block is compressed - whose encoding is not
    /// published - or the surface is one the equation was not read for: a single level of four-
    /// or eight-byte `64KB_R_X` texels under no data pipe-bank XOR.
    fn expand_dcc_blocks(
        &mut self,
        (keys, start, dcc): (&[u8], u64, crate::dcc::Dcc),
        (surface, extent, levels, pipe_bank_xor): (
            u64,
            crate::registers::ColourTargetExtent,
            u32,
            u8,
        ),
        (layout, clear): (tiling::SurfaceLayout, Option<[u32; 2]>),
    ) -> Result<(), String> {
        let texel_bytes = 1_u32 << layout.texel_log2();
        let equation = crate::dcc::key_equation(texel_bytes, dcc.pipe_aligned)
            .filter(|_| levels == 1 && pipe_bank_xor == 0)
            .filter(|_| layout.compress_block_offset(0, 0, 1).is_some())
            .ok_or("its DCC keys need per-block decoding the key equation was not read for")?;
        let pitch = equation.pitch(extent.width);
        // The keys' pipe XOR, which radeonsi ORs into the base's low bits (`ac_descriptors.c`
        // 1504-1506).
        let pipe_xor = u32::try_from((dcc.base - start) >> 8).unwrap_or(0);
        let clear_block: Option<Vec<u8>> = clear.map(|[low, high]| {
            let texel: Vec<u8> = if texel_bytes == 8 {
                [low.to_le_bytes(), high.to_le_bytes()].concat()
            } else {
                low.to_le_bytes().to_vec()
            };
            texel.iter().copied().cycle().take(256).collect()
        });
        let (across, down) = equation.compress;
        // Every block's key first: nothing is written unless every one is decoded.
        let mut blocks = Vec::new();
        for y in (0..extent.height).step_by(down as usize) {
            for x in (0..extent.width).step_by(across as usize) {
                let at = usize::try_from(equation.key_offset((x, y), pitch, pipe_xor))
                    .map_err(|_| "a DCC key outside this host's address range".to_owned())?;
                let key = *keys.get(at).ok_or("a DCC key past the target's keys")?;
                let offset = layout
                    .compress_block_offset(x, y, extent.width)
                    .ok_or("a compress block the layout does not place")?;
                let block = match key {
                    crate::dcc::KEY_UNCOMPRESSED => continue,
                    crate::dcc::KEY_CLEAR_0000 => None,
                    crate::dcc::KEY_CLEAR_REGISTER => Some(
                        clear_block
                            .as_ref()
                            .ok_or("a block cleared to a register colour the stream never wrote")?,
                    ),
                    _ => {
                        return Err(
                            "a DCC block is compressed, whose encoding is not published".to_owned()
                        );
                    }
                };
                blocks.push((surface + (offset as u64 & !0xFF), block));
            }
        }
        let zeros = [0_u8; 256];
        for (address, block) in blocks {
            let bytes = block.map_or(&zeros[..], Vec::as_slice);
            if !cp::CpMemory::write(self, address, bytes) {
                return Err("an expanded DCC block could not be written to guest memory".to_owned());
            }
        }
        let keys_uncompressed = u32::from_le_bytes([crate::dcc::KEY_UNCOMPRESSED; 4]);
        if cp::CpMemory::fill(self, start, keys_uncompressed, keys.len()) {
            Ok(())
        } else {
            Err("the expanded DCC keys could not be written to guest memory".to_owned())
        }
    }

    /// Reads the target, has the executor draw over it, and - after every submission, or at the
    /// flip - writes the result back where the guest reads it (D714). Why not, with nothing
    /// written, at the first thing that is not exact.
    fn draw_into_target(&mut self, submission: &Submission) -> Result<(), String> {
        let (Some(execute), Some(read)) = (draw_executor().get(), frame_reader().get()) else {
            return Err("no device is installed to draw with".to_owned());
        };
        if draws_depth_only(submission) {
            return self.draw_depth_only(submission, *execute);
        }
        let (target, swap) = writable_target(submission).map_err(|why| {
            format!("its colour target is not one a frame can be written back to exactly: {why}")
        })?;
        if let Some(why) = unmodelled_draws(submission) {
            return Err(why);
        }
        if let Some(dcc) = submission.colour_target_dcc
            && let Some(why) = crate::dcc::unsupported(
                submission.colour_target_tiling,
                dcc.single_slice,
                dcc.single_sample,
            )
        {
            return Err(format!(
                "its colour target uses {why}, which is not modelled"
            ));
        }
        // The executor holds one frame per extent, so a frame still pending for another target is
        // let go of before this one is drawn over it (D714): kept on the device as a flip keeps
        // one, written back when something first touches it (D719, D885), or now where it cannot
        // wait.
        if pending_target().is_some_and(|(pending, _)| pending != target) && !hold_pending(self) {
            return Err(
                "the frame pending for another target could not be written back".to_owned(),
            );
        }
        if let Some(dcc) = submission.colour_target_dcc {
            self.expand_dcc(dcc, target.layout)?;
        }
        let more = self.seed_more_targets(submission, target)?;
        // What the guest wrote into the pending frame's rows reaches the device before more is
        // drawn over it (D864); where it cannot, the frame goes back whole and is read afresh.
        let banded_here = open_band()
            .lock()
            .is_ok_and(|open| open.is_some_and(|band| band.target == target))
            && pending_target().is_some_and(|(pending, _)| pending == target);
        let folded_here = if fold_band() {
            banded_here
        } else {
            let _ = write_back_pending(&mut GuestCp {
                memory: MappedRegions::current(),
                submission: None,
            });
            false
        };
        if !write_back_at_flip() {
            draw_over(self, target, swap, |before| {
                execute(submission, before).then(read).flatten()
            })
            .then_some(())
            .ok_or_else(|| {
                format!(
                    "the device did not draw it, or it could not be written back{}",
                    shader_failures(submission)
                )
            })?;
            note_drawn(&drawn_bases(target, &more), submission);
            return self.write_back_more_targets(&more);
        }
        // Drawn and left on the device: the frame is written back when the guest flips, not after
        // each of the many submissions a GL frame takes (D714). A frame still pending with its band
        // folded in is the drawer's own as it stands: any other write to the target would have
        // taken it back whole, so memory has nothing for it (D864).
        let before = if folded_here {
            Some(TargetRead::Unchanged)
        } else {
            crate::perf::span(crate::perf::Span::ReadTargetBefore, || {
                read_target(self, target, swap)
            })
        }
        .ok_or_else(|| {
            format!(
                "its colour target at {:#x}, {:#x} bytes, is not readable guest memory",
                target.base,
                target.words() * 4
            )
        })?;
        if !execute(submission, before.before()) {
            forget_written();
            set_pending(None);
            return Err(format!(
                "the device did not draw it{}",
                shader_failures(submission)
            ));
        }
        set_pending(Some((target, swap)));
        note_drawn(&drawn_bases(target, &more), submission);
        // The frame now pending was drawn over the one the deferred write-back holds, and writes
        // every byte it would: it is superseded, unread (D801).
        if matches!(before, TargetRead::HeldOverDeferred) {
            drop_covered(target.base, target.words() as u64 * 4);
        }
        self.write_back_more_targets(&more)
    }

    /// Hands the device what each colour attachment after the first holds, read from guest memory
    /// now (D825), and answers each with its id. They are not deferred to the flip as the first
    /// is: each is read before and written back after every submission that draws into it.
    fn seed_more_targets(
        &mut self,
        submission: &Submission,
        first: ColourTarget,
    ) -> Result<Vec<(ColourTarget, ComponentSwap, crate::ResourceId)>, String> {
        if submission.more_colour_targets.is_empty() {
            return Ok(Vec::new());
        }
        let Some((seed, _)) = more_targets_io().get() else {
            return Err(
                "no device is installed to draw more than one colour attachment".to_owned(),
            );
        };
        let mut more = Vec::with_capacity(submission.more_colour_targets.len());
        let mut before = Vec::with_capacity(more.capacity());
        for attachment in &submission.more_colour_targets {
            let (target, swap) = writable_more_target(attachment, first).ok_or_else(|| {
                format!(
                    concat!(
                        "its colour attachment {} (CB_COLOR{}) is not one a frame can be written ",
                        "back to exactly: {:?}, {:?}, {:?}, {}"
                    ),
                    more.len() + 1,
                    attachment.buffer,
                    attachment.target,
                    attachment.tiling,
                    attachment.format,
                    if attachment.dcc.is_some() {
                        "compressed"
                    } else {
                        "uncompressed"
                    }
                )
            })?;
            let length = target.words() * 4;
            // A frame still pending for its pages is written back first, so this reads it.
            let bytes = self
                .settle_pending(target.base, length as u64)
                .then(|| cp::CpMemory::read(self, target.base, length))
                .flatten()
                .filter(|bytes| bytes.len() >= length)
                .ok_or_else(|| {
                    format!(
                        "its colour attachment at {:#x}, {length:#x} bytes, is not readable guest memory",
                        target.base
                    )
                })?;
            before.push(target.detile_mapped(&words_of(&bytes), |w| loaded(w, target, swap)));
            more.push((target, swap, attachment.id));
        }
        let seeds: Vec<(crate::ResourceId, (u32, u32), &[u32])> = more
            .iter()
            .zip(&before)
            .map(|((target, _, id), words)| (*id, (target.width, target.height), words.as_slice()))
            .collect();
        if seed(&seeds) {
            Ok(more)
        } else {
            Err("the device could not take its further colour attachments".to_owned())
        }
    }

    /// Writes each colour attachment after the first back where the guest reads it, tiled and in
    /// its byte order, as the device drew it (D825).
    fn write_back_more_targets(
        &mut self,
        more: &[(ColourTarget, ComponentSwap, crate::ResourceId)],
    ) -> Result<(), String> {
        let Some((_, read)) = more_targets_io().get() else {
            return Ok(());
        };
        for &(target, swap, id) in more {
            let after = read(id).ok_or_else(|| {
                format!(
                    "its colour attachment at {:#x} could not be read back",
                    target.base
                )
            })?;
            let mut tiled_ok = false;
            let edited =
                cp::CpMemory::edit_words(self, target.base, target.words(), &mut |tiled| {
                    tiled_ok = target
                        .tile_mapped(&after, tiled, |w| stored(w, target, swap))
                        .is_ok();
                });
            if !(edited && tiled_ok) {
                return Err(format!(
                    "its colour attachment at {:#x} could not be written back",
                    target.base
                ));
            }
        }
        Ok(())
    }
}

impl GuestCp<'_> {
    /// Draws a submission whose colour target zero is unbound (D779): its draws write only the
    /// depth the drawer keeps (the guest's depth tiling is unmeasured, so depth is never written
    /// back), and the colour the drawer produces is no target's. A frame pending for any target
    /// is let go of first - kept on the device as a switch of target keeps it (D885) - and the
    /// drawer's colour is then forgotten, so the next colour draw starts from what guest memory
    /// holds, or will once that frame is carried out.
    fn draw_depth_only(
        &mut self,
        submission: &Submission,
        execute: DrawExecutor,
    ) -> Result<(), String> {
        if let Some(why) = unmodelled_draws(submission) {
            return Err(why);
        }
        if !hold_pending(self) {
            return Err("the frame pending for its target could not be written back".to_owned());
        }
        forget_written();
        set_pending(None);
        let drawn = execute(submission, Before::Uniform(0));
        forget_written();
        if drawn {
            Ok(())
        } else {
            Err(format!(
                "the device did not draw it{}",
                shader_failures(submission)
            ))
        }
    }
}

/// Whether a submission's colour target zero is unbound - `COLOR_INVALID` - so its draws write
/// depth alone (D779).
fn draws_depth_only(submission: &Submission) -> bool {
    submission
        .colour_target_format
        .is_some_and(|format| format.is_unbound())
}

/// Runs a guest compute dispatch's module over its window, images and buffers: the module with its
/// [`crate::content_hash`], the window's words before, the push-constant block and the grid, the
/// images it fetches from and stores to, and each of its own buffers' words before (D746). Answers
/// the window's words after, whether any access escaped the window, the stored image's texels
/// after, and each buffer's words after, or why the device could not run it. Installed by the
/// worker, which owns the graphics device.
pub type DispatchExecutor = fn(
    (&[u32], u64),
    &[u32],
    (&[u32], [u32; 3]),
    (
        &crate::pipeline::DispatchImages,
        &[crate::dispatch::BufferBefore],
    ),
) -> Result<DispatchDone, String>;

/// What a dispatch left: the window's words, whether an access escaped it, the stored image's
/// texels when it bound one, and its own buffers' words with their write masks (D746, D809).
pub type DispatchDone = (
    Vec<u32>,
    bool,
    Option<Vec<u32>>,
    Vec<crate::dispatch::BufferAfter>,
);

fn dispatch_executor() -> &'static OnceLock<DispatchExecutor> {
    static EXECUTOR: OnceLock<DispatchExecutor> = OnceLock::new();
    &EXECUTOR
}

/// The samples the draws carried out so far have passed while counting them (D827), once they have
/// run; `None` when the device cannot count them exactly. Installed by the worker.
pub type SampleCounter = fn() -> Option<u64>;

fn sample_counter() -> &'static OnceLock<SampleCounter> {
    static COUNTER: OnceLock<SampleCounter> = OnceLock::new();
    &COUNTER
}

/// Installs what a `ZPASS_DONE` reads the samples passed from. Without it the event stops the
/// command processor. First install wins.
pub fn install_sample_counter(counter: SampleCounter) {
    let _ = sample_counter().set(counter);
}

/// Installs the executor a submit carries out its compute dispatches with. Without one, a dispatch
/// stops the command processor. First install wins.
pub fn install_dispatch_executor(executor: DispatchExecutor) {
    let _ = dispatch_executor().set(executor);
}

/// Why the most recent dispatch was not carried out, for the run report.
fn dispatch_refusal() -> &'static Mutex<Option<String>> {
    static REFUSAL: Mutex<Option<String>> = Mutex::new(None);
    &REFUSAL
}

fn note_dispatch_refusal(why: String) {
    *dispatch_refusal()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(why);
}

/// Why the most recent compute dispatch was refused, if one was.
#[must_use]
pub fn last_dispatch_refusal() -> Option<String> {
    dispatch_refusal()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

/// What a dispatch's stored image writes back, in order: under cleared keys the whole chain they
/// cover as the zeros they mean, then the stored level's texels over what it held - only the bytes
/// that changed, so the rest of its blocks, a mip tail's other levels among them, are untouched -
/// then the cleared keys marked uncompressed, now every texel is what it says.
///
/// # Errors
///
/// When the device answered no texels for a stored image, or ones that do not fit it.
fn stored_image_writes(
    images: &crate::pipeline::DispatchImages,
    stored: Option<Vec<u32>>,
) -> Result<GuestWrites, String> {
    let mut writes = GuestWrites::default();
    if let Some(expand) = images.stored_keys_to_expand {
        let (base, bytes) = expand.surface;
        writes.push_repeated(base, 0, bytes);
    }
    if let Some((surface, spanned_before)) = &images.stored_surface {
        let mut texels = stored.ok_or("the device did not answer the stored image")?;
        let mut before = images
            .stored
            .as_ref()
            .map(|stored| std::borrow::Cow::Borrowed(&*stored.texels));
        // Texels a descriptor reorders were read into the shader's order; memory keeps its own.
        if let Some(order) = images.stored_order {
            for texel in &mut texels {
                *texel = crate::pipeline::to_memory_channels(*texel, order);
            }
            before = before.map(|before| {
                before
                    .iter()
                    .map(|&texel| crate::pipeline::to_memory_channels(texel, order))
                    .collect()
            });
        }
        let before = before.as_deref();
        if let Some(changed) = before.and_then(|before| changed_texels(surface, before, &texels)) {
            for (slot, texel) in changed {
                writes.push(surface.base() + u64::from(slot) * 4, &texel.to_le_bytes());
            }
            if let Some(expand) = images.stored_keys_to_expand {
                let (keys, bytes) = expand.keys;
                writes.push_repeated(keys, crate::dcc::KEY_UNCOMPRESSED, bytes);
            }
            return Ok(writes);
        }
        let started = std::time::Instant::now();
        let mut spanned = spanned_before.clone();
        surface
            .tile(&texels, &mut spanned)
            .map_err(|e| format!("the stored image could not be tiled back: {e}"))?;
        for (first, bytes) in changed_runs(spanned_before, &spanned) {
            writes.push(
                surface.base() + first as u64,
                &spanned[first..first + bytes],
            );
        }
        crate::detiled::note(
            &format!("{} tiled back", surface.layout_name()),
            spanned.len() as u64,
            started.elapsed(),
        );
    }
    if let Some(expand) = images.stored_keys_to_expand {
        let (keys, bytes) = expand.keys;
        writes.push_repeated(keys, crate::dcc::KEY_UNCOMPRESSED, bytes);
    }
    Ok(writes)
}

/// A stored image's texels that changed, each as the word of the surface it lies in and its value
/// after, in address order - where each texel is a word of its own and few enough changed that
/// writing them one by one costs less than tiling every texel back and comparing the bytes.
/// MDEM00001's dispatches change some thirty texels of an image a frame, and the tiling took 0.35
/// ms of each. `None` where it does not apply, and the caller tiles the image back whole.
fn changed_texels(
    surface: &crate::pipeline::DispatchSurface,
    before: &[u32],
    after: &[u32],
) -> Option<Vec<(u32, u32)>> {
    if before.len() != after.len() {
        return None;
    }
    let most = before.len() / CHANGED_TEXELS_FEWEST_PART;
    let mut changed = Vec::new();
    for (first, words) in changed_runs(before, after) {
        if changed.len() + words > most {
            return None;
        }
        changed.extend(first..first + words);
    }
    if changed.is_empty() {
        return Some(Vec::new());
    }
    let slots = surface.texel_slots()?;
    let mut written: Vec<(u32, u32)> = changed
        .into_iter()
        .map(|index| Some((*slots.get(index)?, after[index])))
        .collect::<Option<_>>()?;
    written.sort_unstable_by_key(|&(slot, _)| slot);
    Some(written)
}

/// The share of a stored image's texels past which [`changed_texels`] gives way to tiling it whole:
/// one in this many.
const CHANGED_TEXELS_FEWEST_PART: usize = 8;

/// The runs of words that differ between `before` and `after`, as `(first word, words)`.
fn changed_runs<T: PartialEq>(before: &[T], after: &[T]) -> Vec<(usize, usize)> {
    let len = before.len().min(after.len());
    let (before, after) = (&before[..len], &after[..len]);
    let mut runs = Vec::new();
    let mut start = None;
    // A chunk at a time, each looked at once. Outside a run, a chunk that did not change is passed
    // over in one comparison, which for words and bytes is a memory compare: PPSA28061's
    // dispatches bind a 32 MiB buffer and change none of it. Any other chunk is compared element
    // by element.
    for (chunk, (old, new)) in before
        .chunks(UNCHANGED_CHUNK)
        .zip(after.chunks(UNCHANGED_CHUNK))
        .enumerate()
    {
        if start.is_none() && old == new {
            continue;
        }
        // Inside a run, a chunk changed throughout extends it whole: one pass over it, which the
        // compiler compares many elements at a time, rather than a branch an element. A stored
        // image written back whole changes nearly every byte (GTA V's video planes, 8 MB each).
        if start.is_some() && old.len() == new.len() && old.iter().zip(new).all(|(a, b)| a != b) {
            continue;
        }
        for (offset, (old, new)) in old.iter().zip(new).enumerate() {
            let index = chunk * UNCHANGED_CHUNK + offset;
            match (old != new, start) {
                (true, None) => start = Some(index),
                (false, Some(first)) => {
                    runs.push((first, index - first));
                    start = None;
                }
                _ => {}
            }
        }
    }
    if let Some(first) = start {
        runs.push((first, len - first));
    }
    runs
}

/// How many elements [`changed_runs`] compares at once where nothing has changed.
const UNCHANGED_CHUNK: usize = 64;

/// Runs of bytes to write into guest memory, in order: each run's address and where its bytes lie
/// in one buffer they all share. A dispatch's changes come back as thousands of runs of a word or
/// two - PPSA25872's, eleven thousand a dispatch - and one allocation each cost more than the
/// writing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct GuestWrites {
    bytes: Vec<u8>,
    runs: Vec<(u64, std::ops::Range<usize>)>,
}

/// What a translated program's dispatches have written back this run: how many were carried out,
/// the bytes they changed, and whether any changed byte is not zero.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DispatchWrites {
    /// Dispatches carried out.
    pub dispatches: u64,
    /// Bytes they changed in guest memory: runs, scattered buffer words and fills.
    pub bytes: u64,
    /// Whether any byte they wrote is not zero.
    pub lit: bool,
    /// Where the guest program they run lies.
    pub program: Option<u64>,
    /// Where the last of them fetched its image, as (first byte, bytes).
    pub fetched: Option<(u64, u64)>,
    /// Where the last of them stored its image, as (first byte, bytes).
    pub stored: Option<(u64, u64)>,
    /// The buffers the last of them bound, each as (first byte, bytes, whether any word it read is
    /// not zero) - `None` for one it only stores to, which is not read.
    pub buffers: Vec<(u64, u64, Option<bool>)>,
}

/// [`DispatchWrites`] by the translated module's content hash.
fn dispatch_writes() -> &'static Mutex<std::collections::BTreeMap<u64, DispatchWrites>> {
    static WRITES: Mutex<std::collections::BTreeMap<u64, DispatchWrites>> =
        Mutex::new(std::collections::BTreeMap::new());
    &WRITES
}

/// Counts a dispatch of `module` and what it wrote back: its runs, its scattered buffer words and
/// its fills; where its `images` lie; and the buffers it bound at `ranges`, as they were `read`.
fn note_dispatch_writes(
    (module, program): (u64, u64),
    (writes, scattered, fills): (&GuestWrites, &[Scattered<'_>], &[cp::Fill]),
    (images, ranges, read): (
        &crate::pipeline::DispatchImages,
        &[(u64, u64)],
        &[crate::dispatch::BufferBefore],
    ),
) {
    let mut bytes = writes.bytes.len() as u64;
    let mut lit = writes.bytes.iter().any(|&byte| byte != 0);
    for buffer in scattered {
        for (_, word, held) in buffer.words() {
            bytes += held as u64;
            lit |= word[..held].iter().any(|&byte| byte != 0);
        }
    }
    for fill in fills {
        bytes += fill.count;
        lit |= fill.pattern != 0 && fill.count > 0;
    }
    let mut kept = dispatch_writes()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let entry = kept.entry(module).or_default();
    entry.dispatches += 1;
    entry.bytes += bytes;
    entry.lit |= lit;
    entry.program = Some(program);
    entry.fetched = images.fetched_span.or(entry.fetched);
    entry.stored = images
        .stored_surface
        .as_ref()
        .map(|(surface, _)| (surface.base(), surface.bytes() as u64))
        .or(entry.stored);
    entry.buffers = ranges
        .iter()
        .zip(read)
        .map(|(&(at, bytes), before)| {
            let lit = match before {
                crate::dispatch::BufferBefore::Words(words) => Some(words.iter().any(|&w| w != 0)),
                crate::dispatch::BufferBefore::Unread(_) => None,
            };
            (at, bytes, lit)
        })
        .collect();
}

/// What each translated program's dispatches wrote back this run, by its module's content hash.
#[must_use]
pub fn dispatch_writes_by_module() -> Vec<(u64, DispatchWrites)> {
    dispatch_writes()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .map(|(module, writes)| (*module, writes.clone()))
        .collect()
}

/// The writes a finished dispatch wrote back, emptied and kept for the next. PPSA25872's
/// dispatches write some 34 000 runs, and growing fresh vectors for them - their pages new to the
/// process each time - cost 0.7 ms of a dispatch.
fn spare_writes() -> &'static Mutex<GuestWrites> {
    static SPARE: Mutex<GuestWrites> = Mutex::new(GuestWrites {
        bytes: Vec::new(),
        runs: Vec::new(),
    });
    &SPARE
}

/// The most bytes, runs and their bytes together, the kept writes hold on to.
const SPARE_WRITES_BYTES_MOST: usize = 64 << 20;

impl GuestWrites {
    /// Empty writes, holding the room the last dispatch's took.
    fn spare() -> Self {
        spare_writes()
            .lock()
            .map(|mut spare| std::mem::take(&mut *spare))
            .unwrap_or_default()
    }

    /// Empties these and keeps them for [`Self::spare`], unless they hold more room than is worth
    /// keeping.
    fn keep(mut self) {
        let held = self.bytes.capacity()
            + self.runs.capacity() * size_of::<(u64, std::ops::Range<usize>)>();
        if held > SPARE_WRITES_BYTES_MOST {
            return;
        }
        self.bytes.clear();
        self.runs.clear();
        if let Ok(mut spare) = spare_writes().lock() {
            *spare = self;
        }
    }

    /// A run of `bytes` at `at`.
    fn push(&mut self, at: u64, bytes: &[u8]) {
        let start = self.bytes.len();
        self.bytes.extend_from_slice(bytes);
        self.runs.push((at, start..self.bytes.len()));
    }

    /// A run of `count` bytes of `value` at `at`.
    fn push_repeated(&mut self, at: u64, value: u8, count: usize) {
        let start = self.bytes.len();
        self.bytes.resize(start + count, value);
        self.runs.push((at, start..self.bytes.len()));
    }

    /// A run of `words`, little-endian, at `at`, cut at its first `most` bytes.
    fn push_words(&mut self, at: u64, words: &[u32], most: usize) {
        let start = self.bytes.len();
        let bytes = most.min(words.len() * 4);
        // Copied whole on a little-endian host, where a word's bytes in memory are its
        // little-endian ones: PPSA25872's store-only buffers hand back some 34 000 runs a
        // dispatch, and building each a byte at a time cost 0.3 ms of it.
        #[cfg(target_endian = "little")]
        self.bytes
            .extend_from_slice(&zerocopy::IntoBytes::as_bytes(words)[..bytes]);
        #[cfg(not(target_endian = "little"))]
        {
            self.bytes
                .extend(words.iter().flat_map(|word| word.to_le_bytes()));
            self.bytes.truncate(start + bytes);
        }
        self.runs.push((at, start..self.bytes.len()));
    }

    /// Every run of `other` after these.
    fn append(&mut self, other: &Self) {
        let offset = self.bytes.len();
        self.bytes.extend_from_slice(&other.bytes);
        self.runs.extend(
            other
                .runs
                .iter()
                .map(|(at, range)| (*at, range.start + offset..range.end + offset)),
        );
    }

    /// Run `index`: its address and its bytes.
    fn run(&self, index: usize) -> (u64, &[u8]) {
        let (at, range) = &self.runs[index];
        (*at, &self.bytes[range.clone()])
    }

    /// Runs `range`, each its address and its bytes.
    fn runs_in(&self, range: std::ops::Range<usize>) -> impl Iterator<Item = (u64, &[u8])> {
        range.map(|index| self.run(index))
    }

    /// Each run as its address and its own bytes, for a test to compare.
    #[cfg(test)]
    fn pairs(&self) -> Vec<(u64, Vec<u8>)> {
        (0..self.runs.len())
            .map(|index| {
                let (at, bytes) = self.run(index);
                (at, bytes.to_vec())
            })
            .collect()
    }
}

/// What a dispatch's own buffers write back (D746): each one's changed words, at its guest range,
/// the last cut where the range ends inside it - and, for a buffer it changed and left holding one
/// word throughout, that word as a fill of the whole range instead (D804), which a later reader of
/// the range is answered without the bytes being written.
///
/// # Errors
///
/// When the device answered another number of buffers, or one of another size.
#[cfg(test)]
fn buffer_writes(
    ranges: &[(u64, u64)],
    before: &[crate::dispatch::BufferBefore],
    after: &[crate::dispatch::BufferAfter],
) -> Result<(GuestWrites, Vec<cp::Fill>), String> {
    let mut writes = GuestWrites::default();
    let mut scattered = Vec::new();
    let fills = buffer_writes_into((&mut writes, &mut scattered), ranges, before, after)?;
    for buffer in scattered {
        buffer.push_runs(&mut writes);
    }
    Ok((writes, fills))
}

/// The words a dispatch stored to a buffer it did not read (D809), as the device listed them: each
/// index's word lies at the buffer's address and four bytes an index on, the last cut where the
/// buffer's range ends. Written back straight from the list rather than as a run each:
/// PPSA25872's dispatches store some 34 000 scattered words to one buffer, and a run apiece, made
/// and then copied, cost 0.2 ms of each.
#[derive(Debug, Clone, Copy)]
struct Scattered<'a> {
    address: u64,
    bytes: u64,
    indices: &'a [u32],
    values: &'a [u32],
}

impl Scattered<'_> {
    /// Each word's address and the bytes of it the range holds.
    fn words(&self) -> impl Iterator<Item = (u64, [u8; 4], usize)> + '_ {
        self.indices
            .iter()
            .zip(self.values)
            .map(|(&index, &value)| {
                let start = u64::from(index) * 4;
                let held = usize::try_from(self.bytes.saturating_sub(start).min(4)).unwrap_or(0);
                (self.address + start, value.to_le_bytes(), held)
            })
    }

    /// The first and last byte past what it writes: the list is ascending, so its first and last
    /// words bound it.
    fn span(&self) -> Option<(u64, u64)> {
        let (&first, &last) = (self.indices.first()?, self.indices.last()?);
        let start = self.address + u64::from(first) * 4;
        let end = (self.address + u64::from(last) * 4 + 4).min(self.address + self.bytes);
        (end > start).then_some((start, end))
    }

    /// Its words as runs of consecutive indices, for a test to compare as the runs were.
    #[cfg(test)]
    fn push_runs(&self, writes: &mut GuestWrites) {
        let mut at = 0;
        while at < self.indices.len() {
            let mut count = 1;
            while self
                .indices
                .get(at + count)
                .is_some_and(|&index| index == self.indices[at + count - 1].wrapping_add(1))
            {
                count += 1;
            }
            let start = u64::from(self.indices[at]) * 4;
            let most = usize::try_from(self.bytes.saturating_sub(start)).unwrap_or(0);
            writes.push_words(self.address + start, &self.values[at..at + count], most);
            at += count;
        }
    }
}

/// [`buffer_writes`], its writes added to `writes` rather than gathered on their own and copied
/// over after - but for a buffer whose stored words the device listed, which goes to `scattered`
/// to be written straight from the list - and answering its fills.
///
/// # Errors
///
/// As [`buffer_writes`].
fn buffer_writes_into<'a>(
    (writes, scattered): (&mut GuestWrites, &mut Vec<Scattered<'a>>),
    ranges: &[(u64, u64)],
    before: &[crate::dispatch::BufferBefore],
    after: &'a [crate::dispatch::BufferAfter],
) -> Result<Vec<cp::Fill>, String> {
    use crate::dispatch::BufferBefore;
    if after.len() != before.len() {
        return Err("the device answered another number of buffers".to_owned());
    }
    let mut fills = Vec::new();
    for ((&(address, bytes), before), after) in ranges.iter().zip(before).zip(after) {
        // A buffer read before the run writes the words it changed; one not read, the words it
        // stored (D809), as its write mask or the device's list of them says.
        // Taken a run at a time, never collected (see `BufferAfter::stored_words`).
        type Runs<'a> = Box<dyn Iterator<Item = (usize, &'a [u32])> + 'a>;
        let (runs, whole): (Runs<'_>, bool) = match before {
            BufferBefore::Words(words) => {
                if after.words.len() != words.len() {
                    return Err("the device answered a buffer of another size".to_owned());
                }
                let runs = Box::new(
                    changed_runs(words, &after.words)
                        .into_iter()
                        .map(|(first, count)| (first, &after.words[first..first + count])),
                );
                // Left holding one word throughout, it is a fill (D804), so the check covers all of
                // it, not only what changed.
                let whole = after
                    .words
                    .split_first()
                    .is_some_and(|(&pattern, rest)| rest.iter().all(|&word| word == pattern));
                (runs, whole)
            }
            // Stored whole with one value, as the device found: a fill, with nothing read back.
            BufferBefore::Unread(_) if let Some(pattern) = after.whole => {
                fills.push(cp::Fill {
                    destination: address,
                    pattern,
                    count: bytes,
                });
                continue;
            }
            BufferBefore::Unread(words) => {
                let size = (*words).max(1);
                if after.listed.is_none() && after.words.len() != size {
                    return Err("the device answered a buffer of another size".to_owned());
                }
                // A mask covers the buffer's words and no more; a list is ascending, so its last
                // index is its furthest, and each index has its value.
                if after.listed.as_ref().is_some_and(|(indices, values)| {
                    indices.len() != values.len()
                        || indices.last().is_some_and(|&last| last as usize >= size)
                }) {
                    return Err("the device stored past a buffer's end".to_owned());
                }
                // Stored whole with one word, it is a fill.
                let whole = after.stored_whole(size)
                    && after
                        .whole_words()
                        .split_first()
                        .is_some_and(|(&pattern, rest)| rest.iter().all(|&word| word == pattern));
                if !whole && let Some((indices, values)) = &after.listed {
                    scattered.push(Scattered {
                        address,
                        bytes,
                        indices,
                        values,
                    });
                    continue;
                }
                (Box::new(after.stored_words()), whole)
            }
        };
        let mut runs = runs.peekable();
        if whole && let Some(&(_, &[pattern, ..])) = runs.peek() {
            fills.push(cp::Fill {
                destination: address,
                pattern,
                count: bytes,
            });
            continue;
        }
        for (first, values) in runs {
            // The last word may run past the range's last byte, which is not the buffer's.
            let start = first as u64 * 4;
            let most = usize::try_from(bytes.saturating_sub(start)).unwrap_or(0);
            writes.push_words(address + start, values, most);
        }
    }
    Ok(fills)
}

/// Each of a dispatch's own buffers its run left holding one word throughout, as the fill it
/// amounts to (D750): what a buffer clear leaves, whether or not memory held it already. radeonsi
/// clears a depth surface's HTILE this way, and the clear is a clear even where the words were
/// the same before.
/// A buffer the run did not read counts only where it stored every word (D809).
fn uniform_buffers(
    ranges: &[(u64, u64)],
    (before, after): (
        &[crate::dispatch::BufferBefore],
        &[crate::dispatch::BufferAfter],
    ),
) -> Vec<cp::Fill> {
    ranges
        .iter()
        .zip(before.iter().zip(after))
        .filter_map(|(&(destination, count), (before, after))| {
            let held: &[u32] = match before {
                // Not read: a fill only where it stored every word, from its mask or its list.
                crate::dispatch::BufferBefore::Unread(_) if let Some(pattern) = after.whole => {
                    return Some(cp::Fill {
                        destination,
                        pattern,
                        count,
                    });
                }
                crate::dispatch::BufferBefore::Unread(words) => {
                    if !after.stored_whole((*words).max(1)) {
                        return None;
                    }
                    after.whole_words()
                }
                crate::dispatch::BufferBefore::Words(_) => &after.words,
            };
            let (&first, rest) = held.split_first()?;
            rest.iter().all(|&word| word == first).then_some(cp::Fill {
                destination,
                pattern: first,
                count,
            })
        })
        .collect()
}

impl GuestCp<'_> {
    /// Each of a dispatch's own buffers as guest memory holds it now, in whole words (D746) -
    /// except one the program only stores to, which is not read (D809).
    fn read_buffers(
        &self,
        ranges: &[(u64, u64)],
        unread: &[bool],
    ) -> Result<Vec<crate::dispatch::BufferBefore>, String> {
        ranges
            .iter()
            .enumerate()
            .map(|(slot, &(address, bytes))| {
                let span = usize::try_from(bytes.next_multiple_of(4))
                    .map_err(|_| "a buffer larger than this host addresses")?;
                if unread.get(slot).copied().unwrap_or(false) {
                    return Ok(crate::dispatch::BufferBefore::Unread(span / 4));
                }
                let read = cp::CpMemory::read(self, address, span)
                    .ok_or("a buffer it binds is not readable guest memory")?;
                Ok(crate::dispatch::BufferBefore::Words(
                    read.chunks_exact(4)
                        .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
                        .collect(),
                ))
            })
            .collect()
    }

    /// Carries out a compute dispatch: reads its state from the stream, prepares its program, runs
    /// it over its window as guest memory holds it now, and writes back exactly the words it
    /// changed. Nothing is written unless the whole dispatch ran exactly.
    /// The dispatch at `dispatch`, translated and with its images read from guest memory as it
    /// holds them now.
    fn prepare_dispatch(
        &self,
        dispatch: &cp::Dispatch<'_>,
    ) -> Result<crate::pipeline::PreparedDispatch, String> {
        crate::perf::span(crate::perf::Span::DispatchPrepare, || {
            let state = self.with_registers(dispatch.stream, |writes| {
                crate::dispatch::state_at(
                    writes,
                    dispatch.offset,
                    dispatch.groups,
                    dispatch.initiator,
                )
                .map_err(str::to_owned)
            })?;
            self.prepare_state(&state)
        })
    }

    /// Answers `read` over the registers in force through `stream`: what earlier submissions left,
    /// then the stream's (D737) - as the submission was prepared, when it is one (D821).
    fn with_registers<T>(
        &self,
        stream: &[u8],
        read: impl FnOnce(&[crate::registers::RegisterWrite]) -> Result<T, String>,
    ) -> Result<T, String> {
        let walked = crate::packet::walk(stream);
        let live = live_pipeline()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let pipeline = live.as_ref().ok_or("no pipeline is live")?;
        let (writes, _) = match self.submission {
            Some(whole) => pipeline.writes_as_submitted(whole, &walked, stream, Some(&self.memory)),
            None => pipeline.writes_in_force(&walked, stream, Some(&self.memory)),
        };
        read(&writes)
    }

    /// The program `state` runs, translated, with its images read from guest memory as it holds
    /// them now.
    fn prepare_state(
        &self,
        state: &crate::dispatch::DispatchState,
    ) -> Result<crate::pipeline::PreparedDispatch, String> {
        let mut live = live_pipeline()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let pipeline = live.as_mut().ok_or("no pipeline is live")?;
        pipeline.prepare_dispatch(state, &self.memory)
    }

    /// Carries out a segment's streamout draws (D861): each `DRAW_INDEX_AUTO` as its legacy vertex
    /// program run as one compute wave, the queue's offsets advanced by what it wrote. `None` for a
    /// segment whose first draw has streamout off, which draws as any other.
    fn streamout_segment(&mut self, segment: &cp::DrawSegment<'_>) -> Option<Result<(), String>> {
        let streams_out = self.with_registers(segment.stream, |writes| {
            Ok(crate::streamout::enabled_at(writes, segment.first))
        });
        if !matches!(streams_out, Ok(true)) {
            return None;
        }
        let walked = crate::packet::walk(segment.stream);
        let draws = walked.packets.iter().filter(|packet| {
            matches!(packet.kind, crate::packet::PacketKind::Command { opcode } if cp::is_draw(opcode))
                && (segment.first..segment.end).contains(&packet.offset)
        });
        let mut rasterized = false;
        for draw in draws {
            match self.streamout_draw(segment.stream, draw) {
                Ok(also_rasterized) => rasterized |= also_rasterized,
                Err(why) => return Some(Err(why)),
            }
        }
        // A draw left rasterizing is drawn too, its legacy vertex shader's stores dropped there
        // (D881): the streamout above made them.
        if rasterized {
            return Some(self.draw_segment(segment));
        }
        Some(Ok(()))
    }

    /// One streamout draw ([`Self::streamout_segment`]), answering whether it is also rasterized.
    fn streamout_draw(
        &mut self,
        stream: &[u8],
        draw: &crate::packet::Packet,
    ) -> Result<bool, String> {
        if draw.kind
            != (crate::packet::PacketKind::Command {
                opcode: crate::packet::build::measured::DRAW_INDEX_AUTO,
            })
        {
            return Err("an indexed streamout draw is not modelled".to_owned());
        }
        let start = draw.body_offset() as usize;
        let vertices = stream
            .get(start..start + 4)
            .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
            .ok_or("a DRAW_INDEX_AUTO with no vertex count")?;
        let offsets = *crate::streamout::offsets()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let plan = self.with_registers(stream, |writes| {
            if !crate::streamout::enabled_at(writes, draw.offset) {
                return Err("a run of draws turning streamout off between two of them".to_owned());
            }
            crate::streamout::plan(writes, draw.offset, vertices, offsets).map_err(str::to_owned)
        })?;
        // A wave at a time, each storing its vertices after the last's (D861).
        for dispatch in &plan.dispatches {
            self.run_prepared(&|cp: &Self| cp.prepare_state(dispatch))?;
        }
        {
            let mut stats = crate::streamout::stats()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            stats.written[0] += plan.primitives.0;
            stats.needed[0] += plan.primitives.1;
        }
        *crate::streamout::offsets()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = plan.after;
        Ok(plan.rasterized)
    }

    fn dispatch_into_memory(&mut self, dispatch: &cp::Dispatch<'_>) -> Result<(), String> {
        self.run_prepared(&|cp: &Self| cp.prepare_dispatch(dispatch))
    }

    /// Runs the dispatch `prepare` makes over guest memory as it holds it now, and writes back
    /// exactly the words it changed. Nothing is written unless the whole dispatch ran exactly.
    fn run_prepared(
        &mut self,
        prepare: &dyn Fn(&Self) -> Result<crate::pipeline::PreparedDispatch, String>,
    ) -> Result<(), String> {
        use crate::perf::{Span, span};
        let execute = dispatch_executor()
            .get()
            .ok_or("no device is installed to run it")?;
        let mut prepared = prepare(self)?;
        // An image the dispatch reads or stores that lies in a target whose frame is still on the
        // device is read from memory that does not hold the draws yet (D714): the frame goes in
        // first and the images are read again, as radeonsi's copy of its colour target into a
        // readback buffer after `glFinish` sees the draws on hardware.
        if let Some((target, _)) = pending_target()
            && prepared
                .images
                .read_spans()
                .any(|(address, length)| overlaps_target(target, address, length))
        {
            if !write_back_pending(self) {
                return Err("the frame on the target it reads could not be written back".to_owned());
            }
            prepared = prepare(self)?;
        }
        let ((base, before), buffers_before) = span(Span::DispatchRead, || {
            // A program with no buffer runs over one word nobody reads, and writes nothing back
            // there.
            let window = match prepared.window {
                Some(window) => {
                    let span = window.words() as usize * 4;
                    let bytes = cp::CpMemory::read(self, window.address(), span)
                        .ok_or("its window is not readable guest memory")?;
                    let words: Vec<u32> = bytes
                        .chunks_exact(4)
                        .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
                        .collect();
                    (Some(window.address()), words)
                }
                None => (None, vec![0]),
            };
            let buffers = self.read_buffers(&prepared.buffers, &prepared.unread)?;
            Ok::<_, String>((window, buffers))
        })?;
        let (after, escaped, stored, buffers_after) = span(Span::DispatchExecute, || {
            execute(
                (&prepared.module, prepared.module_hash),
                &before,
                (&prepared.push, prepared.groups),
                (&prepared.images, &buffers_before),
            )
        })?;
        let _written_back = crate::perf::SpanGuard::start(Span::DispatchWriteBack);
        if escaped {
            return Err(concat!(
                "an access left its window or went through a buffer descriptor whose addressing ",
                "is not modelled, so what it wrote is not the guest's"
            )
            .to_owned());
        }
        if after.len() != before.len() {
            return Err("the device answered a window of another size".to_owned());
        }
        let working_out = crate::perf::SpanGuard::start(Span::WriteBackChanges);
        let mut writes = GuestWrites::spare();
        if let Some(base) = base {
            span(Span::ChangesWindow, || {
                for (first, words) in changed_runs(&before, &after) {
                    writes.push_words(
                        base + first as u64 * 4,
                        &after[first..first + words],
                        usize::MAX,
                    );
                }
            });
        }
        let mut scattered = Vec::new();
        let buffer_fills = span(Span::ChangesBuffers, || {
            buffer_writes_into(
                (&mut writes, &mut scattered),
                &prepared.buffers,
                &buffers_before,
                &buffers_after,
            )
        })?;
        let images = span(Span::ChangesImages, || {
            stored_image_writes(&prepared.images, stored)
        })?;
        writes.append(&images);
        drop(working_out);
        span(Span::WriteBackGuest, || {
            self.write_back_dispatch(&writes, &scattered, &buffer_fills)
        })?;
        note_dispatch_writes(
            (prepared.module_hash, prepared.program),
            (&writes, &scattered, &buffer_fills),
            (&prepared.images, &prepared.buffers, &buffers_before),
        );
        writes.keep();
        // A buffer cleared to one word is a fill the depth surfaces are checked against (D750).
        let fills = uniform_buffers(&prepared.buffers, (&buffers_before, &buffers_after));
        if !fills.is_empty()
            && let Some(pipeline) = live_pipeline()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .as_mut()
        {
            pipeline.note_fills(fills);
        }
        Ok(())
    }
}

/// Answers the frame the executor's last draws left on the target, as linear `Rgba8` words - read
/// off the device when the frame is written back (D714). Installed by the worker with the executor.
pub type FrameReader = fn() -> Option<Vec<u32>>;

fn frame_reader() -> &'static OnceLock<FrameReader> {
    static READER: OnceLock<FrameReader> = OnceLock::new();
    &READER
}

/// Installs the reader a pending frame is fetched with (D714). First install wins.
pub fn install_frame_reader(reader: FrameReader) {
    let _ = frame_reader().set(reader);
}

/// Reads rows `first` to `first + count` of the pending frame off the device, as linear `Rgba8`
/// words (D864).
pub type RowsReader = fn(u32, u32) -> Option<Vec<u32>>;

/// Gives rows `first` to `first + count` of the pending frame on the device new texels, as linear
/// `Rgba8` words (D864). `false` when it could not.
pub type RowsPatcher = fn(u32, u32, &[u32]) -> bool;

fn frame_rows() -> &'static OnceLock<(RowsReader, RowsPatcher)> {
    static ROWS: OnceLock<(RowsReader, RowsPatcher)> = OnceLock::new();
    &ROWS
}

/// Installs what a band of a pending frame's rows is read and patched with (D864). Without it a
/// guest's write into a pending frame writes the whole frame back first. First install wins.
pub fn install_frame_rows(read: RowsReader, patch: RowsPatcher) {
    let _ = frame_rows().set((read, patch));
}

/// Rows of the pending frame the guest may write in memory (D864): read off the device into memory
/// and made writable, and folded back onto the device before anything takes the frame from it.
#[derive(Debug, Clone, Copy)]
struct Band {
    target: ColourTarget,
    swap: ComponentSwap,
    /// The first row and how many.
    rows: (u32, u32),
    /// The whole host pages they lie in.
    pages: (u64, u64),
}

fn open_band() -> &'static Mutex<Option<Band>> {
    static BAND: Mutex<Option<Band>> = Mutex::new(None);
    &BAND
}

/// The fewest rows a band holds: a software pointer's height and then some, so one fault answers
/// every write it makes.
const BAND_ROWS: u32 = 32;

/// The most bytes a band may span: a layout whose rows meet a page, or end a block row, only every
/// many rows would read back most of the frame anyway.
const BAND_BYTES_MOST: u64 = 2 << 20;

/// The band of `target`'s rows a write at `address` falls in: its first row and count, and where
/// its words lie - a surface of their own, `count` rows of the target's width laid out as the
/// target is, starting on a page. A linear target's band is whole rows starting where a row and a
/// page meet; a `64KB_R_X` one's is a row of blocks, 128 rows, which its layout tiles alone (no
/// per-block rotation, one pipe-bank XOR for the surface). `None` for any other target, one not laid
/// out by itself at a page, or one whose band would span more than [`BAND_BYTES_MOST`].
fn band_at(target: ColourTarget, address: u64) -> Option<((u32, u32), (u64, u64))> {
    use crate::tiling::SurfaceLayout;
    if target.layout.texel_log2() != 2
        || target.place != crate::registers::Place::Whole
        || target.base % HOST_PAGE != 0
        || address < target.base
    {
        return None;
    }
    // Rows that start where a band may, every so many, and the bytes between them.
    let (group, group_bytes) = match target.layout {
        SurfaceLayout::Linear => {
            let row_bytes = target_row_bytes(target);
            let (mut a, mut b) = (row_bytes, HOST_PAGE);
            while b != 0 {
                (a, b) = (b, a % b);
            }
            let group = HOST_PAGE / a;
            (group, group * row_bytes)
        }
        SurfaceLayout::Rx64Kb => (
            BLOCK_ROWS,
            u64::from(target.width.div_ceil(BLOCK_ROWS as u32)) * 64 * 1024,
        ),
        _ => return None,
    };
    let index = (address - target.base) / group_bytes;
    let first = index * group;
    let height = u64::from(target.height);
    if first >= height {
        return None;
    }
    let groups = u64::from(BAND_ROWS).div_ceil(group).max(1);
    if groups * group_bytes > BAND_BYTES_MOST {
        return None;
    }
    let count = (groups * group).min(height - first);
    let (first, count) = (u32::try_from(first).ok()?, u32::try_from(count).ok()?);
    let start = target.base + index * group_bytes;
    let bytes = target.layout.words(target.width, count) as u64 * 4;
    Some(((first, count), (start, bytes)))
}

/// The rows a `64KB_R_X` block holds at four bytes a texel.
const BLOCK_ROWS: u64 = 128;

/// Lets the guest write the band of the pending frame's rows its write at `address` falls in
/// (D864): those rows are read off the device into memory, and their pages - only theirs - made
/// writable, so the write lands on the frame as it would on hardware without the whole frame being
/// written back. `false` when it cannot, and the frame is then written back whole.
fn open_band_at(
    (target, swap): (ColourTarget, ComponentSwap),
    address: u64,
    (protection, under): (u32, Option<u32>),
    hooks: &LazyCopies,
) -> bool {
    let Some(&(read, _)) = frame_rows().get() else {
        return false;
    };
    let Ok(mut open) = open_band().lock() else {
        return false;
    };
    if open.is_some() {
        return false;
    }
    let Some(((first, count), pages)) = band_at(target, address) else {
        return false;
    };
    let whole = page_span(pages.0, pages.1);
    let width = target.width as usize;
    let Some(texels) = crate::perf::span(crate::perf::Span::ReadBand, || read(first, count))
        .filter(|texels| texels.len() == width * count as usize)
    else {
        return false;
    };
    if !(hooks.release)(whole.0, whole.1, protection) {
        return false;
    }
    // Laid out over what memory holds there, which keeps the words the rows do not cover - or over
    // the word a deferred fill would have left there.
    let memory = MappedRegions::current();
    let Some(mut words) = usize::try_from(pages.1)
        .ok()
        .and_then(|length| match under {
            Some(word) => Some(vec![word; length / 4]),
            None => memory.read(pages.0, length).map(words_of),
        })
    else {
        return false;
    };
    if target
        .layout
        .tile_mapped(
            &texels,
            target.width,
            count,
            target.pipe_bank_xor,
            &mut words,
            |w| stored(w, target, swap),
        )
        .is_err()
        || !copy_into_guest(pages.0, zerocopy::IntoBytes::as_bytes(words.as_slice()))
    {
        return false;
    }
    if let Ok(mut resolved) = resolved().lock() {
        if resolved.len() >= RESOLVED_KEPT {
            resolved.remove(0);
        }
        resolved.push(whole);
    }
    *open = Some(Band {
        target,
        swap,
        rows: (first, count),
        pages: whole,
    });
    true
}

/// The pending frame's band of rows a touch at `address` falls in, handed over while the deferred
/// fills `touched` over it stay deferred around the band (D864): each is cut into the pieces before
/// and after the band's pages, still guarded, and the band starts from the fill's word, which is
/// what memory would hold under the frame. The pieces, or `None` - with nothing changed - when the
/// touch is not on a pending frame's band, `touched` is not all fills of one word, or the band
/// cannot be opened.
fn band_under_fills(
    touched: &[Deferred],
    address: u64,
    hooks: &LazyCopies,
) -> Option<Vec<Deferred>> {
    let pending = pending_target().filter(|(target, _)| overlaps_target(*target, address, 1))?;
    let (_, rows) = band_at(pending.0, address)?;
    let band = page_span(rows.0, rows.1);
    let mut word = None;
    for copy in touched {
        let Held::Fill(pattern) = copy.held else {
            return None;
        };
        if word.is_some_and(|word| word != pattern) || copy.destination % 4 != 0 {
            return None;
        }
        word = Some(pattern);
    }
    let word = word?;
    let protection = touched.first()?.pages.2;
    if !open_band_at(pending, address, (protection, Some(word)), hooks) {
        return None;
    }
    let mut pieces = Vec::new();
    for copy in touched {
        let end = copy.destination + copy.length;
        let before = (
            copy.destination,
            band.0.min(end).saturating_sub(copy.destination),
        );
        let after_start = (band.0 + band.1).max(copy.destination);
        let after = (after_start, end.saturating_sub(after_start));
        for (destination, length) in [before, after] {
            if length == 0 {
                continue;
            }
            let span = page_span(destination, length);
            pieces.push(Deferred {
                destination,
                length,
                pages: (span.0, span.1, copy.pages.2),
                held: Held::Fill(word),
            });
        }
    }
    Some(pieces)
}

/// [`open_band_at`] for a pending frame whose target was guarded under a deferred fill just carried
/// out (D719): the target's pages are made read-only, as a written target's are (D720), and the band
/// of rows the touch at `address` falls in is handed over. `false`, with nothing left read-only,
/// when it cannot be.
fn band_after_carrying_out(
    (target, swap): (ColourTarget, ComponentSwap),
    address: u64,
    hooks: &LazyCopies,
) -> bool {
    protect_target(target);
    let protection = write_protected().lock().ok().and_then(|protected| {
        protected
            .filter(|p| p.target_base == target.base)
            .map(|p| p.protection)
    });
    if let Some(protection) = protection
        && open_band_at((target, swap), address, (protection, None), hooks)
    {
        return true;
    }
    unprotect_overlapping(target.base, target.words() as u64 * 4);
    false
}

/// Folds the open band back onto the device (D864): its rows as memory holds them now become the
/// pending frame's there, and their pages read-only again where the target is still protected.
/// Before anything takes the pending frame from the device. A band whose frame is no longer pending
/// is forgotten: that frame is gone, and memory holds what the guest wrote. `false` when the rows
/// could not be patched; the caller then writes the frame back as the device holds it.
fn fold_band() -> bool {
    let Some(band) = open_band().lock().ok().and_then(|mut open| open.take()) else {
        return true;
    };
    if pending_target().map(|(target, _)| target) != Some(band.target) {
        return true;
    }
    let Some(&(_, patch)) = frame_rows().get() else {
        return false;
    };
    let (first, count) = band.rows;
    let Some(((_, _), (start, bytes))) =
        band_at(band.target, band.pages.0).filter(|(rows, _)| *rows == band.rows)
    else {
        return false;
    };
    let memory = MappedRegions::current();
    let Some(words) = usize::try_from(bytes)
        .ok()
        .and_then(|length| memory.read(start, length))
        .map(words_of)
    else {
        return false;
    };
    let texels = band.target.layout.detile_mapped(
        &words,
        band.target.width,
        count,
        band.target.pipe_bank_xor,
        |w| loaded(w, band.target, band.swap),
    );
    let patched = crate::perf::span(crate::perf::Span::FoldBand, || patch(first, count, &texels));
    if let (Some(hooks), Ok(protected)) = (lazy_copies().get(), write_protected().lock())
        && protected.is_some_and(|p| p.target_base == band.target.base)
    {
        let _ = (hooks.protect_writes)(band.pages.0, band.pages.1);
    }
    patched
}

/// A linear target's row pitch in bytes.
fn target_row_bytes(target: ColourTarget) -> u64 {
    u64::from(target.layout.linear_pitch(target.width)) * 4
}

/// Whether a drawn target is written back at the flip (the default) or after every submission
/// (D714).
static WRITE_BACK_AT_FLIP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

fn write_back_at_flip() -> bool {
    WRITE_BACK_AT_FLIP.load(std::sync::atomic::Ordering::Relaxed)
}

/// Chooses when a drawn target is written back (D714): at the flip, or after every submission for a
/// guest that reads its own target between them. Set by the worker from
/// `ORBISTOUN_TARGET_WRITEBACK`.
pub fn set_write_back_at_flip(at_flip: bool) {
    WRITE_BACK_AT_FLIP.store(at_flip, std::sync::atomic::Ordering::Relaxed);
}

/// The target whose drawn frame is on the device and not yet in guest memory (D714).
fn pending() -> &'static Mutex<Option<(ColourTarget, ComponentSwap)>> {
    static PENDING: Mutex<Option<(ColourTarget, ComponentSwap)>> = Mutex::new(None);
    &PENDING
}

fn pending_target() -> Option<(ColourTarget, ComponentSwap)> {
    *pending()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The base of the target a drawn frame is waiting to be written back to, if one is.
///
/// Asked at a flip, before the write-back, to learn whether the buffer being flipped holds a frame
/// orbistoun drew for the guest.
#[must_use]
pub fn pending_frame_base() -> Option<u64> {
    pending_target().map(|(target, _)| target.base)
}

fn set_pending(target: Option<(ColourTarget, ComponentSwap)>) {
    *pending()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = target;
}

/// Writes the pending frame, if there is one, into guest memory (D714). `true` when there was none
/// or it was written; `false` when it could not be, which drops it rather than leaving it pending
/// forever.
fn write_back_pending(memory: &mut dyn cp::CpMemory) -> bool {
    // What the guest wrote into the frame's rows reaches the device first (D864).
    let _ = fold_band();
    let Some((target, swap)) = pending_target() else {
        return true;
    };
    set_pending(None);
    let Some(read) = frame_reader().get() else {
        return false;
    };
    let Some(after) = read() else {
        forget_written();
        return false;
    };
    let written = write_target(memory, target, swap, &after);
    if written {
        note_touched(target.base);
        let mut early = written_since_flip()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !early.contains(&target.base) {
            early.push(target.base);
        }
    }
    written
}

/// Lets go of the pending frame for a draw into another target (D885): deferred as a flip defers
/// it (D719), so a frame the next draw into its target takes as held (D801) is never written back,
/// or written back now where it cannot wait. A target whose frame, held at a switch before, was
/// carried out by a read is written back now too: deferring a frame something then reads costs a
/// snapshot and a guard over what the write-back costs. Either way it counts as written since the
/// flip, which may still show it. `true` when nothing was pending or it was kept or written.
fn hold_pending(memory: &mut dyn cp::CpMemory) -> bool {
    let Some((target, _)) = pending_target() else {
        return true;
    };
    let read_after = switch_held()
        .lock()
        .is_ok_and(|held| held.iter().any(|&(kept, read)| kept == target && read));
    if read_after || defer_write_back(false).is_err() {
        return write_back_pending(memory);
    }
    if let Ok(mut held) = switch_held().lock()
        && !held.iter().any(|&(kept, _)| kept == target)
    {
        if held.len() >= SWITCH_HELD_KEPT {
            held.remove(0);
        }
        held.push((target, false));
    }
    let mut early = written_since_flip()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !early.contains(&target.base) {
        early.push(target.base);
    }
    true
}

/// The targets whose frames a switch of target kept on the device ([`hold_pending`]), each with
/// whether such a frame was then carried out - something read or wrote the target's memory before
/// a draw took the frame as held. Never locked while [`deferred`] is waited for.
fn switch_held() -> &'static Mutex<Vec<(ColourTarget, bool)>> {
    static HELD: Mutex<Vec<(ColourTarget, bool)>> = Mutex::new(Vec::new());
    &HELD
}

/// How many targets [`switch_held`] remembers; the oldest is let go past it.
const SWITCH_HELD_KEPT: usize = 16;

/// Notes that a frame deferred onto `target` was carried out, for [`hold_pending`], and the guest
/// access that carried it out when one did.
fn note_carried_out(target: ColourTarget) {
    note_touched(target.base);
    if let Ok(mut held) = switch_held().lock()
        && let Some(kept) = held.iter_mut().find(|(kept, _)| *kept == target)
    {
        kept.1 = true;
    }
}

/// A guest access that faulted on memory a drawn frame was held back from: the instruction's address,
/// the address it touched, and whether it wrote. What brought a frame back before its flip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuestTouch {
    /// The faulting instruction's address.
    pub rip: u64,
    /// The address it read or wrote.
    pub address: u64,
    /// Whether it wrote; a read otherwise.
    pub write: bool,
}

thread_local! {
    /// The guest access the host's fault handler is answering on this thread, if it is.
    static TOUCHING: std::cell::Cell<Option<GuestTouch>> = const { std::cell::Cell::new(None) };
}

/// Runs `answer` - [`written_at`] or [`carry_out_at`] - as the answer to `touch`, so a frame it brings
/// back is noted with the access that did ([`take_early_touch`]).
pub fn with_guest_touch<R>(touch: GuestTouch, answer: impl FnOnce() -> R) -> R {
    let before = TOUCHING.with(|touching| touching.replace(Some(touch)));
    let out = answer();
    TOUCHING.with(|touching| touching.set(before));
    out
}

/// The targets whose frames a guest access brought back before their flips, each with the first
/// such access, the oldest first.
fn early_touches() -> &'static Mutex<Vec<(u64, GuestTouch)>> {
    static TOUCHES: Mutex<Vec<(u64, GuestTouch)>> = Mutex::new(Vec::new());
    &TOUCHES
}

/// How many targets [`early_touches`] remembers; the oldest is let go past it.
const EARLY_TOUCHES_KEPT: usize = 16;

/// Notes the guest access being answered on this thread, if one is, as what brought `base`'s frame
/// back.
fn note_touched(base: u64) {
    let Some(touch) = TOUCHING.with(std::cell::Cell::get) else {
        return;
    };
    let Ok(mut touches) = early_touches().lock() else {
        return;
    };
    if touches.iter().any(|&(kept, _)| kept == base) {
        return;
    }
    if touches.len() >= EARLY_TOUCHES_KEPT {
        touches.remove(0);
    }
    touches.push((base, touch));
}

/// The guest access that brought the frame drawn into the target at `address` back before its flip,
/// if one did; every access noted is forgotten, so the next flip asks afresh. Asked beside
/// [`frame_drawn_into`].
#[must_use]
pub fn take_early_touch(address: u64) -> Option<GuestTouch> {
    let touches = std::mem::take(
        &mut *early_touches()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    );
    touches
        .into_iter()
        .find_map(|(base, touch)| (base == address).then_some(touch))
}

/// Every guest access that brought a drawn frame back before its flip since this last asked, by the
/// target's base; every access noted is forgotten, as [`take_early_touch`] forgets them.
#[must_use]
pub fn take_early_touches() -> Vec<(u64, GuestTouch)> {
    std::mem::take(
        &mut *early_touches()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    )
}

/// The bases of the targets whose drawn frames were written back since a flip last asked
/// ([`frame_drawn_into`]). A frame something reads before its flip is written back then (D714) -
/// oops-gl reads its own target after each frame's draws - and is still the frame that flip shows.
fn written_since_flip() -> &'static Mutex<Vec<u64>> {
    static EARLY: Mutex<Vec<u64>> = Mutex::new(Vec::new());
    &EARLY
}

/// A texture a frame's draws bound, as it was bound: its slot, width, height, texels and what its
/// words are.
pub type HeldTexture = (u32, u32, u32, Arc<[u32]>, crate::TextureEncoding);

/// The frame last drawn into a colour target: what a run's report names behind a flip of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawnBy {
    /// Which frame drawn into a target it was, counted from the run's first.
    pub frame: u64,
    /// How many draws drew it.
    pub draws: usize,
    /// Each texture those draws bound, once.
    pub textures: Vec<HeldTexture>,
    /// Where each texture they sampled lies, with its width and height: what traces a sampled
    /// render target to the frame drawn into it.
    pub sampled: Vec<(u64, u32, u32)>,
    /// Each guest range its draws read as a buffer, rather than sampled: its first byte and its
    /// length.
    pub read: Vec<(u64, u64)>,
}

/// The frames drawn so far and the last drawn into each target, by the target's base.
fn drawn_by() -> &'static Mutex<(u64, std::collections::BTreeMap<u64, DrawnBy>)> {
    static DRAWN_BY: Mutex<(u64, std::collections::BTreeMap<u64, DrawnBy>)> =
        Mutex::new((0, std::collections::BTreeMap::new()));
    &DRAWN_BY
}

/// Notes a frame of `draws` draws, binding the textures `commands` bind and sampling those at
/// `sampled`, drawn into the colour attachments at `targets`: one frame, kept by each. The texels
/// are held, not copied, so nothing frame-sized is made per frame.
pub fn note_drawn_by(
    targets: &[u64],
    draws: usize,
    commands: &[crate::RenderCommand],
    sampled: &[(u64, u32, u32)],
) {
    let mut textures: Vec<HeldTexture> = Vec::new();
    for command in commands {
        if let crate::RenderCommand::BindTexture {
            slot,
            texels,
            width,
            height,
            encoding,
            ..
        } = command
            && !textures
                .iter()
                .any(|(bound, _, _, held, _)| bound == slot && Arc::ptr_eq(held, texels))
        {
            textures.push((*slot, *width, *height, Arc::clone(texels), *encoding));
        }
    }
    let mut store = drawn_by()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    store.0 += 1;
    let frame = store.0;
    let mut read: Vec<(u64, u64)> = Vec::new();
    for command in commands {
        if let crate::RenderCommand::BindDrawBuffers { buffers, .. } = command {
            for buffer in buffers.iter().filter(|buffer| buffer.base != 0) {
                let range = (buffer.base, buffer.bytes.len() as u64);
                if !read.contains(&range) {
                    read.push(range);
                }
            }
        }
    }
    let by = DrawnBy {
        frame,
        draws,
        textures,
        sampled: sampled.to_vec(),
        read,
    };
    for &target in targets {
        store.1.insert(target, by.clone());
    }
}

/// [`note_drawn_by`] for `submission`, drawn into the colour attachments at `targets`: one frame,
/// kept by each.
fn note_drawn(targets: &[u64], submission: &Submission) {
    let sampled: Vec<(u64, u32, u32)> = submission
        .report
        .bound_textures
        .iter()
        .map(|texture| (texture.base, texture.width, texture.height))
        .collect();
    note_drawn_by(
        targets,
        submission.report.draws,
        &submission.commands,
        &sampled,
    );
}

/// The bases of every colour attachment a frame drew into: the first, then each one more.
fn drawn_bases(
    first: ColourTarget,
    more: &[(ColourTarget, ComponentSwap, crate::ResourceId)],
) -> Vec<u64> {
    std::iter::once(first.base)
        .chain(more.iter().map(|(target, _, _)| target.base))
        .collect()
}

/// The frame last drawn into each colour target, each once.
#[must_use]
pub fn drawn_frames() -> Vec<DrawnBy> {
    let mut frames: Vec<DrawnBy> = drawn_by()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .1
        .values()
        .cloned()
        .collect();
    frames.sort_by_key(|by| by.frame);
    frames.dedup_by_key(|by| by.frame);
    frames
}

/// The frame last drawn into the colour target at `target`, where one was.
#[must_use]
pub fn last_drawn_into(target: u64) -> Option<DrawnBy> {
    drawn_by()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .1
        .get(&target)
        .cloned()
}

/// The base of the buffer a frame orbistoun drew went into, asked as the guest flips `address`:
/// the target whose frame is still waiting to be written back, or else `address` itself when its
/// frame was already written back because something read it first. The early write-backs are
/// forgotten, so the next flip asks afresh.
#[must_use]
pub fn frame_drawn_into(address: u64) -> Option<u64> {
    let early = std::mem::take(
        &mut *written_since_flip()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    );
    pending_frame_base().or_else(|| early.contains(&address).then_some(address))
}

/// Writes a pending drawn frame back where the guest scans it out, called as the guest flips,
/// before the flipped buffer is read. `true` when nothing was pending or it was written.
///
/// The write-back happens when something first reads it (D719): the pending target becomes a
/// deferred copy onto itself, carried out by the first touch of its pages. It is written now only
/// when it cannot wait.
pub fn write_back_at_this_flip() -> bool {
    write_back_at_this_flip_showing(false).0
}

/// A flipped frame kept on the device for the display (D719): a snapshot the caller owns and
/// releases, and the target and byte order it was drawn for.
#[derive(Debug, Clone, Copy)]
pub struct ShownFrame {
    /// The snapshot, taken or released through the installed [`LazyCopies`].
    pub snapshot: u64,
    /// Where the frame belongs in guest memory.
    pub target: ColourTarget,
    /// The target's component order in memory.
    pub swap: ComponentSwap,
}

/// [`write_back_at_this_flip`], and - when `show` asks and the write-back was deferred - a second
/// snapshot of the flipped frame for the display to read on its own time (D719). The display scans
/// out the target's memory, which over the visible pixels is that frame in the target's byte order
/// ([`memory_order`]); the guest cannot observe the display, so it need not wait for it.
///
/// The two snapshots are taken back to back on this thread. A draw another guest thread submits in
/// between reaches only the display's copy, never guest memory.
pub fn write_back_at_this_flip_showing(show: bool) -> (bool, Option<ShownFrame>) {
    crate::perf::span(
        crate::perf::Span::FlipWriteBack,
        || match defer_write_back(show) {
            Ok(shown) => (true, shown),
            Err(CannotWait) => (
                write_back_pending(&mut GuestCp {
                    memory: MappedRegions::current(),
                    submission: None,
                }),
                None,
            ),
        },
    )
}

/// A linear `Rgba8` word as a target in `swap` order holds it in memory.
#[must_use]
pub const fn memory_order(word: u32, swap: ComponentSwap) -> u32 {
    swapped(word, swap)
}

/// The pending frame's write-back, deferred (D719): a [`Deferred`] copy of the frame onto its own
/// target, from the target's memory as the draws started from it - which memory still holds, since
/// anything that touched the target since would have written the frame back. `false` when it
/// cannot wait; the caller then writes it back now.
fn defer_write_back(show: bool) -> Result<Option<ShownFrame>, CannotWait> {
    // What the guest wrote into the frame's rows reaches the device before it is kept (D864).
    if !fold_band() {
        return Err(CannotWait);
    }
    let hooks = lazy_copies().get().ok_or(CannotWait)?;
    let (target, swap) = pending_target().ok_or(CannotWait)?;
    let span = target.words() as u64 * 4;
    let memory_then = match last_written().lock().ok().and_then(|last| last.clone()) {
        Some(written) if written.target == target && written.contents.len() as u64 == span => {
            written.contents
        }
        _ => return Err(CannotWait),
    };
    let writable = write_lookup()
        .get()
        .is_some_and(|allows| allows(target.base, span));
    if !writable {
        return Err(CannotWait);
    }
    let pages = page_span(target.base, span);
    let mut list = deferred().lock().map_err(|_| CannotWait)?;
    // Guarded another way now: a protection kept would be saved as the one to restore (D720).
    unprotect_overlapping(pages.0, pages.1);
    // A write-back still deferred from an earlier flip of this target is superseded, unread: this
    // one's frame was drawn over it and writes every byte it would.
    let superseded = list
        .iter()
        .position(|copy| copy.destination == target.base && copy.length == span);
    let guard = superseded.map(|index| {
        crate::perf::span(crate::perf::Span::DeferredSuperseded, || ());
        let old = list.remove(index);
        crate::perf::span(crate::perf::Span::DiscardSnapshot, || {
            old.release_snapshot(hooks);
        });
        old.pages.2
    });
    let (touched, kept): (Vec<Deferred>, Vec<Deferred>) = std::mem::take(&mut *list)
        .into_iter()
        .partition(|copy| ranges_overlap((copy.pages.0, copy.pages.1), pages));
    *list = kept;
    let carried = !touched.is_empty();
    // A fill left around a band (D864) lies inside the target, and the frame's write-back writes
    // every byte of it over the fill's own word, which `memory_then` holds: it is dropped unwritten.
    let (dropped, touched): (Vec<Deferred>, Vec<Deferred>) = touched.into_iter().partition(|copy| {
        matches!((&copy.held, &memory_then), (Held::Fill(word), Contents::Uniform { word: then, .. })
            if word == then)
            && copy.destination >= target.base
            && copy.destination + copy.length <= target.base + span
    });
    for copy in &dropped {
        crate::perf::span(crate::perf::Span::DeferredFillDropped, || ());
        (hooks.release)(copy.pages.0, copy.pages.1, copy.pages.2);
    }
    carry_out_all(&touched, hooks);
    if carried && guard.is_some() && (hooks.guard)(pages.0, pages.1).is_none() {
        return Err(CannotWait);
    }
    let Some(snapshot) = crate::perf::span(crate::perf::Span::KeepSnapshot, hooks.snapshot) else {
        if let Some(protection) = guard {
            (hooks.release)(pages.0, pages.1, protection);
        }
        return Err(CannotWait);
    };
    let Some(protection) = guard.or_else(|| {
        crate::perf::span(crate::perf::Span::CopyGuard, || {
            (hooks.guard)(pages.0, pages.1)
        })
    }) else {
        (hooks.discard)(snapshot);
        return Err(CannotWait);
    };
    list.push(Deferred {
        destination: target.base,
        length: span,
        pages: (pages.0, pages.1, protection),
        held: Held::Frame {
            snapshot,
            target,
            swap,
            memory_then,
        },
    });
    set_pending(None);
    // The display's own copy, beside memory's: released by whoever shows it.
    let shown = show
        .then(|| crate::perf::span(crate::perf::Span::KeepSnapshot, hooks.show))
        .flatten()
        .map(|snapshot| ShownFrame {
            snapshot,
            target,
            swap,
        });
    Ok(shown)
}

/// A write-back that cannot be deferred, and is written now instead (D719).
struct CannotWait;

/// Drops the pending frame, unwritten, when `[address, address + length)` overwrites its whole
/// target (D719): every byte the write-back would produce is overwritten before anything can read
/// it. What memory held before the draws is forgotten too, so the next submission reads the target
/// in full rather than handing the drawer its own frame instead of the fill.
fn drop_pending_covered(address: u64, length: u64) {
    let Some((target, _)) = pending_target() else {
        return;
    };
    let span = target.words() as u64 * 4;
    if target.base >= address && target.base.saturating_add(span) <= address.saturating_add(length)
    {
        set_pending(None);
        forget_written();
    }
}

/// Turns the one deferred copy or fill guarding exactly `[address, address + length)` into a
/// deferred fill of `pattern` there, its pages left guarded (D719): a fill that overwrites it whole
/// would drop it, giving the pages back, and guard them again at once - a dispatch filling the same
/// buffer every time it runs did both, some 0.6 ms a dispatch (PPSA28061). `false`, changing
/// nothing, where no single deferral guards exactly those pages or the fill would not be deferred.
fn refill_guarded(address: u64, pattern: u32, length: u64) -> bool {
    let Some(hooks) = lazy_copies().get() else {
        return false;
    };
    if length < DEFERRED_FILL_MIN
        || address % HOST_PAGE != 0
        || length % HOST_PAGE != 0
        || pending_target().is_some_and(|(target, _)| overlaps_target(target, address, length))
    {
        return false;
    }
    let Ok(mut list) = deferred().lock() else {
        return false;
    };
    let mut over = list
        .iter()
        .enumerate()
        .filter(|(_, copy)| ranges_overlap((copy.pages.0, copy.pages.1), (address, length)));
    let (Some((index, copy)), None) = (over.next(), over.next()) else {
        return false;
    };
    if (copy.pages.0, copy.pages.1) != (address, length)
        || copy.destination < address
        || copy.destination.saturating_add(copy.length) > address.saturating_add(length)
    {
        return false;
    }
    let pages = copy.pages;
    let old = std::mem::replace(
        &mut list[index],
        Deferred {
            destination: address,
            length,
            pages,
            held: Held::Fill(pattern),
        },
    );
    crate::perf::span(crate::perf::Span::DeferredRefilled, || {
        old.release_snapshot(hooks);
    });
    true
}

/// Drops, unread, every deferred copy whose destination `[address, address + length)` overwrites
/// completely (D719): nothing has observed its bytes, and nothing will. Its pages get their
/// protection back and its snapshot is released.
fn drop_covered(address: u64, length: u64) {
    let Some(hooks) = lazy_copies().get() else {
        return;
    };
    let Ok(mut list) = deferred().lock() else {
        return;
    };
    let end = address.saturating_add(length);
    let (covered, kept): (Vec<Deferred>, Vec<Deferred>) =
        std::mem::take(&mut *list).into_iter().partition(|copy| {
            copy.destination >= address && copy.destination.saturating_add(copy.length) <= end
        });
    *list = kept;
    for copy in covered {
        crate::perf::span(crate::perf::Span::DeferredCovered, || ());
        let (base, len, protection) = copy.pages;
        // Its pages come back as they were, and the fill that follows writes them. Pages that would
        // not come back are carried out instead, never left guarded with nothing to answer the
        // fault.
        if !(hooks.release)(base, len, protection) {
            carry_out(&copy, hooks);
            continue;
        }
        if let Ok(mut resolved) = resolved().lock() {
            if resolved.len() >= RESOLVED_KEPT {
                resolved.remove(0);
            }
            resolved.push((base, len));
        }
        copy.release_snapshot(hooks);
    }
}

/// Reads `target` out of `memory`, has `draw` turn its `Rgba8` contents into the contents after the
/// draws, and writes that back tiled and in the target's byte order. `false`, with nothing written,
/// when the target is not readable, `draw` declines or answers the wrong size, or the write is
/// refused - a target outside the guest's memory is never written.
fn draw_over(
    memory: &mut dyn cp::CpMemory,
    target: ColourTarget,
    swap: ComponentSwap,
    draw: impl FnOnce(Before<'_>) -> Option<Vec<u32>>,
) -> bool {
    let Some(before) = read_target(memory, target, swap) else {
        return false;
    };
    let Some(after) = draw(before.before()) else {
        // A drawer that failed part-way may hold draws the target does not: it is not trusted as
        // "unchanged" until a frame has been written.
        forget_written();
        return false;
    };
    write_target(memory, target, swap, &after)
}

/// Reads `target` out of `memory` as linear `Rgba8` for a drawer to start from, or answers that it
/// is unchanged since this last wrote or read it, so the drawer already holds it. `None` when the
/// target is not readable.
///
/// Unchanged is asked first, in place: a GL frame is many submissions into one target.
fn read_target(
    memory: &dyn cp::CpMemory,
    target: ColourTarget,
    swap: ComponentSwap,
) -> Option<TargetRead> {
    let read_started = std::time::Instant::now();
    let length = target.words() * 4;
    // Unchanged is asked of the host first: whether any page of the target has been written since
    // this last wrote or read it. Only where the host cannot say, or says it was, are the bytes
    // compared - a write of the same bytes is still unchanged. Read-only and unwritten (D720) is
    // asked first, since write-watch cannot answer for guest direct memory's mapped views. The
    // pages are protected again before they are compared or read, so a racing write is seen next
    // time rather than lost.
    // A fill still deferred over the whole target is what memory holds, as far as anything can see:
    // a touch would have carried it out. Uniform, and nothing to read.
    if let Some(word) = deferred_fill_over(target.base, length as u64) {
        let now = Contents::Uniform { word, length };
        let unchanged = last_written().lock().is_ok_and(|last| {
            last.as_ref().is_some_and(|written| {
                written.target == target
                    && matches!(written.contents, Contents::Uniform { word: held, length: kept }
                        if held == word && kept == length)
            })
        });
        if !unchanged {
            replace_written(Some(Written {
                target,
                contents: now,
                since: None,
            }));
        }
        crate::perf::add(crate::perf::Phase::ReadTarget, read_started.elapsed());
        return Some(if unchanged {
            TargetRead::Unchanged
        } else {
            TargetRead::of_pattern(word, target, swap)
        });
    }
    // The frame this target held, written back to it at its flip and still deferred (D719): nothing
    // has touched the target since - the copy's guard would have carried it out - and the drawer,
    // which keeps each target it draws, holds that frame (D801).
    if let Some(found) = written_for(target, length)
        && deferred_self_frames(target, &found.contents)
    {
        replace_written(Some(found));
        crate::perf::add(crate::perf::Phase::ReadTarget, read_started.elapsed());
        return Some(TargetRead::HeldOverDeferred);
    }
    let was_protected = protected_unwritten(target);
    protect_target(target);
    let unchanged = crate::perf::span(crate::perf::Span::ReadTargetCompare, || {
        last_written().lock().is_ok_and(|last| {
            last.as_ref().is_some_and(|written| {
                written.target == target
                    && written.contents.len() == length
                    && (was_protected
                        || written.since.and_then(|since| {
                            orbistoun_mem::watch::written_since(target.base, length as u64, since)
                        }) == Some(false)
                        || memory.holds(target.base, &written.contents.bytes()))
            })
        })
    }) || crate::perf::span(crate::perf::Span::ReadTargetEarlier, || {
        held_earlier(memory, target, length)
    });
    if unchanged {
        crate::perf::add(crate::perf::Phase::ReadTarget, read_started.elapsed());
        return Some(TargetRead::Unchanged);
    }
    let _read = crate::perf::SpanGuard::start(crate::perf::Span::ReadTargetRead);
    // Marked before it is read, so a write that races the read is seen next time rather than lost.
    let since = orbistoun_mem::watch::mark(target.base, length as u64);
    let bytes = memory.read(target.base, length)?;
    if bytes.len() < length {
        return None;
    }
    // A target of one word throughout - a clear - needs no detile: every pixel is that word in the
    // target's byte order, wherever the tiling puts it.
    let first = bytes.first_chunk::<4>().map(|w| u32::from_le_bytes(*w));
    let uniform = first.filter(|&word| {
        bytes
            .chunks_exact(4)
            .all(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]) == word)
    });
    let before = match uniform {
        Some(word) => TargetRead::of_pattern(word, target, swap),
        None => TargetRead::Changed(
            target.detile_mapped(&words_of(&bytes), |w| loaded(w, target, swap)),
        ),
    };
    // What the drawer is handed is what memory holds now: with the frame kept on the device until
    // the flip, memory still holding these bytes means nothing else has written the target since
    // (D714).
    replace_written(Some(Written {
        target,
        contents: Contents::Bytes(Arc::new(bytes)),
        since,
    }));
    crate::perf::add(crate::perf::Phase::ReadTarget, read_started.elapsed());
    Some(before)
}

/// What reading a colour target found.
#[derive(Debug)]
enum TargetRead {
    /// Memory holds exactly what this last wrote or read there: the drawer already has it.
    Unchanged,
    /// Memory will hold the frame the drawer last drew there, once the write-back still deferred
    /// over it is carried out (D801): the drawer already has it.
    HeldOverDeferred,
    /// Something else wrote it: its contents, as linear `Rgba8`.
    Changed(Vec<u32>),
    /// Something else wrote it with one word throughout - a clear: that word as linear `Rgba8`.
    Uniform(u32),
}

impl TargetRead {
    /// A target whose memory is `word` throughout: every texel that word, or - one or two bytes a
    /// texel, the word's texels not all alike - the texels it makes where the layout puts them.
    fn of_pattern(word: u32, target: ColourTarget, swap: ComponentSwap) -> Self {
        // The word's texels: four bytes, two halves, or the word itself.
        let alike = match target.layout.texel_log2() {
            0 => word
                .to_le_bytes()
                .iter()
                .all(|&b| b == word.to_le_bytes()[0]),
            1 => word >> 16 == word & 0xFFFF,
            _ => true,
        };
        if alike {
            Self::Uniform(loaded(word, target, swap))
        } else {
            Self::Changed(
                target.detile_mapped(&vec![word; target.words()], |w| loaded(w, target, swap)),
            )
        }
    }

    /// What a drawer is handed of it.
    fn before(&self) -> Before<'_> {
        match self {
            Self::Unchanged | Self::HeldOverDeferred => Before::Held,
            Self::Changed(contents) => Before::Words(contents),
            Self::Uniform(word) => Before::Uniform(*word),
        }
    }
}

/// Little-endian bytes as words.
fn words_of(bytes: &[u8]) -> Vec<u32> {
    bytes
        .chunks_exact(4)
        .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
        .collect()
}

/// Tiles `after` over the target in its byte order and writes it where the guest reads it,
/// remembering what was written. The surface's padding words - a partial edge block's - are kept as
/// memory holds them. `false` when the target is unreadable, the size is wrong, or the write
/// refused.
fn write_target(
    memory: &mut dyn cp::CpMemory,
    target: ColourTarget,
    swap: ComponentSwap,
    after: &[u32],
) -> bool {
    crate::perf::measure(crate::perf::Phase::WriteTarget, || {
        // Tiled straight into the target, in place, rather than read out, converted and written
        // again.
        let words = target.words();
        let mut tiled_ok = false;
        let mut written = spare_written();
        let edited = memory.edit_words(target.base, words, &mut |tiled| {
            tiled_ok = target
                .tile_mapped(after, tiled, |w| stored(w, target, swap))
                .is_ok();
            // What memory holds now, for the next submission's unchanged check.
            written.clear();
            written.extend_from_slice(zerocopy::IntoBytes::as_bytes(&*tiled));
        });
        let wrote = edited && tiled_ok;
        // Marked after this write, so the next check does not see this write as someone else's.
        let since = orbistoun_mem::watch::mark(target.base, words as u64 * 4);
        replace_written(wrote.then(|| Written {
            target,
            contents: Contents::Bytes(Arc::new(written)),
            since,
        }));
        // Protected from here, as write-watch is marked from here (D720).
        if wrote {
            protect_target(target);
        }
        wrote
    })
}

/// The target this last wrote back or read, and the bytes memory held then. Shared, so a deferred
/// copy of the target can keep them without copying.
fn last_written() -> &'static Mutex<Option<Written>> {
    static LAST: Mutex<Option<Written>> = Mutex::new(None);
    &LAST
}

/// How many targets before the last this keeps the bytes of (D801): a frame's display buffers
/// alternate, so the one a submission draws into is the one before the last.
const EARLIER_KEPT: usize = 3;

/// The targets this wrote or read before [`last_written`], most recent first, each with the bytes
/// its memory held then (D801). Nothing but their bytes vouches for them: the protection and the
/// write-watch mark follow only the last.
fn earlier_written() -> &'static Mutex<Vec<Written>> {
    static EARLIER: Mutex<Vec<Written>> = Mutex::new(Vec::new());
    &EARLIER
}

/// Makes `next` the last target written or read, keeping the one it replaces among the earlier
/// ones when it is another target's bytes (D801).
fn replace_written(next: Option<Written>) {
    let Ok(mut last) = last_written().lock() else {
        return;
    };
    let replaced = std::mem::replace(&mut *last, next);
    let current = last.as_ref().map(|written| written.target);
    let Ok(mut earlier) = earlier_written().lock() else {
        return;
    };
    // The current target's older bytes go - a display buffer's, written again two frames on - and
    // their storage holds the next.
    let (gone, kept): (Vec<Written>, Vec<Written>) = std::mem::take(&mut *earlier)
        .into_iter()
        .partition(|kept| Some(kept.target) == current);
    *earlier = kept;
    for old in gone {
        keep_spare(old.contents);
    }
    match replaced {
        Some(old) if Some(old.target) != current && matches!(old.contents, Contents::Bytes(_)) => {
            earlier.retain(|kept| kept.target != old.target);
            earlier.insert(0, old);
            earlier.truncate(EARLIER_KEPT);
        }
        // The same target written again: its old bytes go, and their storage holds the next.
        Some(old) => keep_spare(old.contents),
        None => {}
    }
}

/// Storage for the bytes [`write_target`] keeps of a target, from bytes kept before and let go:
/// a target is megabytes, and a fresh allocation of that comes from the system a page at a time,
/// every write-back (GLTC00001, one a frame).
fn spare_written() -> Vec<u8> {
    spare()
        .lock()
        .ok()
        .and_then(|mut spare| spare.take())
        .unwrap_or_default()
}

/// Keeps `contents`' bytes as [`spare_written`]'s next storage, where nothing else holds them.
fn keep_spare(contents: Contents) {
    if let Contents::Bytes(bytes) = contents
        && let Ok(bytes) = Arc::try_unwrap(bytes)
        && let Ok(mut spare) = spare().lock()
    {
        *spare = Some(bytes);
    }
}

fn spare() -> &'static Mutex<Option<Vec<u8>>> {
    static SPARE: Mutex<Option<Vec<u8>>> = Mutex::new(None);
    &SPARE
}

/// What this recorded of `target`'s memory when it last wrote or read it, from the last target or
/// an earlier one, where it recorded `length` bytes.
fn written_for(target: ColourTarget, length: usize) -> Option<Written> {
    let matching = |written: &Written| written.target == target && written.contents.len() == length;
    if let Some(last) = last_written()
        .lock()
        .ok()
        .and_then(|last| last.clone())
        .filter(matching)
    {
        return Some(last);
    }
    earlier_written()
        .lock()
        .ok()
        .and_then(|earlier| earlier.iter().find(|kept| matching(kept)).cloned())
}

/// Whether every deferred write over `target` is a write-back of its own frame onto it whole, taken
/// over exactly the bytes `contents` records (D801) - and there is one.
fn deferred_self_frames(target: ColourTarget, contents: &Contents) -> bool {
    let Contents::Bytes(kept) = contents else {
        return false;
    };
    let span = target.words() as u64 * 4;
    let Ok(list) = deferred().lock() else {
        return false;
    };
    let mut over = list
        .iter()
        .filter(|copy| ranges_overlap((copy.destination, copy.length), (target.base, span)))
        .peekable();
    over.peek().is_some()
        && over.all(|copy| {
            copy.destination == target.base
                && copy.length == span
                && matches!(&copy.held, Held::Frame { target: from, memory_then: Contents::Bytes(then), .. }
                    if *from == target && Arc::ptr_eq(then, kept))
        })
}

/// Whether `target` is one this wrote or read before the last and its memory still holds exactly
/// the bytes it held then (D801): nothing has written it since, so the drawer, which keeps each
/// target it draws, already holds it. It becomes the last again.
fn held_earlier(memory: &dyn cp::CpMemory, target: ColourTarget, length: usize) -> bool {
    // Marked before the bytes are compared, so a write that races the compare is seen next time.
    let since = orbistoun_mem::watch::mark(target.base, length as u64);
    let found = earlier_written().lock().ok().and_then(|mut earlier| {
        let at = earlier.iter().position(|kept| {
            kept.target == target
                && kept.contents.len() == length
                && memory.holds(target.base, &kept.contents.bytes())
        })?;
        Some(earlier.remove(at))
    });
    let Some(mut found) = found else {
        return false;
    };
    found.since = since;
    replace_written(Some(found));
    true
}

/// A target, the bytes its memory held when this last wrote or read it, and the host's mark
/// of that moment when it keeps one.
#[derive(Debug, Clone)]
struct Written {
    target: ColourTarget,
    contents: Contents,
    since: Option<u64>,
}

/// What a target's memory held when this last wrote or read it: its bytes, or - under a fill
/// still deferred over the whole of it (D719) - one word throughout, kept without the bytes.
#[derive(Debug, Clone)]
enum Contents {
    Bytes(Arc<Vec<u8>>),
    Uniform { word: u32, length: usize },
}

impl Contents {
    /// How many bytes.
    fn len(&self) -> usize {
        match self {
            Self::Bytes(bytes) => bytes.len(),
            Self::Uniform { length, .. } => *length,
        }
    }

    /// As words.
    fn words(&self) -> Vec<u32> {
        match self {
            Self::Bytes(bytes) => words_of(bytes),
            Self::Uniform { word, length } => vec![*word; length / 4],
        }
    }

    /// As bytes.
    fn bytes(&self) -> std::borrow::Cow<'_, [u8]> {
        match self {
            Self::Bytes(bytes) => std::borrow::Cow::Borrowed(bytes.as_slice()),
            Self::Uniform { word, length } => std::borrow::Cow::Owned(
                word.to_le_bytes()
                    .into_iter()
                    .cycle()
                    .take(*length)
                    .collect(),
            ),
        }
    }
}

/// Forgets [`last_written`], so the next submission hands its drawer the target as it is.
fn forget_written() {
    if let Ok(mut last) = last_written().lock() {
        *last = None;
    }
    if let Ok(mut earlier) = earlier_written().lock() {
        earlier.clear();
    }
    unprotect_overlapping(0, u64::MAX);
}

/// The target whose pages are read-only while it is trusted unchanged (D720): its whole host pages,
/// and the protection they get back.
#[derive(Debug, Clone, Copy)]
struct WriteProtected {
    target_base: u64,
    pages: (u64, u64),
    protection: u32,
}

fn write_protected() -> &'static Mutex<Option<WriteProtected>> {
    static PROTECTED: Mutex<Option<WriteProtected>> = Mutex::new(None);
    &PROTECTED
}

/// Makes the target read-only while it is trusted unchanged (D720), so a write to it by anything
/// is seen. Where the host cannot, or another protected target cannot be released, nothing is
/// protected and "unchanged" is answered by comparing.
fn protect_target(target: ColourTarget) {
    let Some(hooks) = lazy_copies().get() else {
        return;
    };
    let span = target.words() as u64 * 4;
    let pages = page_span(target.base, span);
    // A deferred copy guarding these pages would be carried out by the next reader; protecting them
    // now would save its no-access as the protection to restore. The list is held throughout, so no
    // deferral lands on these pages meanwhile, and taken first, as everywhere both are held.
    let Ok(list) = deferred().lock() else {
        return;
    };
    if list
        .iter()
        .any(|copy| ranges_overlap((copy.pages.0, copy.pages.1), pages))
    {
        crate::perf::span(crate::perf::Span::ProtectUnderCopy, || ());
        return;
    }
    let Ok(mut protected) = write_protected().lock() else {
        return;
    };
    if protected.is_some_and(|p| p.target_base == target.base) {
        return;
    }
    if let Some(old) = protected.take() {
        release_protection(old, hooks);
    }
    if let Some(protection) = (hooks.protect_writes)(pages.0, pages.1) {
        *protected = Some(WriteProtected {
            target_base: target.base,
            pages,
            protection,
        });
    } else {
        crate::perf::span(crate::perf::Span::ProtectRefused, || ());
    }
}

/// Whether `target` has been read-only, unwritten, since it was last written or read (D720).
fn protected_unwritten(target: ColourTarget) -> bool {
    let banded = open_band()
        .lock()
        .is_ok_and(|open| open.is_some_and(|band| band.target.base == target.base));
    !banded
        && write_protected()
            .lock()
            .is_ok_and(|protected| protected.is_some_and(|p| p.target_base == target.base))
}

/// Gives a protected target's pages back their protection when `[address, address + length)`
/// touches them - before this process writes there, or guards them another way (D720). The target
/// is no longer vouched for, so the next check compares.
fn unprotect_overlapping(address: u64, length: u64) {
    let Some(hooks) = lazy_copies().get() else {
        return;
    };
    let Ok(mut protected) = write_protected().lock() else {
        return;
    };
    if let Some(p) = *protected
        && ranges_overlap(p.pages, (address, length))
    {
        if length != u64::MAX {
            crate::perf::span(crate::perf::Span::ProtectHostWrote, || ());
        }
        release_protection(p, hooks);
        *protected = None;
    }
}

/// Puts a protected target's pages back, and remembers the range as resolved, so a thread whose
/// write faulted on them meanwhile retries rather than reporting a fault.
fn release_protection(p: WriteProtected, hooks: &LazyCopies) {
    (hooks.release)(p.pages.0, p.pages.1, p.protection);
    if let Ok(mut resolved) = resolved().lock() {
        if resolved.len() >= RESOLVED_KEPT {
            resolved.remove(0);
        }
        resolved.push(p.pages);
    }
}

/// A write to guest memory faulted: when `address` is in the protected target, its pages get their
/// protection back and the write can be retried (D720). The host's fault handler asks this for a
/// faulting write, before [`carry_out_at`].
///
/// A frame of that target still on the device (D714) is written back first, so the write lands on
/// the frame as it does on hardware, where the draws are in memory by then. Let through onto the
/// memory from before the draws, the next submission would start from that and lose them.
pub fn written_at(address: u64) -> bool {
    let Some(hooks) = lazy_copies().get() else {
        return false;
    };
    {
        let Ok(mut protected) = write_protected().lock() else {
            return false;
        };
        match *protected {
            Some(p) if ranges_overlap(p.pages, (address, 1)) => {
                // A write into the pending frame's rows: only their band is handed over (D864),
                // the rest kept read-only and on the device.
                if let Some(pending) = pending_target()
                    .filter(|(target, _)| target.base == p.target_base)
                    .filter(|(target, _)| overlaps_target(*target, address, 1))
                    && open_band_at(pending, address, (p.protection, None), hooks)
                {
                    return true;
                }
                crate::perf::span(crate::perf::Span::ProtectGuestWrote, || ());
                release_protection(p, hooks);
                *protected = None;
            }
            _ => return false,
        }
    }
    if pending_target().is_some_and(|(target, _)| overlaps_target(target, address, 1)) {
        let mut memory = GuestCp {
            memory: MappedRegions::current(),
            submission: None,
        };
        // A frame that cannot be read back is dropped by the write-back, which forgets what memory
        // held: the next submission then reads the target in full.
        let _ = write_back_pending(&mut memory);
    }
    true
}

/// Writes `bytes` into guest memory at `address` - `false`, writing nothing, when the range is not
/// writable guest memory.
fn write_guest(address: u64, bytes: &[u8]) -> bool {
    if !write_lookup()
        .get()
        .is_some_and(|allows| allows(address, bytes.len() as u64))
    {
        return false;
    }
    unprotect_overlapping(address, bytes.len() as u64);
    copy_into_guest(address, bytes)
}

/// [`write_guest`] into pages the caller has made writable already: no protection is released.
fn copy_into_guest(address: u64, bytes: &[u8]) -> bool {
    let writable = write_lookup()
        .get()
        .is_some_and(|allows| allows(address, bytes.len() as u64));
    let Ok(dest) = usize::try_from(address) else {
        return false;
    };
    if !writable {
        return false;
    }
    orbistoun_mem::watch::release_overlapping(address, bytes.len() as u64);
    // SAFETY: the installed lookup vouched that `[address, address + len)` lies wholly inside one
    // identity-mapped guest mapping whose protection allows writes: `len` writable bytes of this
    // process. A deferred copy's destination and a protected target (D720) have their host
    // protection restored by the caller - [`write_guest`], or a band's opener (D864) - so nothing
    // here faults.
    unsafe {
        std::ptr::copy_nonoverlapping(
            bytes.as_ptr(),
            std::ptr::with_exposed_provenance_mut::<u8>(dest),
            bytes.len(),
        );
    }
    true
}

/// Runs `edit` over `words` words of guest memory at `address`, in place, and answers what it
/// does - `false`, editing nothing, when the range is not writable guest memory or not word-aligned.
fn edit_guest_words(address: u64, words: usize, edit: impl FnOnce(&mut [u32]) -> bool) -> bool {
    let length = words as u64 * 4;
    if address % 4 != 0
        || !write_lookup()
            .get()
            .is_some_and(|allows| allows(address, length))
    {
        return false;
    }
    let Ok(at) = usize::try_from(address) else {
        return false;
    };
    unprotect_overlapping(address, length);
    orbistoun_mem::watch::release_overlapping(address, length);
    // SAFETY: the installed lookup vouched that `[address, address + length)` lies wholly inside
    // one identity-mapped guest mapping whose protection allows writes, and `address` is
    // word-aligned, so it is `words` writable, aligned words of this process. Its pages' protection
    // is restored above or by the caller, and nothing else in this process holds a reference into
    // them meanwhile: a guest thread touching a deferred copy's pages waits on the list its caller
    // holds.
    let slice = unsafe {
        std::slice::from_raw_parts_mut(std::ptr::with_exposed_provenance_mut::<u32>(at), words)
    };
    edit(slice)
}

/// Fills `[address, address + length)` of guest memory with `pattern`, starting at `address` -
/// `false`, writing nothing, when the range is not writable guest memory.
fn fill_guest(address: u64, length: u64, pattern: u32) -> bool {
    let writable = write_lookup()
        .get()
        .is_some_and(|allows| allows(address, length));
    let (Ok(dest), Ok(count)) = (usize::try_from(address), usize::try_from(length)) else {
        return false;
    };
    if !writable {
        return false;
    }
    unprotect_overlapping(address, length);
    // SAFETY: the installed lookup vouched that `[address, address + length)` lies inside
    // identity-mapped guest mappings whose protection allows writes, and a protected target's
    // pages among them were given their protection back above: `count` writable bytes of this
    // process. The guest thread that owns them is blocked in the submit this fill belongs to.
    let target = unsafe {
        std::slice::from_raw_parts_mut(std::ptr::with_exposed_provenance_mut::<u8>(dest), count)
    };
    // Whole words as words, since a frame's clears are megabytes. A misaligned destination or a
    // count that is not whole words keeps the byte loop; the pattern starts at the destination
    // either way.
    let whole = count - count % 4;
    let (words, tail) = target.split_at_mut(whole);
    match <[u32] as zerocopy::FromBytes>::mut_from_bytes(words) {
        Ok(words) => words.fill(u32::from_le_bytes(pattern.to_le_bytes())),
        Err(_) => {
            for chunk in words.chunks_mut(4) {
                chunk.copy_from_slice(&pattern.to_le_bytes());
            }
        }
    }
    tail.copy_from_slice(&pattern.to_le_bytes()[..tail.len()]);
    true
}

/// The smallest fill worth deferring: below it, guarding and releasing the pages costs more than
/// the fill.
const DEFERRED_FILL_MIN: u64 = 0x1_0000;

/// Defers a fill of whole host pages (D719): its pages are guarded and it is carried out by the
/// first touch, as a deferred copy is. A frame's clears are megabytes each, and the next draw only
/// asks what the target holds, which a fill answers without memory ([`deferred_fill_over`]).
/// `false` when it is not deferred and must be written now.
fn defer_fill(address: u64, pattern: u32, length: u64) -> bool {
    let Some(hooks) = lazy_copies().get() else {
        return false;
    };
    if length < DEFERRED_FILL_MIN || address % HOST_PAGE != 0 || length % HOST_PAGE != 0 {
        return false;
    }
    if !write_lookup()
        .get()
        .is_some_and(|allows| allows(address, length))
    {
        return false;
    }
    // A frame still on the device that this fill only partly covers is written back first, on the
    // path that writes now; one it covers wholly was dropped before this is asked.
    if pending_target().is_some_and(|(target, _)| overlaps_target(target, address, length)) {
        return false;
    }
    let Ok(mut list) = deferred().lock() else {
        return false;
    };
    // Guarded another way now: a protection kept would be saved as the one to restore (D720).
    unprotect_overlapping(address, length);
    // Anything deferred that this fill only partly covers is carried out first, so the guard below
    // is the only one on these pages.
    let (touched, kept): (Vec<Deferred>, Vec<Deferred>) = std::mem::take(&mut *list)
        .into_iter()
        .partition(|copy| ranges_overlap((copy.pages.0, copy.pages.1), (address, length)));
    *list = kept;
    if !carry_out_all(&touched, hooks) {
        return false;
    }
    let Some(protection) = (hooks.guard)(address, length) else {
        return false;
    };
    list.push(Deferred {
        destination: address,
        length,
        pages: (address, length, protection),
        held: Held::Fill(pattern),
    });
    true
}

/// The word a fill still deferred over all of `[base, base + length)` puts there, if one is.
fn deferred_fill_over(base: u64, length: u64) -> Option<u32> {
    let list = deferred().lock().ok()?;
    list.iter().find_map(|copy| match copy.held {
        Held::Fill(pattern)
            if copy.destination <= base
                && base.saturating_add(length) <= copy.destination.saturating_add(copy.length)
                && (base - copy.destination) % 4 == 0 =>
        {
            Some(pattern)
        }
        _ => None,
    })
}

/// Whether `[address, address + length)` touches any byte of `target`'s tiled surface.
fn overlaps_target(target: ColourTarget, address: u64, length: u64) -> bool {
    let span = target.words() as u64 * 4;
    address < target.base.saturating_add(span) && target.base < address.saturating_add(length)
}

/// What a copy out of a colour target whose frame is still on the device needs from the host to be
/// carried out lazily (D717): a snapshot of the frame on the device, and page protection on the
/// copy's destination, so the bytes are produced the first time anything touches them - exactly the
/// bytes the hardware's copy would have left there.
#[derive(Debug, Clone, Copy)]
pub struct LazyCopies {
    /// Keeps the frame the executor last drew, as it stands now; an id to take it by.
    pub snapshot: fn() -> Option<u64>,
    /// Keeps the frame the executor last drew for the display: at the extent it was drawn at, which
    /// a render scale makes larger than the target's (D862). Taken by the display, never written to
    /// guest memory.
    pub show: fn() -> Option<u64>,
    /// The kept frame's pixels as linear `Rgba8` words, releasing it: read into the storage of the
    /// vector given, whatever it held: the storage of the frame carried out before, so a frame of
    /// megabytes is not allocated afresh every time.
    pub take: fn(u64, Vec<u32>) -> Option<Vec<u32>>,
    /// Releases a kept frame unread.
    pub discard: fn(u64),
    /// Releases kept frame `old` unread and keeps the frame as it stands now, as one step - what a
    /// copy superseding another does. The new id, or `None` with `old` released.
    pub replace: fn(u64) -> Option<u64>,
    /// Makes `[base, base + len)` (whole host pages) inaccessible; the protection to restore.
    pub guard: fn(u64, u64) -> Option<u32>,
    /// Makes `[base, base + len)` (whole host pages) read-only (D720); the protection to restore.
    pub protect_writes: fn(u64, u64) -> Option<u32>,
    /// Restores a guarded range's protection.
    pub release: fn(u64, u64, u32) -> bool,
}

fn lazy_copies() -> &'static OnceLock<LazyCopies> {
    static HOOKS: OnceLock<LazyCopies> = OnceLock::new();
    &HOOKS
}

/// Installs what a lazy copy needs. First install wins; without one every copy out of a pending
/// target writes the frame back first, which is exact and slow.
pub fn install_lazy_copies(hooks: LazyCopies) {
    let _ = lazy_copies().set(hooks);
}

/// Installs what is told, once per thread and with a name for it, that a thread does this module's
/// work: the guest thread that submits command buffers ("submitting") and the command processor's
/// own ("command processor"). A profiler watches them, since in a title drawing from a thread of its
/// own neither is the thread that started it. First install wins.
pub fn install_working_thread(hook: fn(&'static str)) {
    let _ = working_thread().set(hook);
}

fn working_thread() -> &'static OnceLock<fn(&'static str)> {
    static HOOK: OnceLock<fn(&'static str)> = OnceLock::new();
    &HOOK
}

/// Tells the installed hook, the first time this thread asks, that it does this module's work as
/// `name`.
fn note_working_thread(name: &'static str) {
    thread_local! {
        static NOTED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    if NOTED.with(|noted| noted.replace(true)) {
        return;
    }
    if let Some(hook) = working_thread().get() {
        hook(name);
    }
}

/// A copy of a colour target, or a fill, into guest memory that has not been carried out yet.
struct Deferred {
    /// The destination and length.
    destination: u64,
    length: u64,
    /// The guarded host pages covering it, and the protection they had.
    pages: (u64, u64, u32),
    /// What it writes when carried out.
    held: Held,
}

/// What a deferred write puts in memory when it is carried out.
enum Held {
    /// A colour target's frame.
    Frame {
        /// The frame as the copy saw it, on the device.
        snapshot: u64,
        target: ColourTarget,
        swap: ComponentSwap,
        /// The target's memory as the copy saw it: every word the frame does not cover - a
        /// partial edge block's padding - comes from here.
        memory_then: Contents,
    },
    /// One word throughout: a command-processor fill.
    Fill(u32),
}

impl Deferred {
    /// The device snapshot it keeps, if it keeps one.
    const fn snapshot(&self) -> Option<u64> {
        match self.held {
            Held::Frame { snapshot, .. } => Some(snapshot),
            Held::Fill(_) => None,
        }
    }

    /// Lets go of the device snapshot it keeps, unread.
    fn release_snapshot(&self, hooks: &LazyCopies) {
        if let Some(snapshot) = self.snapshot() {
            (hooks.discard)(snapshot);
        }
    }
}

/// Whether any guest memory is guarded for a deferred copy, so reading it would fault into the
/// copy's handler. `true` when the list cannot be read at once: the answer is asked from inside a
/// fault handler, where waiting could deadlock and guessing no could fault.
#[must_use]
pub fn guards_guest_memory() -> bool {
    deferred()
        .try_lock()
        .map_or(true, |copies| !copies.is_empty())
}

fn deferred() -> &'static Mutex<Vec<Deferred>> {
    static DEFERRED: Mutex<Vec<Deferred>> = Mutex::new(Vec::new());
    &DEFERRED
}

/// Page ranges recently carried out, so a thread that faulted on one while another thread was
/// carrying it out retries its access rather than reporting a fault.
fn resolved() -> &'static Mutex<Vec<(u64, u64)>> {
    static RESOLVED: Mutex<Vec<(u64, u64)>> = Mutex::new(Vec::new());
    &RESOLVED
}

/// How many resolved ranges are remembered.
const RESOLVED_KEPT: usize = 32;

/// The host page size page protection works in.
const HOST_PAGE: u64 = 4096;

/// The whole host pages covering `[address, address + length)`, as a base and a length.
const fn page_span(address: u64, length: u64) -> (u64, u64) {
    let start = address & !(HOST_PAGE - 1);
    let end = (address.saturating_add(length).saturating_add(HOST_PAGE - 1)) & !(HOST_PAGE - 1);
    (start, end - start)
}

const fn ranges_overlap(a: (u64, u64), b: (u64, u64)) -> bool {
    a.0 < b.0.saturating_add(b.1) && b.0 < a.0.saturating_add(a.1)
}

/// Carries out one deferred copy: its pages are made accessible again, then the target's memory as
/// the copy saw it, with the frame as the copy saw it tiled over, is written to the destination.
fn carry_out(copy: &Deferred, hooks: &LazyCopies) -> bool {
    crate::perf::span(crate::perf::Span::CarryOut, || carry_out_now(copy, hooks))
}

fn carry_out_now(copy: &Deferred, hooks: &LazyCopies) -> bool {
    let (base, len, protection) = copy.pages;
    if !(hooks.release)(base, len, protection) {
        copy.release_snapshot(hooks);
        return false;
    }
    if let Ok(mut resolved) = resolved().lock() {
        if resolved.len() >= RESOLVED_KEPT {
            resolved.remove(0);
        }
        resolved.push((base, len));
    }
    let (snapshot, target, swap, memory_then) = match &copy.held {
        Held::Fill(pattern) => return fill_guest(copy.destination, copy.length, *pattern),
        Held::Frame {
            snapshot,
            target,
            swap,
            memory_then,
        } => (*snapshot, *target, *swap, memory_then),
    };
    note_carried_out(target);
    let Some(frame) = crate::perf::span(crate::perf::Span::CarryOutTake, || {
        (hooks.take)(snapshot, spare_frame())
    }) else {
        tracing::warn!(
            "a deferred copy's frame could not be read - {:#x} keeps what it held",
            copy.destination
        );
        return false;
    };
    let laid = lay_out_frame(copy, (target, swap, memory_then), &frame);
    keep_spare_frame(frame);
    laid
}

/// Writes a carried-out copy's bytes where it lands: `frame` tiled over memory as the copy saw it.
fn lay_out_frame(
    copy: &Deferred,
    (target, swap, memory_then): (ColourTarget, ComponentSwap, &Contents),
    frame: &[u32],
) -> bool {
    let (width, height) = (target.width, target.height);
    let Ok(length) = usize::try_from(copy.length) else {
        return false;
    };
    // Laid out in place: memory as the copy saw it goes down first, then the frame is tiled over it
    // in the destination itself - rather than into a copy of memory that is then written.
    // A copy of the surface's first bytes alone (a GL context's readback) is tiled in place too
    // where the layout tiles into a prefix: through a whole surface's copy of memory, it was an
    // eight-megabyte allocation and a copy every frame (GLCB00001, 2 ms of a 12 ms frame).
    let whole = length / 4 >= target.words();
    if copy.destination % 4 == 0 && length % 4 == 0 && (whole || target.layout.tiles_prefixes()) {
        // Only where the frame leaves memory as it was: the words the tile writes every one of are
        // not laid down first only to be written over (GLCB00001: 8 MB a frame, 16% of its run).
        let skip = (target.layout.words_tiled_whole(width, height) * 4).min(length);
        let (at, rest) = (copy.destination + skip as u64, (length - skip) as u64);
        // A clear's word filled as words, not spelt out byte by byte first.
        let laid = crate::perf::span(crate::perf::Span::CarryOutWrite, || match memory_then {
            _ if rest == 0 => true,
            Contents::Uniform { word, .. } => fill_guest(at, rest, *word),
            Contents::Bytes(then) => then
                .get(skip..length)
                .is_some_and(|then| write_guest(at, then)),
        });
        if !laid {
            return false;
        }
        return crate::perf::span(crate::perf::Span::CarryOutTile, || {
            edit_guest_words(copy.destination, length / 4, |words| {
                let map = |w| stored(w, target, swap);
                if whole {
                    target
                        .layout
                        .tile_mapped(frame, width, height, target.pipe_bank_xor, words, map)
                        .is_ok()
                } else {
                    target
                        .layout
                        .tile_prefix_mapped(
                            frame,
                            (width, height),
                            target.pipe_bank_xor,
                            words,
                            map,
                        )
                        .is_some_and(|tiled| tiled.is_ok())
                }
            })
        });
    }
    let tiled = crate::perf::span(crate::perf::Span::CarryOutTile, || {
        let mut tiled = memory_then.words();
        target
            .layout
            .tile_mapped(
                frame,
                width,
                height,
                target.pipe_bank_xor,
                &mut tiled,
                |w| stored(w, target, swap),
            )
            .is_ok()
            .then_some(tiled)
    });
    let Some(tiled) = tiled else {
        return false;
    };
    let bytes = zerocopy::IntoBytes::as_bytes(tiled.as_slice());
    let Some(bytes) = usize::try_from(copy.length)
        .ok()
        .and_then(|length| bytes.get(..length))
    else {
        return false;
    };
    crate::perf::span(crate::perf::Span::CarryOutWrite, || {
        write_guest(copy.destination, bytes)
    })
}

/// Storage for the next carried-out frame's words, from the last one's: a frame is megabytes, and
/// a fresh allocation of that comes from the system a page at a time, every frame a GL context
/// reads back (GLCB00001).
fn spare_frame() -> Vec<u32> {
    spare_frame_slot()
        .lock()
        .ok()
        .and_then(|mut spare| spare.take())
        .unwrap_or_default()
}

/// Keeps a carried-out frame's storage for [`spare_frame`].
fn keep_spare_frame(frame: Vec<u32>) {
    if let Ok(mut spare) = spare_frame_slot().lock() {
        *spare = Some(frame);
    }
}

fn spare_frame_slot() -> &'static Mutex<Option<Vec<u32>>> {
    static SPARE: Mutex<Option<Vec<u32>>> = Mutex::new(None);
    &SPARE
}

/// Carries out every deferred copy whose destination pages touch `[address, address + length)` -
/// before anything else reads or writes there. `false` when one could not be.
fn carry_out_overlapping(address: u64, length: u64) -> bool {
    let Some(hooks) = lazy_copies().get() else {
        return true;
    };
    let Ok(mut list) = deferred().lock() else {
        return false;
    };
    let range = (address, length);
    // Asked before every read a submission's preparation makes, a word at a time for a draw's
    // descriptors: when nothing deferred is there the list is left as it is, not taken apart and
    // rebuilt.
    if !list
        .iter()
        .any(|copy| ranges_overlap((copy.pages.0, copy.pages.1), range))
    {
        return true;
    }
    let (touched, kept): (Vec<Deferred>, Vec<Deferred>) = std::mem::take(&mut *list)
        .into_iter()
        .partition(|copy| ranges_overlap((copy.pages.0, copy.pages.1), range));
    *list = kept;
    // Carried out with the list held, so a thread faulting on these pages meanwhile waits for them.
    carry_out_all(&touched, hooks)
}

/// Carries out every copy in `copies` - all of them, even after one fails, because a copy left
/// neither carried out nor deferred would be lost.
fn carry_out_all(copies: &[Deferred], hooks: &LazyCopies) -> bool {
    let mut ok = true;
    for copy in copies {
        ok &= carry_out(copy, hooks);
    }
    ok
}

/// Before host code reads `[address, address + length)` of guest memory directly: carries out every
/// deferred copy into it (D719), so the read sees what the hardware's memory holds. `false` when
/// one could not be.
pub fn carry_out_before_reading(address: u64, length: u64) -> bool {
    carry_out_overlapping(address, length)
}

/// An access to guest memory faulted: when `address` is in a deferred copy's destination, the copy
/// is carried out now and the access can be retried. The host's fault handler asks this for a
/// faulting read or write.
pub fn carry_out_at(address: u64) -> bool {
    let Some(hooks) = lazy_copies().get() else {
        return false;
    };
    let carried = {
        let Ok(mut list) = deferred().lock() else {
            return false;
        };
        let (touched, kept): (Vec<Deferred>, Vec<Deferred>) = std::mem::take(&mut *list)
            .into_iter()
            .partition(|copy| ranges_overlap((copy.pages.0, copy.pages.1), (address, 1)));
        *list = kept;
        // A touch of a pending frame under a deferred fill takes a band of its rows, the fill kept
        // deferred around them (D864).
        if let Some(pieces) = band_under_fills(&touched, address, hooks) {
            list.extend(pieces);
            return true;
        }
        (!touched.is_empty()).then(|| carry_out_all(&touched, hooks))
    };
    if let Some(carried) = carried {
        // A target under a deferred fill is guarded rather than protected, so a touch of it arrives
        // here and not at `written_at`. Its frame on the device goes into memory first all the
        // same: on hardware the draws are there by the time the guest looks.
        // A write lands on a band of the frame's rows, the rest kept on the device and read-only
        // (D864).
        if let Some(pending) =
            pending_target().filter(|(target, _)| overlaps_target(*target, address, 1))
            && !band_after_carrying_out(pending, address, hooks)
        {
            let mut memory = GuestCp {
                memory: MappedRegions::current(),
                submission: None,
            };
            let _ = write_back_pending(&mut memory);
        }
        return carried;
    }
    // Another thread carried it out while this one waited for the list.
    resolved()
        .lock()
        .is_ok_and(|resolved| resolved.iter().any(|r| ranges_overlap(*r, (address, 1))))
}

impl GuestCp<'_> {
    /// A copy of the whole pending target, deferred (D717): the hardware copies the target's memory
    /// with the frame drawn so far over it, and nothing observes the destination until something
    /// touches it. The frame is kept on the device, the destination's pages are guarded, and
    /// [`carry_out`] produces the bytes on first touch. `false` when this copy cannot wait; the
    /// caller then carries it out now.
    fn defer_copy(source: u64, destination: u64, count: usize) -> bool {
        // What the guest wrote into the frame's rows reaches the device before it is kept (D864).
        if !fold_band() {
            return false;
        }
        let Some(hooks) = lazy_copies().get() else {
            return false;
        };
        let Some((target, swap)) = pending_target() else {
            return false;
        };
        let span = target.words() as u64 * 4;
        let length = count as u64;
        // A level within a larger surface - a mip tail's block, or a padded level - shares its
        // words with what is not the frame, so a copy of them is more than the frame on the device.
        // A copy of the surface's first bytes is the frame laid out and cut short: a GL context
        // copies its tiled colour target by the size it would have linear, 0x7e9000 of 0x870000
        // bytes at 1920 by 1080, and each was a whole-frame write-back (TSHP00001).
        if target.place != crate::registers::Place::Whole
            || source != target.base
            || length > span
            || overlaps_target(target, destination, length)
        {
            return false;
        }
        // The target's memory as this copy sees it: what the draws started from, unchanged since -
        // any memory work that touched it would have written the frame back and left nothing
        // pending.
        let memory_then = match last_written().lock().ok().and_then(|last| last.clone()) {
            Some(written) if written.target == target && written.contents.len() as u64 == span => {
                written.contents
            }
            _ => return false,
        };
        let writable = write_lookup()
            .get()
            .is_some_and(|allows| allows(destination, length));
        if !writable {
            return false;
        }
        // Anything already deferred into these pages is carried out first, so the guard below is
        // the only one on them - unless it is a copy to exactly this destination, which this one
        // overwrites completely and which is dropped unread.
        let pages = page_span(destination, length);
        let Ok(mut list) = deferred().lock() else {
            return false;
        };
        // Guarded another way now: a protection kept would be saved as the one to restore (D720).
        unprotect_overlapping(pages.0, pages.1);
        let superseded = list
            .iter()
            .position(|copy| copy.destination == destination && copy.length == length);
        // The superseded copy's snapshot is released when this one's is kept, in one trip to the
        // device - or on its own, on a path that keeps none.
        let (guard, superseded_snapshot) = match superseded {
            Some(index) => {
                let old = list.remove(index);
                (Some(old.pages.2), old.snapshot())
            }
            None => (None, None),
        };
        let (touched, kept): (Vec<Deferred>, Vec<Deferred>) = std::mem::take(&mut *list)
            .into_iter()
            .partition(|copy| ranges_overlap((copy.pages.0, copy.pages.1), pages));
        *list = kept;
        let carried = !touched.is_empty();
        carry_out_all(&touched, hooks);
        // Carrying another copy out restored protection on the pages it shared with this one; a
        // guard kept from the superseded copy is put back, keeping the protection it saved.
        if carried && guard.is_some() && (hooks.guard)(pages.0, pages.1).is_none() {
            if let Some(old) = superseded_snapshot {
                (hooks.discard)(old);
            }
            return false;
        }
        let keep = || match superseded_snapshot {
            Some(old) => (hooks.replace)(old),
            None => (hooks.snapshot)(),
        };
        let Some(snapshot) = crate::perf::span(crate::perf::Span::KeepSnapshot, keep) else {
            if let Some(protection) = guard {
                (hooks.release)(pages.0, pages.1, protection);
            }
            return false;
        };
        let Some(protection) = guard.or_else(|| {
            crate::perf::span(crate::perf::Span::CopyGuard, || {
                (hooks.guard)(pages.0, pages.1)
            })
        }) else {
            (hooks.discard)(snapshot);
            return false;
        };
        list.push(Deferred {
            destination,
            length,
            pages: (pages.0, pages.1, protection),
            held: Held::Frame {
                snapshot,
                target,
                swap,
                memory_then,
            },
        });
        true
    }

    /// Memory work that touches a target whose frame is still on the device writes that frame back
    /// first (D714): deferring to the flip holds only while nothing else looks at the target, and a
    /// GL context copies its colour target into a readback buffer after each submission. `false`
    /// when the frame could not be written back.
    fn settle_pending(&mut self, address: u64, length: u64) -> bool {
        match pending_target() {
            Some((target, _)) if overlaps_target(target, address, length) => {
                crate::perf::span(crate::perf::Span::SettlePending, || {
                    write_back_pending(self)
                })
            }
            _ => true,
        }
    }

    /// Writes a dispatch's changed runs and fills into guest memory, in order, as
    /// [`cp::CpMemory::write`] and [`cp::CpMemory::fill`] would one at a time; nothing is written
    /// unless every one of them is writable. A dispatch's changes come back as runs of the bytes it
    /// changed, often thousands close together, so the writability lookup, the deferred copies, the
    /// pending frame and the protected target are each asked once for a stretch of neighbouring
    /// runs rather than once per run. Only the runs' own bytes are written: what lies between them
    /// may have been written by the guest since it was read. The words of `scattered` buffers follow
    /// the runs, each buffer asked about once for the span its words cover; a dispatch's buffers are
    /// written by its lanes in no order the hardware promises, so that order is as good as any.
    fn write_back_dispatch(
        &mut self,
        writes: &GuestWrites,
        scattered: &[Scattered<'_>],
        fills: &[cp::Fill],
    ) -> Result<(), String> {
        let writable = write_lookup().get();
        let allowed = |at: u64, bytes: u64| writable.is_some_and(|allows| allows(at, bytes));
        let stretches = stretches(writes);
        // A stretch not writable as a whole (a gap between its runs crossing out of a mapping) has
        // each run checked on its own.
        let whole: Vec<bool> = stretches
            .iter()
            .map(|&((first, end), _)| allowed(first, end - first))
            .collect();
        let each_run = || {
            stretches.iter().zip(&whole).all(|((_, runs), &whole)| {
                whole
                    || writes
                        .runs_in(runs.clone())
                        .all(|(at, bytes)| allowed(at, bytes.len() as u64))
            })
        };
        // A buffer's span not writable as a whole has each word checked on its own.
        let scattered_whole: Vec<Option<(u64, u64)>> = scattered
            .iter()
            .map(|buffer| {
                buffer
                    .span()
                    .filter(|&(first, end)| allowed(first, end - first))
            })
            .collect();
        let each_word = || {
            scattered
                .iter()
                .zip(&scattered_whole)
                .all(|(buffer, whole)| {
                    whole.is_some()
                        || buffer
                            .words()
                            .all(|(at, _, held)| held == 0 || allowed(at, held as u64))
                })
        };
        if !each_run()
            || !each_word()
            || !fills
                .iter()
                .all(|fill| allowed(fill.destination, fill.count))
        {
            return Err("it wrote guest memory that is not writable".to_owned());
        }
        for ((range, runs), whole) in stretches.into_iter().zip(whole) {
            if !self.write_stretch(range, (writes, runs), whole) {
                return Err("a write back into guest memory was refused".to_owned());
            }
        }
        for (buffer, whole) in scattered.iter().zip(scattered_whole) {
            if !self.write_scattered(buffer, whole) {
                return Err("a write back into guest memory was refused".to_owned());
            }
        }
        let _filling = crate::perf::SpanGuard::start(crate::perf::Span::WriteBackFills);
        for fill in fills {
            let count = usize::try_from(fill.count).map_err(|_| "a fill past this host's reach")?;
            if !cp::CpMemory::fill(self, fill.destination, fill.pattern, count) {
                return Err("a fill back into guest memory was refused".to_owned());
            }
        }
        Ok(())
    }

    /// Writes a scattered buffer's words, settling the span they cover once where `whole` - the span,
    /// writable as a whole - is given; otherwise each word through the checked path.
    fn write_scattered(&mut self, buffer: &Scattered<'_>, whole: Option<(u64, u64)>) -> bool {
        let Some((first, end)) = whole else {
            return buffer
                .words()
                .all(|(at, bytes, held)| held == 0 || write_guest(at, &bytes[..held]));
        };
        let length = end - first;
        if !carry_out_overlapping(first, length) || !self.settle_pending(first, length) {
            return false;
        }
        unprotect_overlapping(first, length);
        orbistoun_mem::watch::release_overlapping(first, length);
        for (at, bytes, held) in buffer.words() {
            let Ok(dest) = usize::try_from(at) else {
                return false;
            };
            let dest = std::ptr::with_exposed_provenance_mut::<u8>(dest);
            if held == 4 {
                // SAFETY: the installed lookup vouched that `[first, end)`, which holds this word's
                // four bytes, lies inside identity-mapped guest mappings whose protection allows
                // writes, and the pages guarded in it were given their protection back above, so
                // nothing here faults; the store is unaligned-safe.
                unsafe { std::ptr::write_unaligned(dest.cast::<[u8; 4]>(), bytes) };
                continue;
            }
            // SAFETY: as above, for the `held` bytes of the word the range holds, at most four,
            // all of them in the source.
            unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), dest, held) };
        }
        true
    }

    /// Writes `runs`, which lie within `[first, end)`, settling the stretch once; a stretch not
    /// `whole`ly writable has each run written through the checked path.
    fn write_stretch(
        &mut self,
        (first, end): (u64, u64),
        (writes, runs): (&GuestWrites, std::ops::Range<usize>),
        whole: bool,
    ) -> bool {
        let length = end - first;
        if !carry_out_overlapping(first, length) || !self.settle_pending(first, length) {
            return false;
        }
        if !whole {
            return writes
                .runs_in(runs)
                .all(|(at, bytes)| write_guest(at, bytes));
        }
        crate::perf::span(crate::perf::Span::WriteBackRelease, || {
            unprotect_overlapping(first, length);
            // A write-watch guard (D824) is released rather than faulted through.
            orbistoun_mem::watch::release_overlapping(first, length);
        });
        let _copying = crate::perf::SpanGuard::start(crate::perf::Span::WriteBackCopy);
        for (at, bytes) in writes.runs_in(runs) {
            let Ok(dest) = usize::try_from(at) else {
                return false;
            };
            // SAFETY: the installed lookup vouched that `[first, end)`, which holds this run, lies
            // inside identity-mapped guest mappings whose protection allows writes, and a protected
            // target's pages in it were given their protection back above, so nothing here faults.
            unsafe {
                std::ptr::copy_nonoverlapping(
                    bytes.as_ptr(),
                    std::ptr::with_exposed_provenance_mut::<u8>(dest),
                    bytes.len(),
                );
            }
        }
        true
    }
}

/// `writes` cut into stretches of consecutive, neighbouring runs, in order: each stretch's byte
/// range and the runs it holds.
fn stretches(writes: &GuestWrites) -> Vec<((u64, u64), std::ops::Range<usize>)> {
    let runs = &writes.runs;
    let mut stretches = Vec::new();
    let mut start = 0;
    while start < runs.len() {
        let first = runs[start].0;
        let mut end = first.saturating_add(runs[start].1.len() as u64);
        let mut stop = start + 1;
        while let Some((at, bytes)) = runs.get(stop) {
            let after = at.saturating_add(bytes.len() as u64);
            if *at < first
                || *at > end.saturating_add(RUN_GAP)
                || after.max(end) - first > RUN_STRETCH
            {
                break;
            }
            end = end.max(after);
            stop += 1;
        }
        stretches.push(((first, end), start..stop));
        start = stop;
    }
    stretches
}

/// The largest gap between two runs written back as one stretch.
const RUN_GAP: u64 = 4096;

/// The longest stretch written back as one.
const RUN_STRETCH: u64 = 16 << 20;

thread_local! {
    /// The releases the command processor reported during the walk in progress on this thread,
    /// handed to the display once the walk returns (D728).
    static RELEASED: std::cell::RefCell<Vec<(u64, u32)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Hands the display every release the walk just reported. After the walk rather than during it:
/// a flip presents, and presenting reads back through this module, which the walk is inside.
fn hand_releases_to_display() {
    let releases = RELEASED.with(|released| std::mem::take(&mut *released.borrow_mut()));
    for (address, context) in releases {
        crate::display::released(address, context);
    }
}

impl cp::CpMemory for GuestCp<'_> {
    fn released(&mut self, address: u64, context: u32) {
        RELEASED.with(|released| released.borrow_mut().push((address, context)));
    }

    fn run_draws(&mut self, segment: &cp::DrawSegment<'_>) -> bool {
        crate::perf::span(crate::perf::Span::RunDraws, || {
            let ran = self
                .streamout_segment(segment)
                .unwrap_or_else(|| self.draw_segment(segment));
            match ran {
                Ok(()) => true,
                Err(why) => {
                    note_draw_refusal(format!(
                        "the draws at bytes {:#x}..{:#x}: {why}",
                        segment.first, segment.end
                    ));
                    false
                }
            }
        })
    }

    fn with_streamout(&mut self, edit: &mut dyn FnMut(&mut crate::streamout::Offsets)) -> bool {
        edit(
            &mut crate::streamout::offsets()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        true
    }

    fn streamout_stats(&mut self, stream: usize) -> Option<(u64, u64)> {
        let stats = crate::streamout::stats()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Some((*stats.written.get(stream)?, *stats.needed.get(stream)?))
    }

    fn samples_passed(&mut self) -> Option<u64> {
        // The draws before it have been carried out (each segment runs at its first draw); the
        // device answers once they have run.
        sample_counter().get().and_then(|count| count())
    }

    fn run_dispatch(&mut self, dispatch: &cp::Dispatch<'_>) -> bool {
        match self.dispatch_into_memory(dispatch) {
            Ok(()) => true,
            Err(why) => {
                note_dispatch_refusal(format!(
                    "the compute dispatch at byte {:#x}: {why}",
                    dispatch.offset
                ));
                false
            }
        }
    }

    fn holds(&self, address: u64, expected: &[u8]) -> bool {
        carry_out_overlapping(address, expected.len() as u64)
            && self
                .memory
                .read(address, expected.len())
                .is_some_and(|bytes| bytes == expected)
    }

    fn read(&self, address: u64, length: usize) -> Option<Vec<u8>> {
        if !carry_out_overlapping(address, length as u64) {
            return None;
        }
        self.memory.read(address, length).map(<[u8]>::to_vec)
    }

    fn fill(&mut self, address: u64, pattern: u32, count: usize) -> bool {
        // A deferred copy this fill overwrites completely is dropped unread (D719), once the fill
        // is sure to happen, so a refused fill never loses the bytes it would have covered.
        if write_lookup()
            .get()
            .is_some_and(|allows| allows(address, count as u64))
        {
            drop_pending_covered(address, count as u64);
            if refill_guarded(address, pattern, count as u64) {
                return true;
            }
            drop_covered(address, count as u64);
        }
        if crate::perf::span(crate::perf::Span::FillDeferred, || {
            defer_fill(address, pattern, count as u64)
        }) {
            return true;
        }
        if !carry_out_overlapping(address, count as u64)
            || !self.settle_pending(address, count as u64)
        {
            return false;
        }
        crate::perf::span(crate::perf::Span::FillWritten, || {
            fill_guest(address, count as u64, pattern)
        })
    }

    fn edit_words(&mut self, address: u64, count: usize, edit: &mut dyn FnMut(&mut [u32])) -> bool {
        let length = count as u64 * 4;
        if !carry_out_overlapping(address, length) || !self.settle_pending(address, length) {
            return false;
        }
        let writable = write_lookup()
            .get()
            .is_some_and(|allows| allows(address, length));
        let Ok(at) = usize::try_from(address) else {
            return false;
        };
        // Words in place need word alignment; a colour target is block-aligned, so this refuses
        // only an address no surface has.
        if !writable || at % 4 != 0 {
            return false;
        }
        unprotect_overlapping(address, length);
        // SAFETY: a protected target's pages here were given their protection back above, and the
        // installed lookup vouched that `[address, address + 4 * count)` lies inside
        // identity-mapped guest mappings whose protection allows writes: `count` writable,
        // word-aligned `u32`s of this process (host and guest are both little-endian). The owning
        // guest thread is blocked in this submit while `edit` runs.
        let words = unsafe {
            std::slice::from_raw_parts_mut(std::ptr::with_exposed_provenance_mut::<u32>(at), count)
        };
        edit(words);
        true
    }

    fn copy(&mut self, source: u64, destination: u64, count: usize) -> bool {
        // A deferred copy reads nothing of the source's memory - the frame is on the device, and
        // what memory held under it is remembered - so what is deferred into the source (a fill the
        // frame was drawn over) stays deferred. The destination is left to `defer_copy`, which drops
        // a deferred copy this one overwrites completely.
        if Self::defer_copy(source, destination, count) {
            return true;
        }
        // A source wholly under a deferred fill, with no frame drawn over it, holds the fill's word
        // throughout: copying it is filling the destination with that word, and reads nothing.
        if destination % 4 == 0
            && !pending_target()
                .is_some_and(|(target, _)| overlaps_target(target, source, count as u64))
            && let Some(pattern) = deferred_fill_over(source, count as u64)
        {
            return self.fill(destination, pattern, count);
        }
        // Read now, so what is deferred into it is carried out first.
        if !carry_out_overlapping(source, count as u64)
            || !self.settle_pending(source, count as u64)
        {
            return false;
        }
        let writable = write_lookup()
            .get()
            .is_some_and(|allows| allows(destination, count as u64));
        let Ok(dest) = usize::try_from(destination) else {
            return false;
        };
        if !writable || self.memory.read(source, count).is_none() {
            return false;
        }
        // The copy is sure to happen now, so what is deferred or pending wholly inside its
        // destination is dropped unread as a fill drops it (D719): carrying a frame out only for
        // this copy to overwrite every byte of it was a whole-frame tiling per frame.
        drop_covered(destination, count as u64);
        drop_pending_covered(destination, count as u64);
        if !carry_out_overlapping(destination, count as u64)
            || !self.settle_pending(destination, count as u64)
        {
            return false;
        }
        let Some(from) = self.memory.read(source, count) else {
            return false;
        };
        unprotect_overlapping(destination, count as u64);
        // SAFETY: `from` is `count` readable guest bytes and the installed lookup vouched that the
        // destination's `count` bytes are writable identity-mapped guest memory (a protected
        // target's pages given their protection back above). A DMA's two ranges may overlap, so
        // this is a `memmove`; the guest thread that owns both is blocked in this submit for the
        // copy.
        unsafe {
            std::ptr::copy(
                from.as_ptr(),
                std::ptr::with_exposed_provenance_mut::<u8>(dest),
                count,
            );
        }
        true
    }

    fn write(&mut self, address: u64, bytes: &[u8]) -> bool {
        // The write-back itself clears the pending target before it writes, so this never recurses.
        if !carry_out_overlapping(address, bytes.len() as u64)
            || !self.settle_pending(address, bytes.len() as u64)
        {
            return false;
        }
        write_guest(address, bytes)
    }

    /// The GPU clock counter: nanoseconds since the first stamp, plus one so it is never zero. The
    /// hardware's counter rate is unmeasured; guests check only that a non-zero stamp came back.
    fn timestamp(&mut self) -> u64 {
        static START: OnceLock<std::time::Instant> = OnceLock::new();
        let start = START.get_or_init(std::time::Instant::now);
        u64::try_from(start.elapsed().as_nanos())
            .unwrap_or(u64::MAX - 1)
            .saturating_add(1)
    }
}

/// What the command processor did across the run: submissions executed, how many ran to completion,
/// and the last one's detail - read by the run report.
#[derive(Debug, Clone, Copy, Default)]
pub struct ExecutionRecord {
    /// Submissions whose command-processor work was attempted.
    pub submissions: u64,
    /// Of those, the ones that ran to the end with nothing needing the GPU.
    pub completed: u64,
    /// Of those, the ones whose draws were carried out and written back.
    pub drawn: u64,
    /// Bytes written to guest memory across them all.
    pub bytes_written: u64,
    /// Submissions not carried out because one before them on the queue stopped (D710).
    pub held_back: u64,
    /// The most recent submission's execution - once the queue has stopped, the one that stopped
    /// it, since that is where its work ends.
    pub last: Option<cp::CpExecution>,
}

/// The execution of the first submission on the queue that did not run to completion, once one
/// has not.
///
/// The command processor retires its queue in order, and nothing here ever runs the packet a
/// submission stopped at, so everything submitted after it waits forever: none of it is carried
/// out, and no fence after it is written (D705, D710).
fn queue_stall() -> &'static Mutex<Option<cp::CpExecution>> {
    static STALL: Mutex<Option<cp::CpExecution>> = Mutex::new(None);
    &STALL
}

/// How many held writes are kept, newest last: a waiting thread polls the fence of the work it
/// submitted most recently, and each kept write costs the wait sub-loop a watch.
const HELD_MOST: usize = 64;

/// The memory writes the stopped queue never made (D832).
fn held() -> &'static Mutex<Vec<cp::HeldWrite>> {
    static HELD: Mutex<Vec<cp::HeldWrite>> = Mutex::new(Vec::new());
    &HELD
}

/// Keeps the writes of `bytes` from byte `from` on as held, dropping the oldest past
/// [`HELD_MOST`].
fn note_held(bytes: &[u8], from: u32) {
    let mut held = held()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    held.extend(cp::held_writes(bytes, from));
    let excess = held.len().saturating_sub(HELD_MOST);
    held.drain(..excess);
}

/// The memory writes the queue holds: those from where it stopped on, and those of every
/// submission held back behind it (D710), newest last. A thread waiting on the GPU polls one of
/// these; the wait sub-loop watches them (D832).
#[must_use]
pub fn held_writes() -> Vec<cp::HeldWrite> {
    held()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

fn execution_record() -> &'static Mutex<ExecutionRecord> {
    static RECORD: OnceLock<Mutex<ExecutionRecord>> = OnceLock::new();
    RECORD.get_or_init(|| Mutex::new(ExecutionRecord::default()))
}

/// The run's command-processor execution so far.
#[must_use]
pub fn execution() -> ExecutionRecord {
    execution_record()
        .lock()
        .map(|record| *record)
        .unwrap_or_default()
}

/// Guest memory served from the regions the guest was given.
///
/// A read whose whole range lies inside a region is answered from identity-mapped host memory; one
/// outside every region is `None`, so a shader GPU address that does not resolve is reported
/// rather than dereferenced (D130).
struct MappedRegions {
    regions: Vec<(u64, u64)>,
}

impl MappedRegions {
    /// The regions in force for this submit, copied so the lock is not held across a walk.
    fn current() -> Self {
        Self {
            regions: guest_regions()
                .lock()
                .map(|r| r.clone())
                .unwrap_or_default(),
        }
    }

    /// Whether `[address, address + length)` lies wholly inside one region - one set at entry, or
    /// one the installed live lookup vouches for.
    fn contains(&self, address: u64, length: usize) -> bool {
        let Some(end) = address.checked_add(length as u64) else {
            return false;
        };
        self.regions
            .iter()
            .any(|&(start, region_end)| address >= start && end <= region_end)
            || region_lookup()
                .get()
                .is_some_and(|readable| readable(address, length as u64))
    }
}

/// Guest memory as a submission's preparation reads it - its shaders, textures and buffers - with
/// every earlier write settled first: a frame still pending on the device for a target the read
/// overlaps is written back as a flip writes it (D714), and a copy deferred over the read is
/// carried out (D719). The
/// hardware ran the earlier submission before this one read, so a texture drawn into by one is
/// what the next samples. Only the reads that overlap pay for it; the many submissions a frame
/// draws into one target never read that target while preparing. Asynchronously, a submission
/// still queued that draws over the read, or could write back the frame pending over it, is
/// carried out first (D874).
struct SettledReads<'m>(&'m MappedRegions);

impl GuestMemory for SettledReads<'_> {
    fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
        let span = length as u64;
        command_processor::settle_over(address, span);
        // Settled as a flip settles it - deferred onto the target's own pages where it can be
        // (D719) - and then carried out by this read, with any other copy over it.
        if pending_target().is_some_and(|(target, _)| overlaps_target(target, address, span)) {
            let _ = write_back_at_this_flip();
        }
        carry_out_overlapping(address, span);
        self.0.read(address, length)
    }
}

impl GuestMemory for MappedRegions {
    fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
        if length == 0 || !self.contains(address, length) {
            return None;
        }
        let ptr = std::ptr::with_exposed_provenance::<u8>(usize::try_from(address).ok()?);
        // SAFETY: `[address, address + length)` lies wholly inside a region the guest was given
        // (checked above), so it is `length` bytes of readable identity-mapped guest memory, which
        // outlives this borrow because the guest's mappings live for the process.
        Some(unsafe { std::slice::from_raw_parts(ptr, length) })
    }
}

/// The last submission a guest handed to `sceAgcDriverSubmitDcb`, held whole so the worker can
/// read its report and drive its commands to a backend (D695).
fn last_submission() -> &'static Mutex<Option<Submission>> {
    static LAST: OnceLock<Mutex<Option<Submission>>> = OnceLock::new();
    LAST.get_or_init(|| Mutex::new(None))
}

/// The report of the most recent DCB a guest submitted, or `None` if none has.
///
/// Read by the run report (`orbistoun-worker`), beside the reach and import counts.
#[must_use]
pub fn last_submission_report() -> Option<SubmissionReport> {
    last_submission()
        .lock()
        .ok()
        .and_then(|slot| slot.as_ref().map(|s| s.report.clone()))
}

/// The whole of the most recent submission - its commands and modules, not only its report - so the
/// worker can drive it to a backend. `None` until a guest submits one.
#[must_use]
pub fn take_last_submission() -> Option<Submission> {
    last_submission()
        .lock()
        .ok()
        .and_then(|mut slot| slot.take())
}

/// Reads the 16-byte submit descriptor (`{gpu_addr: u64, size_dwords: u32, flags: u8, pad}`, as
/// obSCEne measures it) into an address and a byte length, refusing a null or absurd one.
fn submit_descriptor(descriptor: u64) -> Option<(u64, usize)> {
    if descriptor == 0 {
        return None;
    }
    // SAFETY: `descriptor` is the guest's own identity-mapped 16-byte submit descriptor, filled and
    // passed by pointer; sixteen bytes are read, the width the driver reads.
    let desc = unsafe {
        std::slice::from_raw_parts(
            std::ptr::with_exposed_provenance::<u8>(usize::try_from(descriptor).ok()?),
            16,
        )
    };
    let gpu_addr = u64::from_le_bytes(desc[0..8].try_into().ok()?);
    let size_dwords = u32::from_le_bytes(desc[8..12].try_into().ok()?);
    if gpu_addr == 0 || size_dwords == 0 || size_dwords > MAX_DCB_DWORDS {
        return None;
    }
    Some((gpu_addr, size_dwords as usize * 4))
}

/// `sceAgcDriverSubmitDcb(dcb)` - the handover. A guest builds a command buffer with `libSceAgc`
/// and hands it over here; this reads the descriptor, walks the command buffer through the
/// translator, and records a `SubmissionReport`.
///
/// It never fails on the guest's account: an empty or out-of-bounds descriptor records nothing and
/// returns success, as the hardware does (`rc-submit 0x0`). The command buffer and the shader
/// addresses it names are served from the guest's regions, so a descriptor outside every region
/// is refused rather than dereferenced, and an unresolved shader address is counted (D130).
fn submit_dcb(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    submit_described(args[0])
}

/// `sceAgcDriverSubmitCommandBuffer(queue, dcb)` - the same handover with the queue named.
///
/// The open-toolchain SDK tries this first and falls back to [`submit_dcb`], passing the handle
/// [`create_queue`] wrote and the same 16-byte descriptor (`gl_context.c`, `agc_draw.c` in
/// oops-sdk). The queue carries nothing a submit needs, so the descriptor in argument one takes the
/// path `SubmitDcb`'s argument zero does and answers the same success code; that this entry point
/// answers that code is assumed, not measured.
fn submit_command_buffer(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    submit_described(args[1])
}

/// The pipeline a running guest's submissions are prepared through, kept for the run so its shader
/// cache survives between them.
fn live_pipeline() -> &'static Mutex<Option<Pipeline>> {
    static LIVE: OnceLock<Mutex<Option<Pipeline>>> = OnceLock::new();
    LIVE.get_or_init(|| Mutex::new(None))
}

/// Where the title's kept translations are written, once a run has named it (D113).
static TRANSLATIONS_AT: OnceLock<std::path::PathBuf> = OnceLock::new();

/// The kept translations read before the guest started, until the live pipeline takes them.
static TRANSLATIONS: Mutex<Option<crate::translations::TranslationStore>> = Mutex::new(None);

/// Reads the title's kept translations from `path` and makes every one this translator's, before
/// the guest starts, so a draw finds its module already made (D113). `build` is the build line; the
/// translator is that and a digest of its sources, so an edited tree retranslates. Returns what the
/// refill did and how many translations are kept.
pub fn prepare_translations(
    path: std::path::PathBuf,
    build: &str,
) -> (crate::translations::Refill, usize) {
    let translator = crate::translations::translator(build);
    let mut store = crate::translations::TranslationStore::load(&path);
    let tables = orbistoun_shader::EncodingTable::builtin()
        .and_then(|e| orbistoun_shader::OperandTable::builtin().map(|o| (e, o)));
    let refill = match tables {
        Ok((encodings, operands)) => store.refill(&translator, (&encodings, &operands)),
        Err(e) => {
            tracing::warn!("the kept translations were not refilled: {e}");
            crate::translations::Refill::default()
        }
    };
    if let Err(e) = store.save_if_changed(&path) {
        tracing::warn!(
            "the kept translations could not be written to {}: {e}",
            path.display()
        );
    }
    let kept = store.len();
    let _ = TRANSLATIONS_AT.set(path);
    if let Ok(mut slot) = TRANSLATIONS.lock() {
        *slot = Some(store);
    }
    (refill, kept)
}

/// Writes any translation the pipeline made since the last write.
fn keep_new_translations(pipeline: &mut Pipeline) {
    let (Some(path), Some(store)) = (TRANSLATIONS_AT.get(), pipeline.translation_store_mut())
    else {
        return;
    };
    if let Err(e) = store.save_if_changed(path) {
        tracing::warn!(
            "the kept translations could not be written to {}: {e}",
            path.display()
        );
    }
}

/// Every module the live pipeline made that no drawn submission has yet carried to the backend.
///
/// A module travels with the preparation that made it, but the pipeline is shared: asynchronously
/// the guest prepares submissions ahead of the one carried out (D821), and a segment re-prepared
/// then on the command processor's thread finds a module one of those later submissions made
/// already translated, and binds it while its words wait in the queue behind it. So what a
/// submission binds is looked up here as it is carried out, not where it was prepared, and a
/// module whose draws were refused stays here for the next submission that binds it.
fn undelivered_modules() -> &'static Mutex<std::collections::BTreeMap<crate::ResourceId, Vec<u32>>>
{
    static PENDING: OnceLock<Mutex<std::collections::BTreeMap<crate::ResourceId, Vec<u32>>>> =
        OnceLock::new();
    PENDING.get_or_init(Default::default)
}

/// Keeps the modules a preparation made until a drawn submission carries them. Called with the
/// live pipeline still held, so no other preparation can bind one before it is kept.
fn note_made(submission: &Submission) {
    if submission.modules.is_empty() {
        return;
    }
    undelivered_modules()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .extend(submission.modules.iter().map(|(id, m)| (*id, m.clone())));
}

/// `submission`, carrying every module it binds that the backend may not have yet; itself,
/// uncopied, when it carries them all already.
fn with_bound_modules(submission: &Submission) -> std::borrow::Cow<'_, Submission> {
    let missing: Vec<(crate::ResourceId, Vec<u32>)> = {
        let pending = undelivered_modules()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        submission
            .commands
            .iter()
            .filter_map(|command| match command {
                crate::RenderCommand::BindShader { shader, .. }
                    if !submission.modules.contains_key(shader) =>
                {
                    pending.get(shader).map(|module| (*shader, module.clone()))
                }
                _ => None,
            })
            .collect()
    };
    if missing.is_empty() {
        return std::borrow::Cow::Borrowed(submission);
    }
    let mut carrying = submission.clone();
    carrying.modules.extend(missing);
    std::borrow::Cow::Owned(carrying)
}

/// Every module a drawn submission has carried to the backend, kept so a submission can be driven
/// again to a backend of its own, which holds none of them (a draw's change, measured at the run's
/// end).
fn delivered_modules() -> &'static Mutex<std::collections::BTreeMap<crate::ResourceId, Vec<u32>>> {
    static DELIVERED: OnceLock<Mutex<std::collections::BTreeMap<crate::ResourceId, Vec<u32>>>> =
        OnceLock::new();
    DELIVERED.get_or_init(Default::default)
}

/// Moves the modules a drawn submission carried out of the undelivered store: the backend holds
/// them now, and their words are kept among the delivered.
fn note_delivered(submission: &Submission) {
    if submission.modules.is_empty() {
        return;
    }
    let mut pending = undelivered_modules()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut delivered = delivered_modules()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for (id, words) in &submission.modules {
        pending.remove(id);
        delivered.entry(*id).or_insert_with(|| words.clone());
    }
}

/// `submission`, carrying every module it binds that the run made, delivered or not: what a
/// backend that holds none of them needs to drive it.
#[must_use]
pub fn with_every_bound_module(submission: &Submission) -> Submission {
    let mut whole = with_bound_modules(submission).into_owned();
    let delivered = delivered_modules()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for command in &submission.commands {
        if let crate::RenderCommand::BindShader { shader, .. } = command
            && !whole.modules.contains_key(shader)
            && let Some(words) = delivered.get(shader)
        {
            whole.modules.insert(*shader, words.clone());
        }
    }
    whole
}

/// Reads one submit descriptor, walks the command buffer it names, and keeps the submission - the
/// body both submit entry points share.
fn submit_described(descriptor: u64) -> u64 {
    // On a title's own stack the walk and the translator's frames may not fit (D841).
    orbistoun_kernel::thread::with_room(SUBMIT_STACK_ROOM, || {
        crate::perf::span(crate::perf::Span::SubmitCall, || {
            crate::perf::measure(crate::perf::Phase::Submit, || {
                submit_described_timed(descriptor)
            })
        })
    })
}

/// The stack a submission's preparation is given (D841): it decodes and translates shaders and
/// walks the command buffer, and PPSA21564's render thread, on a stack of its own, overflowed in it.
const SUBMIT_STACK_ROOM: u64 = 4 * 1024 * 1024;

/// What a submission left in the registers is what the next one starts from (D737), its indirect
/// loads read from the tables as it was submitted with them.
fn carry_registers(bytes: &[u8], submission: &Submission) {
    if let Some(pipeline) = live_pipeline()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_mut()
    {
        pipeline.carry(
            bytes,
            Some(&submission.tables.over(&MappedRegions::current())),
        );
    }
}

/// [`submit_described`]'s body, timed whole as [`crate::perf::Phase::Submit`].
fn submit_described_timed(descriptor: u64) -> u64 {
    note_working_thread("submitting");
    let Some((gpu_addr, length)) = submit_descriptor(descriptor) else {
        return SUBMIT_OK;
    };
    let memory = MappedRegions::current();
    // A descriptor whose command buffer lies outside every region is refused rather than
    // dereferenced, and recorded as an empty report so the run report shows the submit happened.
    let Some(bytes) = memory.read(gpu_addr, length).map(<[u8]>::to_vec) else {
        if let Ok(mut slot) = last_submission().lock() {
            *slot = Some(Submission::default());
        }
        return SUBMIT_OK;
    };
    // Asynchronously, a preparation that read what a queued submission writes is made again once
    // the queue has drained, the live pipeline free meanwhile for the command processor (D876);
    // one that read only what a queued submission draws is drawn again as it is carried out (D882).
    let ((mut submission, writes), read_ahead) =
        crate::perf::measure(crate::perf::Phase::Prepare, || {
            command_processor::preparing(|| prepare_live(&bytes, &memory))
        });
    submission.read_ahead_of_draws = read_ahead;
    // The next submission is prepared from the registers this one leaves, so they are carried here,
    // in submission order, before it is carried out anywhere.
    carry_registers(&bytes, &submission);
    // Prepared here, in order, on the guest's thread; carried out where the run says (D821).
    if asynchronous() {
        command_processor::queue(bytes, submission, writes);
    } else {
        run_submission(&bytes, submission);
    }
    SUBMIT_OK
}

/// Prepares `bytes` through the live pipeline, made at the first submission, and answers what
/// carrying it out writes once it is queued ([`queued_writes`]); nothing, run synchronously.
fn prepare_live(bytes: &[u8], memory: &MappedRegions) -> (Submission, QueuedWrites) {
    // One pipeline for the run, not one per submission: the pipeline caches translated shaders.
    let mut live = live_pipeline()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if live.is_none() {
        *live = Pipeline::new(Strategy::Predicated {
            fidelity: Fidelity::Lane,
            width: Width::default(),
        })
        .ok()
        // A live guest's shaders name their own buffers, so the window is placed from them
        // (D711).
        .map(|pipeline| {
            pipeline
                .placing_window_from_shaders()
                .feeding_user_data()
                .carrying_register_state()
        })
        .map(
            |pipeline| match TRANSLATIONS.lock().ok().and_then(|mut slot| slot.take()) {
                Some(store) => pipeline.with_translation_store(store),
                None => pipeline,
            },
        );
    }
    live.as_mut().map_or_else(
        || (Submission::default(), Some(Vec::new())),
        |pipeline| {
            let reads = SettledReads(memory);
            let submission = pipeline.submit(bytes, Queue::Draw, &[], &reads);
            keep_new_translations(pipeline);
            note_made(&submission);
            // What the submission carries forward - the tables its registers load from, the
            // bindings its queued writes are worked out from - is read after what is queued
            // ahead of it: a read of it over a queued write makes the preparation again (D882).
            let writes = command_processor::carrying(|| {
                for (address, length) in submission.tables.spans() {
                    command_processor::settle_over(address, length);
                }
                if asynchronous() {
                    crate::perf::span(crate::perf::Span::QueuedWrites, || {
                        queued_writes(pipeline, bytes, &submission, &reads)
                    })
                } else {
                    Some(Vec::new())
                }
            });
            (submission, writes)
        },
    )
}

/// What carrying out a queued submission writes besides the colour targets it draws into, as
/// (first byte, bytes) of guest memory; `None` where that is not bounded before it runs, and every
/// preparation read waits for it (D876).
type QueuedWrites = Option<Vec<(u64, u64)>>;

/// [`QueuedWrites`] for `submission`: its command processor's own packets' writes
/// ([`cp::written_spans`]) and each dispatch's, from its bindings
/// ([`Pipeline::dispatch_write_spans`]). Not bounded when a dispatch's state or bindings are not
/// known before it runs, or a draw streams out.
fn queued_writes(
    pipeline: &Pipeline,
    bytes: &[u8],
    submission: &Submission,
    reads: &SettledReads<'_>,
) -> QueuedWrites {
    if submission.streams_out {
        return None;
    }
    let mut spans = cp::written_spans(bytes);
    for state in &submission.dispatch_states {
        spans.extend(pipeline.dispatch_write_spans(state.as_ref()?, reads)?);
    }
    Some(spans)
}

/// Whether submissions are carried out on a thread of their own, the guest running on (D821).
static ASYNCHRONOUS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Carries out submissions on the command processor's own thread from now on, so a guest runs on
/// while its work executes and waits on its fences as it would on hardware (D821). Off by default:
/// two runs then agree on when each fence was written, which a verdict between them needs.
pub fn set_asynchronous(on: bool) {
    ASYNCHRONOUS.store(on, std::sync::atomic::Ordering::Relaxed);
}

fn asynchronous() -> bool {
    ASYNCHRONOUS.load(std::sync::atomic::Ordering::Relaxed)
}

/// Waits, up to `limit`, for every queued submission to have been carried out, and answers whether
/// they all were. Immediate when submissions run synchronously.
pub fn settle(limit: std::time::Duration) -> bool {
    command_processor::settle(limit)
}

/// The command processor's thread, and the submissions queued for it (D821).
mod command_processor {
    use super::{
        ColourTarget, QueuedWrites, Submission, overlaps_target, pending_target, run_submission,
    };
    use std::cell::Cell;
    use std::collections::VecDeque;
    use std::sync::mpsc::{SyncSender, sync_channel};
    use std::sync::{Condvar, Mutex, OnceLock};

    /// Submissions queued ahead of the one executing. A guest that gets this far ahead waits at
    /// its next submit, as a full ring stops a guest on hardware; the wait is timed as its own
    /// span.
    const DEPTH: usize = 4;

    /// A queued submission as a preparation read waits on it: the colour targets it draws into
    /// ([`Submission::drawn_targets`], D874) and what else carrying it out writes (D876).
    struct Queued {
        /// Its place in the order submissions were queued in.
        order: u64,
        targets: Vec<ColourTarget>,
        writes: QueuedWrites,
    }

    impl Queued {
        /// Whether it draws into any of `[address, address + length)`.
        fn draws_over(&self, address: u64, length: u64) -> bool {
            self.targets
                .iter()
                .any(|target| overlaps_target(*target, address, length))
        }

        /// Whether carrying it out writes any of `[address, address + length)` other than by
        /// drawing into it.
        fn writes_otherwise_over(&self, address: u64, length: u64) -> bool {
            let end = address.saturating_add(length);
            self.writes.as_ref().is_none_or(|spans| {
                spans
                    .iter()
                    .any(|&(at, bytes)| at < end && address < at.saturating_add(bytes))
            })
        }
    }

    /// Submissions queued and not yet carried out, oldest first, each as what it writes, and the
    /// signal that one was carried out.
    static IN_FLIGHT: (Mutex<VecDeque<Queued>>, Condvar) =
        (Mutex::new(VecDeque::new()), Condvar::new());

    /// The [`Queued::order`] the next queued submission takes.
    static NEXT_ORDER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    /// How many times [`preparing`] makes a preparation before it keeps the last however it read:
    /// another guest thread can queue between a drain and the next preparation.
    const PREPARATIONS: usize = 3;

    /// How long a preparation read waits for the queue: a submission is never held by the guest,
    /// so this bounds a fault, not a wait.
    const SETTLE_LIMIT: std::time::Duration = std::time::Duration::from_secs(10);

    std::thread_local! {
        /// Whether this thread is the command processor's: what it reads, it reads after every
        /// submission ahead of the one it is carrying out, so it never waits on the queue.
        static ON_PROCESSOR: Cell<bool> = const { Cell::new(false) };
        /// Whether this thread is inside [`preparing`], which holds the live pipeline: a read over
        /// what a queued submission writes is noted in `CROSSED`, as the latest [`Queued::order`]
        /// it waits for, instead of waited on.
        static NOTING: Cell<bool> = const { Cell::new(false) };
        static CROSSED: Cell<Option<u64>> = const { Cell::new(None) };
        /// Whether a read inside [`preparing`] was over what a queued submission draws or writes,
        /// which the command processor reads again as it carries the submission out (D882).
        static AHEAD: Cell<bool> = const { Cell::new(false) };
        /// Whether this thread is reading what a preparation carries forward ([`carrying`]): a
        /// read over a queued write is noted in `CROSSED`, so the preparation is made again.
        static CARRYING: Cell<bool> = const { Cell::new(false) };
    }

    /// A prepared submission and its command stream, boxed so a refused send hands back a pointer.
    type Work = Box<(Vec<u8>, Submission)>;

    fn sender() -> Option<&'static Mutex<SyncSender<Work>>> {
        static SENDER: OnceLock<Option<Mutex<SyncSender<Work>>>> = OnceLock::new();
        SENDER
            .get_or_init(|| {
                let (send, receive) = sync_channel::<Work>(DEPTH);
                std::thread::Builder::new()
                    .name("orbistoun-command-processor".to_owned())
                    .spawn(move || {
                        ON_PROCESSOR.set(true);
                        super::note_working_thread("command processor");
                        for work in receive {
                            let (bytes, submission) = *work;
                            run_submission(&bytes, submission);
                            let (queued, fell) = &IN_FLIGHT;
                            queued
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .pop_front();
                            fell.notify_all();
                        }
                    })
                    .ok()
                    .map(|_| Mutex::new(send))
            })
            .as_ref()
    }

    /// Queues a prepared submission; carries it out here when no thread could be started.
    pub(super) fn queue(bytes: Vec<u8>, submission: Submission, writes: QueuedWrites) {
        let Some(sender) = sender() else {
            run_submission(&bytes, submission);
            return;
        };
        IN_FLIGHT
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push_back(Queued {
                order: NEXT_ORDER.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                targets: submission.drawn_targets.clone(),
                writes,
            });
        let sent = crate::perf::span(crate::perf::Span::QueueFull, || {
            sender
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .send(Box::new((bytes, submission)))
        });
        if let Err(std::sync::mpsc::SendError(work)) = sent {
            IN_FLIGHT
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .pop_back();
            let (bytes, submission) = *work;
            run_submission(&bytes, submission);
        }
    }

    pub(super) fn settle(limit: std::time::Duration) -> bool {
        let (queued, fell) = &IN_FLIGHT;
        let held = queued
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        fell.wait_timeout_while(held, limit, |queued| !queued.is_empty())
            .is_ok_and(|(left, _)| left.is_empty())
    }

    /// Waits, up to `limit`, for every submission queued up to [`Queued::order`] `last` to have
    /// been carried out; the ones queued after it are not waited for.
    fn settle_through(last: u64, limit: std::time::Duration) {
        let (queued, fell) = &IN_FLIGHT;
        let held = queued
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = fell.wait_timeout_while(held, limit, |queued| {
            queued.front().is_some_and(|oldest| oldest.order <= last)
        });
    }

    /// Runs `prepare`, a preparation holding the live pipeline, with every read over what a queued
    /// submission writes noted rather than waited on; when one was, the queue drains as far as the
    /// latest submission read over, with the pipeline free, and `prepare` runs again (D876). The
    /// command processor carries a dispatch or a run of draws out through that pipeline, so a read
    /// waiting while it was held waited out [`SETTLE_LIMIT`] and then read what it would have read
    /// without the wait. Only a read of what the preparation carries forward ([`carrying`]) is made
    /// again so: any other read over a queued write, or over the frame pending, feeds only the
    /// submission's own draws, and is answered beside the preparation kept - the command processor
    /// prepares those draws again as it carries the submission out, after the writes (D882).
    pub(super) fn preparing<T>(mut prepare: impl FnMut() -> T) -> (T, bool) {
        let was = NOTING.replace(true);
        CROSSED.set(None);
        AHEAD.set(false);
        let mut prepared = prepare();
        let mut made = 1;
        while let Some(last) = CROSSED.get()
            && made < PREPARATIONS
        {
            made += 1;
            prepared = crate::perf::span(crate::perf::Span::PrepareAgain, || {
                // A wait on the command processor, whose work is counted where it runs: kept out of
                // the preparation's own time.
                crate::perf::span(crate::perf::Span::SettleOver, || {
                    settle_through(last, SETTLE_LIMIT);
                });
                CROSSED.set(None);
                AHEAD.set(false);
                prepare()
            });
        }
        NOTING.set(was);
        CROSSED.set(None);
        (prepared, AHEAD.replace(false))
    }

    /// Runs `read`, reading what a preparation carries forward past the submission itself - the
    /// tables its registers load from, the bindings of what it writes: inside [`preparing`], a read
    /// of it over a queued write makes the preparation again rather than answering it read ahead.
    pub(super) fn carrying<T>(read: impl FnOnce() -> T) -> T {
        let was = CARRYING.replace(true);
        let read = read();
        CARRYING.set(was);
        read
    }

    /// Carries out the queued submissions up to the latest that writes over `[address, address +
    /// length)` before a preparation reads it - one that draws over it (D874), or writes it
    /// through its own packets or a dispatch (D876) - or all of them, when the frame pending over
    /// it could be written back in the middle of the read. The hardware ran them before this
    /// submission read; preparing it here, ahead of them, read the memory from before they wrote
    /// it. Submissions queued after the latest one are not waited for, and a read overlapping
    /// nothing queued keeps the guest running on while the command processor works. Inside
    /// [`preparing`] the read is only noted: of what the preparation carries forward, it is made
    /// again once they have run; otherwise its draws are prepared again as it is carried out.
    pub(super) fn settle_over(address: u64, length: u64) {
        if ON_PROCESSOR.get() {
            return;
        }
        let over_pending =
            pending_target().is_some_and(|(target, _)| overlaps_target(target, address, length));
        // The latest queued submission that draws over the read - every one, over the pending
        // frame, since any of them can write it back - and the latest that writes over it
        // otherwise.
        let (drawn, written) = {
            let queued = IN_FLIGHT
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let latest = |over: &dyn Fn(&Queued) -> bool| {
                queued
                    .iter()
                    .rev()
                    .find(|submission| over(submission))
                    .map(|submission| submission.order)
            };
            let drawn = if over_pending {
                queued.back().map(|submission| submission.order)
            } else {
                latest(&|submission| submission.draws_over(address, length))
            };
            (
                drawn,
                latest(&|submission| submission.writes_otherwise_over(address, length)),
            )
        };
        let Some(last) = drawn.max(written) else {
            return;
        };
        if NOTING.get() {
            if CARRYING.get() {
                CROSSED.set(Some(
                    CROSSED.get().map_or(last, |crossed| crossed.max(last)),
                ));
            } else {
                AHEAD.set(true);
            }
        } else {
            crate::perf::span(crate::perf::Span::SettleOver, || {
                settle_through(last, SETTLE_LIMIT);
            });
        }
    }
}

/// Carries out one prepared submission: the command processor's memory work, its draws, its
/// flips' release, and the record of what ran.
fn run_submission(bytes: &[u8], submission: Submission) {
    let memory = MappedRegions::current();
    // A queue an earlier submission stopped retires nothing more (D710).
    let stalled = *queue_stall()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if stalled.is_some() {
        note_held(bytes, 0);
        if let Ok(mut record) = execution_record().lock() {
            record.held_back += 1;
        }
        if let Ok(mut slot) = last_submission().lock() {
            *slot = Some(submission);
        }
        return;
    }
    // The command processor's own memory work, carried out as the submit returns or, asynchronously,
    // on its own thread. It stops at the first packet needing the GPU, so a fence is written only
    // when everything before it ran (D705); draws are such work once an executor is installed and
    // they can run as one into a target they can be written back to.
    let executed = crate::perf::span(crate::perf::Span::CommandProcessor, || {
        cp::execute(
            bytes,
            &mut GuestCp {
                memory,
                submission: Some(&submission),
            },
        )
    });
    hand_releases_to_display();
    if let Ok(mut record) = execution_record().lock() {
        record.submissions += 1;
        record.completed += u64::from(executed.stopped == cp::Stopped::Completed);
        record.drawn += u64::from(executed.draws > 0);
        record.bytes_written += executed.bytes_written;
        record.last = Some(executed);
    }
    if let Some(from) = executed.stopped.offset() {
        note_held(bytes, from);
    }
    if executed.stopped != cp::Stopped::Completed {
        *queue_stall()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(executed);
    }
    if let Ok(mut slot) = last_submission().lock() {
        *slot = Some(submission);
    }
}

/// Implementations this crate provides for `libSceAgcDriver`.
///
/// `sceAgcDriverCreateQueue` accepts the queue; the resource-registration family returns the
/// hardware's measured `0x8a6c9018`.
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("sceAgcDriverGetDefaultOwner", resource_registration_query),
        (
            "sceAgcDriverGetResourceRegistrationMaxNameLength",
            resource_registration_query,
        ),
        ("sceAgcDriverCreateQueue", create_queue),
        ("sceAgcDriverDestroyQueue", destroy_queue),
        ("sceAgcDriverRegisterOwner", register_owner),
        ("sceAgcDriverRegisterResource", register_resource),
        ("sceAgcDriverSetHsOffchipParam", set_hs_offchip_param),
        ("sceAgcDriverSetTFRing", set_tf_ring),
        ("sceAgcDriverAddEqEvent", add_eq_event),
        ("sceAgcDriverGetEqContextId", get_eq_context_id),
        ("sceAgcDriverGetEqEventType", get_eq_event_type),
        (
            "sceAgcDriverInitResourceRegistration",
            init_resource_registration,
        ),
        (
            "sceAgcDriverQueryResourceRegistrationUserMemoryRequirements",
            query_resource_registration_user_memory_requirements,
        ),
        ("sceAgcDriverSubmitCommandBuffer", submit_command_buffer),
        ("sceAgcDriverSubmitDcb", submit_dcb),
    ]
}

#[cfg(test)]
mod tests {
    use super::{
        GUEST_ARG_REGISTERS, SUBMIT_OK, create_queue, last_submission_report, set_guest_regions,
        submit_command_buffer, submit_dcb,
    };

    /// A frame drawn into more than one colour attachment is kept by each of them: a texture a
    /// later frame samples that was drawn as a second attachment is as drawn as the first.
    #[test]
    fn a_frame_is_kept_by_every_attachment_it_drew_into() {
        let (first, second) = (0x7fed_0000_0000_u64, 0x7fed_0001_0000_u64);
        let mut submission = crate::pipeline::Submission::default();
        submission.report.draws = 2;
        super::note_drawn(&[first, second], &submission);
        let kept = super::last_drawn_into(second).expect("the second attachment");
        assert_eq!(kept.draws, 2);
        assert_eq!(
            super::last_drawn_into(first).map(|by| by.frame),
            Some(kept.frame)
        );
    }

    /// The frame last drawn into a target is kept by the target: its draws and each texture they
    /// bound, once however many draws bound it, its texels held rather than copied. A later frame
    /// into another target leaves it as it was. Addresses far from any a guest run uses, as other
    /// tests share the store.
    #[test]
    fn the_frame_last_drawn_into_a_target_is_kept() {
        use crate::backend::{RenderCommand, TextureEncoding};
        let texels: std::sync::Arc<[u32]> = std::sync::Arc::from(vec![1_u32, 2, 2, 3]);
        let bind = RenderCommand::BindTexture {
            slot: 1,
            texels: texels.clone(),
            hash: 7,
            width: 2,
            height: 2,
            sampling: crate::registers::TextureSampling::CLAMPED_POINT,
            encoding: TextureEncoding::Rgba8,
            faces: 1,
            levels: 1,
        };
        let (target, other) = (0x7fee_0000_0000_u64, 0x7fef_0000_0000_u64);
        super::note_drawn_by(&[target], 3, &[bind.clone(), bind], &[(0x5000, 2, 2)]);
        let first = super::last_drawn_into(target).expect("kept");
        assert_eq!(first.draws, 3);
        assert!(first.read.is_empty(), "no draw buffer bound");
        assert_eq!(
            first.sampled,
            vec![(0x5000, 2, 2)],
            "where what it sampled lies"
        );
        assert_eq!(first.textures.len(), 1, "bound by two draws, kept once");
        assert_eq!((first.textures[0].0, first.textures[0].1), (1, 2));
        assert!(std::sync::Arc::ptr_eq(&first.textures[0].3, &texels));
        super::note_drawn_by(&[other], 1, &[], &[]);
        assert_eq!(super::last_drawn_into(target), Some(first.clone()));
        assert!(super::last_drawn_into(other).is_some_and(|by| by.frame > first.frame));
        assert_eq!(super::last_drawn_into(0x7ff0_0000_0000), None);
    }
    use crate::dispatch::{BufferAfter, BufferBefore};
    use std::sync::{Mutex, PoisonError};

    /// A module another preparation made is carried by the first submission that binds it and is
    /// carried out, even though that submission was prepared without it, and once drawn it is not
    /// carried again. Ids far from any the live pipeline hands out, as other tests share the store.
    #[test]
    fn a_bound_module_made_elsewhere_travels_until_it_is_drawn() {
        use crate::{RenderCommand, ResourceId, ShaderStage};
        let made_ahead = crate::pipeline::Submission {
            modules: [(ResourceId(0xdead_0010), vec![0x0723_0203, 1])].into(),
            ..Default::default()
        };
        super::note_made(&made_ahead);
        let binding = crate::pipeline::Submission {
            commands: vec![
                RenderCommand::BindShader {
                    stage: ShaderStage::Fragment,
                    shader: ResourceId(0xdead_0010),
                },
                RenderCommand::BindShader {
                    stage: ShaderStage::Vertex,
                    shader: ResourceId(0xdead_0011),
                },
            ],
            ..Default::default()
        };
        let carried = super::with_bound_modules(&binding);
        assert_eq!(
            carried.modules.keys().copied().collect::<Vec<_>>(),
            [ResourceId(0xdead_0010)],
            "the module bound and not yet drawn travels; one never made is left to the backend"
        );
        super::note_delivered(&carried);
        assert!(
            matches!(
                super::with_bound_modules(&binding),
                std::borrow::Cow::Borrowed(_)
            ),
            "a drawn module is the backend's, and is not carried again"
        );
        // Asked for afresh - a submission driven again to a backend of its own - a drawn module's
        // words are still there to carry.
        let whole = super::with_every_bound_module(&binding);
        assert_eq!(
            whole.modules.get(&ResourceId(0xdead_0010)),
            Some(&vec![0x0723_0203, 1])
        );
    }

    /// The runs found a chunk at a time are the runs an element-by-element comparison finds: at
    /// chunk edges, across them, at either end, and with nothing or everything changed.
    #[test]
    fn changed_runs_match_an_element_by_element_comparison() {
        let naive = |before: &[u32], after: &[u32]| {
            let mut runs = Vec::new();
            let mut start = None;
            for (index, (old, new)) in before.iter().zip(after).enumerate() {
                match (old != new, start) {
                    (true, None) => start = Some(index),
                    (false, Some(first)) => {
                        runs.push((first, index - first));
                        start = None;
                    }
                    _ => {}
                }
            }
            if let Some(first) = start {
                runs.push((first, before.len().min(after.len()) - first));
            }
            runs
        };
        let chunk = super::UNCHANGED_CHUNK;
        for len in [0_usize, 1, chunk - 1, chunk, chunk + 1, 3 * chunk + 5] {
            let before: Vec<u32> = (0..len as u32).collect();
            let changes: [Vec<usize>; 6] = [
                vec![],
                vec![0, chunk - 1, chunk, chunk + 1],
                vec![len.saturating_sub(1)],
                vec![1, 2, 2 * chunk + 3],
                (0..len).collect(),
                // A run crossing whole chunks changed throughout, ending inside a later one.
                (5..2 * chunk + 7).collect(),
            ];
            for changed in &changes {
                let mut after = before.clone();
                for &at in changed.iter().filter(|&&at| at < len) {
                    after[at] ^= 0xFFFF;
                }
                assert_eq!(
                    super::changed_runs(&before, &after),
                    naive(&before, &after),
                    "{len} words, {changed:?} changed"
                );
                // A shorter after compares only as far as both reach.
                let short = &after[..len.saturating_sub(1)];
                assert_eq!(
                    super::changed_runs(&before, short),
                    naive(&before, short),
                    "{len} words, one short"
                );
            }
        }
    }

    /// Neighbouring runs written back share a stretch; a wide gap, a run behind the stretch's start
    /// or one past its longest length starts another; and the runs keep their order.
    #[test]
    fn neighbouring_runs_share_a_stretch_in_order() {
        let run = |at: u64, len: usize| (at, vec![0u8; len]);
        let writes = [
            run(0x1000, 4),
            run(0x1008, 4),
            run(0x2000, 8),
            run(0x9000, 4),
            run(0x8000, 4),
            run(0x8000 + super::RUN_STRETCH, 4),
        ];
        let mut listed = super::GuestWrites::default();
        for (at, bytes) in &writes {
            listed.push(*at, bytes);
        }
        assert_eq!(
            super::stretches(&listed),
            [
                ((0x1000, 0x2008), 0..3),
                ((0x9000, 0x9004), 3..4),
                ((0x8000, 0x8004), 4..5),
                (
                    (0x8000 + super::RUN_STRETCH, 0x8004 + super::RUN_STRETCH),
                    5..6
                ),
            ]
        );
    }

    /// A submission whose colour target zero is `COLOR_INVALID` draws depth alone and is not a
    /// target a frame is written back to; an `8_8_8_8` one draws colour (D779). PPSA02664's depth
    /// passes leave `CB_COLOR0_INFO` at `0x0001_8000`.
    #[test]
    fn an_unbound_colour_target_draws_depth_alone() {
        let with = |info| crate::pipeline::Submission {
            colour_target_format: Some(crate::registers::decode_colour_target_format(info)),
            ..crate::pipeline::Submission::default()
        };
        assert!(super::draws_depth_only(&with(0x0001_8000)));
        assert!(!super::draws_depth_only(&with(0x0001_80a8)));
        assert!(!super::draws_depth_only(
            &crate::pipeline::Submission::default()
        ));
        assert!(super::writable_target(&with(0x0001_8000)).is_err());
    }

    /// A dispatch's buffer it changed and left holding one word throughout is written back as a
    /// fill of its whole range (D804), as PPSA28061's per-frame clear of its display buffer is;
    /// one with two words in it writes its changed runs, and one it left as it was writes nothing.
    #[test]
    fn a_buffer_cleared_by_a_dispatch_is_written_back_as_a_fill() {
        let ranges = [(0x1000, 10), (0x2000, 8), (0x3000, 8)];
        let before = [vec![7, 0, 7], vec![1, 1], vec![5, 5]].map(BufferBefore::Words);
        let after = [vec![0, 0, 0], vec![1, 2], vec![5, 5]].map(|words| BufferAfter {
            words,
            written: Vec::new(),
            listed: None,
            whole: None,
        });
        let (writes, fills) = super::buffer_writes(&ranges, &before, &after).expect("same shapes");
        assert_eq!(
            fills,
            [crate::cp::Fill {
                destination: 0x1000,
                pattern: 0,
                count: 10,
            }]
        );
        assert_eq!(writes.pairs(), [(0x2004, 2u32.to_le_bytes().to_vec())]);
    }

    /// A buffer the program only stores to was not read (D809): the words its write mask marks are
    /// written back, and nothing else - the device's zeros in the words it did not store stay off
    /// guest memory. Stored whole with one word, it is a fill.
    #[test]
    fn an_unread_buffer_writes_back_the_words_it_stored() {
        let ranges = [(0x1000, 16), (0x2000, 8)];
        let before = [BufferBefore::Unread(4), BufferBefore::Unread(2)];
        let after = [
            BufferAfter {
                words: vec![9, 0, 8, 7],
                written: vec![1, 0, 1, 1],
                listed: None,
                whole: None,
            },
            BufferAfter {
                words: vec![0, 0],
                written: vec![1, 1],
                listed: None,
                whole: None,
            },
        ];
        let (writes, fills) = super::buffer_writes(&ranges, &before, &after).expect("same shapes");
        // The same stores as the device's lists of them (indices, then values), with no words or
        // mask: the same writes and the same fill.
        let listed =
            [(vec![0, 2, 3], vec![9, 8, 7]), (vec![0, 1], vec![0, 0])].map(|list| BufferAfter {
                listed: Some(list),
                whole: None,
                ..BufferAfter::default()
            });
        assert_eq!(
            super::buffer_writes(&ranges, &before, &listed),
            Ok((writes.clone(), fills.clone())),
            "listed as masked"
        );
        assert_eq!(
            super::uniform_buffers(&ranges, (&before, &listed)),
            fills,
            "the list stored whole is the fill"
        );
        let past = [BufferAfter {
            listed: Some((vec![4], vec![1])),
            whole: None,
            ..BufferAfter::default()
        }];
        assert!(
            super::buffer_writes(&ranges[..1], &before[..1], &past).is_err(),
            "a word listed past the buffer is refused"
        );
        assert_eq!(
            writes.pairs(),
            [
                (0x1000, 9u32.to_le_bytes().to_vec()),
                (0x1008, [8u32.to_le_bytes(), 7u32.to_le_bytes()].concat()),
            ]
        );
        assert_eq!(
            fills,
            [crate::cp::Fill {
                destination: 0x2000,
                pattern: 0,
                count: 8,
            }]
        );
        assert_eq!(
            super::uniform_buffers(&ranges, (&before, &after)),
            fills,
            "only the buffer stored whole is a fill"
        );
    }

    /// A buffer not read whose every word the device found stored with one value - a clear - comes
    /// back as that value alone, and is written back as a fill of its whole range, and counted one
    /// (D750), with no words to compare.
    #[test]
    fn a_clear_the_device_found_is_a_fill() {
        let ranges = [(0x1000, 4096)];
        let before = [BufferBefore::Unread(1024)];
        let after = [BufferAfter {
            whole: Some(0xFFFC_00F0),
            ..BufferAfter::default()
        }];
        let fill = crate::cp::Fill {
            destination: 0x1000,
            pattern: 0xFFFC_00F0,
            count: 4096,
        };
        assert_eq!(
            super::buffer_writes(&ranges, &before, &after),
            Ok((super::GuestWrites::default(), vec![fill]))
        );
        assert_eq!(super::uniform_buffers(&ranges, (&before, &after)), [fill]);
    }

    /// A dispatch's buffer left holding one word throughout is a fill of it, whatever it held
    /// before; one with two words in it is not (D750).
    #[test]
    fn a_buffer_left_uniform_is_a_fill() {
        let ranges = [(0x1000, 8), (0x2000, 8), (0x3000, 0)];
        let before = [vec![0, 0], vec![0, 0], vec![]].map(BufferBefore::Words);
        let after = [vec![0xFFFC_00F0, 0xFFFC_00F0], vec![1, 2], vec![]].map(|words| BufferAfter {
            words,
            written: Vec::new(),
            listed: None,
            whole: None,
        });
        assert_eq!(
            super::uniform_buffers(&ranges, (&before, &after)),
            [crate::cp::Fill {
                destination: 0x1000,
                pattern: 0xFFFC_00F0,
                count: 8,
            }]
        );
    }

    /// A segment's stream keeps every packet up to its last draw, register writes included, turns
    /// each earlier draw into a `NOP` of the same length, and leaves out everything after (D729).
    #[test]
    fn a_segment_stream_keeps_register_state_and_silences_earlier_draws() {
        use crate::packet::build::{command_header, measured};
        let set = [command_header(measured::SET_CONTEXT_REG, 2), 0x318, 7];
        let draw = [command_header(measured::DRAW_INDEX_AUTO, 2), 3, 2];
        let fill = [
            command_header(measured::DMA_DATA, 6),
            0x8000_0000 | (2 << 29) | (3 << 20),
            1,
            0,
            0x2000,
            0,
            16,
        ];
        let mut words = set.to_vec();
        words.extend(draw);
        words.extend(fill);
        let first = u32::try_from(words.len() * 4).expect("small");
        words.extend(set);
        words.extend(draw);
        let end = u32::try_from(words.len() * 4).expect("small");
        words.extend(fill);
        let stream: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();

        let segment = super::segment_stream(&crate::cp::DrawSegment {
            stream: &stream,
            first,
            end,
            whole: false,
        });

        let mut expected = stream[..end as usize].to_vec();
        expected[12..16].copy_from_slice(&command_header(measured::NOP, 2).to_le_bytes());
        assert_eq!(segment, expected);
        let walked = crate::packet::walk(&segment);
        assert!(walked.is_trustworthy());
        assert_eq!(walked.packets.len(), 5, "every packet kept, none split");
    }

    /// A dispatch before a segment is memory work the command processor already carried out
    /// (D729), so the segment's stream silences it as it does an earlier draw: drawn again it
    /// would be a second dispatch, and on a device with no compute shader bound, a refusal.
    #[test]
    fn a_segment_stream_silences_the_dispatches_before_it() {
        use crate::packet::build::{command_header, dispatch_direct, measured};
        let set = [command_header(measured::SET_CONTEXT_REG, 2), 0x318, 7];
        let draw = [command_header(measured::DRAW_INDEX_AUTO, 2), 3, 2];
        let mut words = set.to_vec();
        let dispatch_at = words.len() * 4;
        words.extend(dispatch_direct(0xc0, 1, 1));
        let first = u32::try_from(words.len() * 4).expect("small");
        words.extend(draw);
        let end = u32::try_from(words.len() * 4).expect("small");
        let stream: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();

        let segment = super::segment_stream(&crate::cp::DrawSegment {
            stream: &stream,
            first,
            end,
            whole: false,
        });

        let mut expected = stream.clone();
        expected[dispatch_at..dispatch_at + 4]
            .copy_from_slice(&command_header(measured::NOP, 4).to_le_bytes());
        assert_eq!(segment, expected);
    }

    /// A dispatch writes back exactly the words it changed, as runs.
    #[test]
    fn a_dispatch_writes_back_the_runs_it_changed() {
        let before = [0, 1, 2, 3, 4, 5, 6];
        assert_eq!(super::changed_runs(&before, &before), vec![]);
        assert_eq!(
            super::changed_runs(&before, &[9, 1, 9, 9, 4, 5, 9]),
            vec![(0, 1), (2, 2), (6, 1)]
        );
    }

    /// A write into a linear 1920x1080 frame is handed a band of whole rows that starts and ends on
    /// a page: 7680-byte rows meet a 4 KiB page every 8 rows, so a write in row 0 or row 5 gets rows
    /// 0 to 31, fifteen pages to the 8 rows, and the last band stops at the frame's foot (D864). A
    /// tiled frame, or one whose rows are not each a run of memory, gets none.
    #[test]
    fn a_write_into_a_linear_frame_is_handed_a_band_of_whole_rows() {
        use super::{ColourTarget, band_at};
        let target = ColourTarget {
            base: 0x40_0000,
            width: 1920,
            height: 1080,
            pipe_bank_xor: 0,
            layout: crate::tiling::SurfaceLayout::Linear,
            place: crate::registers::Place::Whole,
        };
        let row = 1920 * 4;
        let top = Some(((0, 32), (0x40_0000, 32 * row)));
        assert_eq!(band_at(target, 0x40_0000), top);
        assert_eq!(band_at(target, 0x40_0000 + 5 * row + 100), top);
        assert_eq!(
            band_at(target, 0x40_0000 + 40 * row),
            Some(((40, 32), (0x40_0000 + 40 * row, 32 * row)))
        );
        // The foot: row 1079's group starts at 1072, and the frame ends 8 rows on.
        assert_eq!(
            band_at(target, 0x40_0000 + 1079 * row),
            Some(((1072, 8), (0x40_0000 + 1072 * row, 8 * row)))
        );
        assert_eq!(
            band_at(target, 0x40_0000 + 1080 * row),
            None,
            "past the frame"
        );
        assert_eq!(band_at(target, 0x3f_ffff), None, "before it");
        // A 64KB_R_X frame's band is a row of fifteen blocks, 128 rows; the last is 56.
        let tiled = ColourTarget {
            layout: crate::tiling::SurfaceLayout::Rx64Kb,
            ..target
        };
        let block_row = 15 * 64 * 1024;
        assert_eq!(
            band_at(tiled, 0x40_0000),
            Some(((0, 128), (0x40_0000, block_row)))
        );
        assert_eq!(
            band_at(tiled, 0x40_0000 + 8 * block_row + 5),
            Some(((1024, 56), (0x40_0000 + 8 * block_row, block_row)))
        );
        let other = ColourTarget {
            layout: crate::tiling::SurfaceLayout::Dx4Kb,
            ..target
        };
        assert_eq!(band_at(other, 0x40_0000), None);
        let unaligned = ColourTarget {
            base: 0x40_0100,
            ..target
        };
        assert_eq!(band_at(unaligned, 0x40_0100), None);
    }

    /// A range touches a target exactly when it shares a byte with its tiled surface: a 16x8 target
    /// is one 64 KiB block, so the byte before it and the byte after its block are outside, and a
    /// copy starting inside or spanning it is not.
    #[test]
    fn a_range_overlaps_a_target_when_it_shares_a_byte_with_its_surface() {
        use super::{ColourTarget, overlaps_target};
        let target = ColourTarget {
            base: 0x10_0000,
            width: 16,
            height: 8,
            pipe_bank_xor: 0,
            layout: crate::tiling::SurfaceLayout::Rx64Kb,
            place: crate::registers::Place::Whole,
        };
        assert!(
            !overlaps_target(target, 0x0f_ff00, 0x100),
            "ends at the base"
        );
        assert!(
            overlaps_target(target, 0x0f_ff00, 0x101),
            "reaches the first byte"
        );
        assert!(
            overlaps_target(target, 0x10_ffff, 1),
            "the block's last byte"
        );
        assert!(
            !overlaps_target(target, 0x11_0000, 4),
            "just past the block"
        );
        assert!(overlaps_target(target, 0, u64::MAX), "spans it");
    }

    /// A frame goes into a `SWAP_ALT` target as B, G, R, A and comes back out as it went in:
    /// `Rgba8` `0xff190000` lands with its blue in the first byte of memory, and a standard-order
    /// target takes it unchanged.
    #[test]
    fn a_swap_alt_target_stores_blue_first_and_round_trips() {
        use super::{ComponentSwap, swapped};
        let blue = 0xff19_0000;
        assert_eq!(swapped(blue, ComponentSwap::Alternate), 0xff00_0019);
        assert_eq!(
            swapped(
                swapped(0x4433_2211, ComponentSwap::Alternate),
                ComponentSwap::Alternate
            ),
            0x4433_2211
        );
        assert_eq!(swapped(blue, ComponentSwap::Standard), blue);
    }

    /// One 64 KiB block of guest memory at `BASE`; everything else unmapped.
    struct Block(Vec<u8>);

    const BASE: u64 = 0x10_0000;

    impl crate::cp::CpMemory for Block {
        fn read(&self, address: u64, length: usize) -> Option<Vec<u8>> {
            let start = usize::try_from(address.checked_sub(BASE)?).ok()?;
            self.0.get(start..start + length).map(<[u8]>::to_vec)
        }
        fn write(&mut self, address: u64, bytes: &[u8]) -> bool {
            let Some(start) = address
                .checked_sub(BASE)
                .and_then(|s| usize::try_from(s).ok())
            else {
                return false;
            };
            match self.0.get_mut(start..start + bytes.len()) {
                Some(dest) => {
                    dest.copy_from_slice(bytes);
                    true
                }
                None => false,
            }
        }
        fn timestamp(&mut self) -> u64 {
            1
        }
    }

    /// The frame the draws make lands in the guest's target, tiled and in its byte order, over what
    /// the target held: the draw is handed the target's own contents, and every texel it answers is
    /// found at its `64KB_R_X` address, blue first for a `SWAP_ALT` target.
    #[test]
    fn a_drawn_frame_is_written_back_tiled_over_the_target_s_own_contents() {
        use super::{ColourTarget, ComponentSwap, draw_over};
        use crate::tiling::tiled_byte_offset_64kb_rx_bpp4_surface as at;
        let _guard = serial();
        let (width, height) = (16u32, 8u32);
        let mut memory = Block(vec![0; 65536]);
        // The guest's clear, in its own B, G, R, A order: opaque blue.
        for y in 0..height {
            for x in 0..width {
                let o = at(x, y, width);
                memory.0[o..o + 4].copy_from_slice(&[0xff, 0, 0, 0xff]);
            }
        }
        let target = ColourTarget {
            base: BASE,
            width,
            height,
            pipe_bank_xor: 0,
            layout: crate::tiling::SurfaceLayout::Rx64Kb,
            place: crate::registers::Place::Whole,
        };
        let mut seen = None;
        let wrote = draw_over(&mut memory, target, ComponentSwap::Alternate, |before| {
            seen = before.to_words(128).and_then(|b| b.first().copied());
            // Every texel gets its own red value; texel (3, 5) is the one checked.
            Some(
                (0..width * height)
                    .map(|i| 0xff00_0000 | (i & 0xff))
                    .collect(),
            )
        });
        assert!(wrote);
        assert_eq!(
            seen,
            Some(0xffff_0000),
            "the draw starts from the guest's blue, as Rgba8"
        );
        let o = at(3, 5, width);
        let red = u8::try_from(5 * width + 3).expect("small");
        assert_eq!(
            &memory.0[o..o + 4],
            &[0, 0, red, 0xff],
            "red third in B, G, R, A"
        );

        // The next submission into the same, untouched target is told so: the drawer already holds
        // that frame, so it is handed nothing to start from...
        let mut handed = Some(Vec::new());
        draw_over(&mut memory, target, ComponentSwap::Alternate, |before| {
            handed = before.to_words(128);
            Some(vec![0xff00_0000; (width * height) as usize])
        });
        assert_eq!(handed, None, "unchanged since it was written");
        // ...and once the guest writes the target itself, it is handed the target again. Where the
        // target is write-protected (D720) that write faults and the handler asks `written_at`,
        // asked here.
        memory.0[0] ^= 0xff;
        super::written_at(BASE);
        draw_over(&mut memory, target, ComponentSwap::Alternate, |before| {
            handed = before.to_words(128);
            None
        });
        assert!(handed.is_some(), "changed by the guest, so read again");
    }

    /// A one-byte target is handed to the drawer as red with alpha one, and only each texel's red
    /// comes back: every other byte of its block - padding past the extent - stays as it was.
    #[test]
    fn a_one_byte_target_draws_through_red_alone() {
        use super::{ColourTarget, ComponentSwap, draw_over};
        use crate::tiling::tiled_byte_offset_64kb_rx_bpp1_xor as at;
        let _guard = serial();
        super::forget_written();
        let (width, height, xor) = (16u32, 8u32, 0x2u8);
        let mut memory = Block(vec![0x5a; 65536]);
        for y in 0..height {
            for x in 0..width {
                memory.0[at(x, y, width, xor)] = u8::try_from(y * width + x).expect("small");
            }
        }
        let target = ColourTarget {
            base: BASE,
            width,
            height,
            pipe_bank_xor: xor,
            layout: crate::tiling::SurfaceLayout::Rx64KbBpp1,
            place: crate::registers::Place::Whole,
        };
        let mut handed = None;
        let wrote = draw_over(&mut memory, target, ComponentSwap::Standard, |before| {
            handed = before.to_words(128);
            // A drawer that wrote green, blue and alpha too: none of it reaches memory.
            handed.as_ref().map(|words| {
                words
                    .iter()
                    .map(|w| 0x1234_5600 | ((w + 1) & 0xFF))
                    .collect()
            })
        });
        assert!(wrote);
        let handed = handed.expect("read");
        assert_eq!(handed[0], 0xff00_0000, "red 0, alpha one");
        assert_eq!(handed[5 * 16 + 3], 0xff00_0000 | (5 * 16 + 3));
        assert_eq!(
            u32::from(memory.0[at(3, 5, width, xor)]),
            5 * 16 + 3 + 1,
            "red written back as the texel's byte"
        );
        let texels: Vec<usize> = (0..height)
            .flat_map(|y| (0..width).map(move |x| at(x, y, width, xor)))
            .collect();
        assert!(
            memory
                .0
                .iter()
                .enumerate()
                .filter(|(i, _)| !texels.contains(i))
                .all(|(_, &b)| b == 0x5a),
            "no byte outside the texels moved"
        );
    }

    /// A one-byte target under a fill whose bytes differ is not uniform: each texel is the byte
    /// the layout puts there.
    #[test]
    fn a_one_byte_target_under_a_mixed_fill_reads_texel_by_texel() {
        use super::{ColourTarget, ComponentSwap, TargetRead};
        use crate::tiling::tiled_byte_offset_64kb_rx_bpp1_xor as at;
        let target = ColourTarget {
            base: BASE,
            width: 16,
            height: 8,
            pipe_bank_xor: 0,
            layout: crate::tiling::SurfaceLayout::Rx64KbBpp1,
            place: crate::registers::Place::Whole,
        };
        let TargetRead::Changed(texels) =
            TargetRead::of_pattern(0x4433_2211, target, ComponentSwap::Standard)
        else {
            panic!("a mixed fill is not uniform");
        };
        for (x, y) in [(0, 0), (1, 0), (3, 5), (15, 7)] {
            let byte = [0x11, 0x22, 0x33, 0x44][at(x, y, 16, 0) % 4];
            assert_eq!(
                texels[(y * 16 + x) as usize],
                0xff00_0000 | byte,
                "({x},{y})"
            );
        }
        assert!(matches!(
            TargetRead::of_pattern(0x7777_7777, target, ComponentSwap::Standard),
            TargetRead::Uniform(0xff00_0077)
        ));
    }

    /// A stored image read through cleared keys writes back its whole chain as zeros first, then
    /// its level's non-zero texels over them, then its keys as uncompressed: the chain's other
    /// levels read the zeros their cleared keys meant, not what memory held under them.
    #[test]
    fn a_cleared_chain_expands_whole_before_its_level_is_written() {
        use crate::pipeline::{DispatchImages, DispatchSurface, KeysToExpand};
        let level = super::ColourTarget {
            base: BASE + 0x2_0000,
            width: 16,
            height: 8,
            pipe_bank_xor: 0,
            layout: crate::tiling::SurfaceLayout::Rx64KbBpp1,
            place: crate::registers::Place::Whole,
        };
        let surface = DispatchSurface::Tiled(level);
        let images = DispatchImages {
            stored_surface: Some((surface, vec![0; surface.bytes()])),
            stored_keys_to_expand: Some(KeysToExpand {
                keys: (0x9000, 3),
                surface: (BASE, 0x3_0000),
            }),
            ..DispatchImages::default()
        };
        let mut texels = vec![0u32; 16 * 8];
        texels[5] = 0x42;
        let writes = super::stored_image_writes(&images, Some(texels))
            .expect("writes")
            .pairs();
        assert_eq!(
            writes[0],
            (BASE, vec![0; 0x3_0000]),
            "the chain zeroed first"
        );
        let at = crate::tiling::tiled_byte_offset_64kb_rx_bpp1_surface(5, 0, 16) as u64;
        assert_eq!(
            writes[1],
            (BASE + 0x2_0000 + at, vec![0x42]),
            "then the one texel"
        );
        assert_eq!(writes[2], (0x9000, vec![crate::dcc::KEY_UNCOMPRESSED; 3]));
        assert_eq!(writes.len(), 3);
    }

    /// A stored image with a few texels changed writes back those texels where they lie, leaving
    /// memory exactly as tiling the whole image back would - and with no texel changed, nothing.
    #[test]
    fn a_few_changed_texels_write_back_as_tiling_the_image_would() {
        use crate::pipeline::{DispatchImages, DispatchSurface, DispatchTexels, TexelFormat};
        let (width, height) = (64u32, 32u32);
        let surface = DispatchSurface::Tiled(super::ColourTarget {
            base: BASE,
            width,
            height,
            pipe_bank_xor: 3,
            layout: crate::tiling::SurfaceLayout::Rx64Kb,
            place: crate::registers::Place::Whole,
        });
        let before: Vec<u32> = (0..width * height)
            .map(|i| i.wrapping_mul(0x9E37_79B9))
            .collect();
        let mut spanned = vec![0; surface.bytes()];
        surface.tile(&before, &mut spanned).expect("tiled");
        let stored = DispatchTexels {
            texels: before.clone().into(),
            format: TexelFormat::Rgba8,
            width,
            height,
            depth: None,
        };
        let known = DispatchImages {
            stored: Some(stored),
            stored_surface: Some((surface, spanned.clone())),
            ..DispatchImages::default()
        };
        let whole = DispatchImages {
            stored: None,
            ..known.clone()
        };
        let applied = |writes: super::GuestWrites| {
            let mut memory = spanned.clone();
            for (at, bytes) in writes.pairs() {
                let at = usize::try_from(at - BASE).expect("in the surface");
                memory[at..at + bytes.len()].copy_from_slice(&bytes);
            }
            memory
        };
        let mut after = before.clone();
        for index in [0, 1, 2, 65, 700, 2047] {
            after[index] ^= 0x0102_0304;
        }
        let few = super::stored_image_writes(&known, Some(after.clone())).expect("writes");
        assert_eq!(few.runs.len(), 6, "one write a texel");
        let tiled = super::stored_image_writes(&whole, Some(after)).expect("writes");
        assert_eq!(applied(few), applied(tiled));
        let none = super::stored_image_writes(&known, Some(before.clone())).expect("writes");
        assert!(none.runs.is_empty());

        // Read through selects that swap red and blue, the image went to the device in the
        // shader's order; what comes back goes into memory in memory's own.
        let order = [2, 1, 0, 3];
        let swapped = |texels: &[u32]| -> Vec<u32> {
            texels
                .iter()
                .map(|&t| t & 0xff00_ff00 | (t & 0xff) << 16 | (t >> 16) & 0xff)
                .collect()
        };
        let reordered = DispatchImages {
            stored: Some(DispatchTexels {
                texels: swapped(&before).into(),
                format: TexelFormat::Rgba8,
                width,
                height,
                depth: None,
            }),
            stored_order: Some(order),
            ..known.clone()
        };
        let mut written = before.clone();
        written[65] = 0x4433_2211;
        let writes =
            super::stored_image_writes(&reordered, Some(swapped(&written))).expect("writes");
        assert_eq!(writes.runs.len(), 1, "the one texel the shader changed");
        assert_eq!(
            applied(writes),
            applied(super::stored_image_writes(&whole, Some(written)).expect("writes")),
            "memory as if no selects had reordered it"
        );
    }

    /// A uniform target is handed over exactly as detiling the same memory would hand it.
    #[test]
    fn a_uniform_target_reads_as_its_detile_would() {
        use super::{ColourTarget, ComponentSwap, draw_over, swapped, words_of};
        let _guard = serial();
        super::forget_written();
        let (width, height) = (16u32, 8u32);
        let clear = 0xff10_2030u32;
        let mut memory = Block(clear.to_le_bytes().repeat(65536 / 4));
        let target = ColourTarget {
            base: BASE,
            width,
            height,
            pipe_bank_xor: 0,
            layout: crate::tiling::SurfaceLayout::Rx64Kb,
            place: crate::registers::Place::Whole,
        };
        let expected = crate::tiling::detile_surface_64kb_rx_bpp4_mapped(
            &words_of(&memory.0),
            width,
            height,
            0,
            |w| swapped(w, ComponentSwap::Alternate),
        );
        let mut handed = None;
        draw_over(&mut memory, target, ComponentSwap::Alternate, |before| {
            handed = before.to_words(128);
            None
        });
        assert_eq!(
            handed,
            Some(expected),
            "every pixel, in the target's byte order"
        );
    }

    /// A target outside the guest's memory is refused unwritten, and the draw is never run.
    #[test]
    fn a_target_outside_guest_memory_is_refused_unwritten() {
        use super::{ColourTarget, ComponentSwap, draw_over};
        // Refused or not, reading the target asks and re-protects the shared slots (D720).
        let _guard = serial();
        let mut memory = Block(vec![0x5a; 65536]);
        let target = ColourTarget {
            base: BASE + 0x1_0000,
            width: 16,
            height: 8,
            pipe_bank_xor: 0,
            layout: crate::tiling::SurfaceLayout::Rx64Kb,
            place: crate::registers::Place::Whole,
        };
        let mut ran = false;
        let wrote = draw_over(&mut memory, target, ComponentSwap::Standard, |before| {
            ran = true;
            before.to_words(128)
        });
        assert!(!wrote && !ran);
        assert!(memory.0.iter().all(|&b| b == 0x5a));
    }

    /// Only a target a frame can be written into exactly qualifies: single-base, single-target,
    /// `64KB_R_X`, `8_8_8_8` `UNORM`; a second base, linear tiling, a scaled format or no format
    /// each refuses, saying which: Grand Theft Auto V's second submission drew into an `8_8_8_8`
    /// `SNORM` target before one was written back (D866), and the refusal named every field but
    /// the one that refused.
    #[test]
    fn a_writable_target_needs_one_tiled_rgba8_surface() {
        use super::writable_target;
        use crate::pipeline::Submission;
        use crate::registers::{
            ColourTarget, ColourTargetExtent, SwizzleMode, decode_colour_target_format,
        };
        let mut submission = Submission {
            colour_target: Some(ColourTarget {
                base: 0x7400_0000_0000,
                width: 1920,
                height: 1080,
                pipe_bank_xor: 0,
                layout: crate::tiling::SurfaceLayout::Rx64Kb,
                place: crate::registers::Place::Whole,
            }),
            colour_target_tiling: Some(SwizzleMode::Tiled64KbRX),
            colour_target_format: Some(decode_colour_target_format(0x0001_80a8 | (1 << 11))),
            colour_target_bases: 1,
            ..Submission::default()
        };
        submission.targets.insert(
            crate::backend::ResourceId(1),
            ColourTargetExtent {
                width: 1920,
                height: 1080,
            },
        );
        assert!(writable_target(&submission).is_ok());
        let refused = |change: fn(&mut Submission)| {
            let mut changed = submission.clone();
            change(&mut changed);
            writable_target(&changed).expect_err("refused")
        };
        assert!(refused(|s| s.colour_target_bases = 2).contains("2 base(s)"));
        assert!(
            refused(|s| s.colour_target_tiling = Some(SwizzleMode::Linear))
                .contains("tiling Some(Linear)")
        );
        assert!(refused(|s| s.colour_target_format = None).contains("never written"));
        assert!(
            refused(|s| {
                s.colour_target_format = Some(decode_colour_target_format(
                    0x0001_80a8 | (1 << 11) | (2 << 8),
                ));
            })
            .contains("COLOR code 10 with number type 2")
        );
    }

    /// SetTFRing answers 0 and writes nothing in PPSA04263's shape, as `166-agc/driver-set-tf-ring`
    /// measured (`20261009-215800-eboot.log`).
    #[test]
    fn set_tf_ring_answers_zero_in_the_title_s_shape() {
        let (_, f) = super::implementations()
            .iter()
            .find(|(n, _)| *n == "sceAgcDriverSetTFRing")
            .expect("implemented");
        let mut ring = [0x55_u8; 0x40];
        let base = ring.as_mut_ptr() as u64;
        let args = [base + 0x200, 0x3fff8, base >> 32, base, 0, 0];
        assert_eq!(f(&args), 0);
        assert!(ring.iter().all(|&b| b == 0x55), "nothing written");
    }

    /// DestroyQueue refuses with `0x8a6d0003` on a queue CreateQueue just made, of every type, as
    /// the console did (`166-agc/driver-create-queue` and `driver-queue-types`,
    /// `20261009-151440-eboot.obs.log`).
    #[test]
    fn destroy_queue_refuses_as_measured() {
        let mut queue_out: u64 = 0;
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[1] = std::ptr::addr_of_mut!(queue_out) as usize as u64;
        assert_eq!(create_queue(&args), 0);
        let (_, destroy) = super::implementations()
            .iter()
            .find(|(n, _)| *n == "sceAgcDriverDestroyQueue")
            .expect("implemented");
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = queue_out;
        assert_eq!(destroy(&args), 0x8a6d_0003);
    }

    /// CreateQueue returns the measured success code and hands back the same non-null queue handle
    /// on every call.
    #[test]
    fn create_queue_hands_back_a_non_null_handle() {
        let mut queue_out: u64 = 0;
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[1] = std::ptr::addr_of_mut!(queue_out) as usize as u64;

        assert_eq!(create_queue(&args), 0, "the measured success code");
        assert_ne!(
            queue_out, 0,
            "and a non-null handle the guest can gate its hardware path on"
        );

        let first = queue_out;
        queue_out = 0;
        assert_eq!(create_queue(&args), 0);
        assert_eq!(queue_out, first, "the same object every call");

        // A null out-parameter is tolerated - the call still returns success, writing nothing.
        let mut null_args = [0_u64; GUEST_ARG_REGISTERS];
        null_args[1] = 0;
        assert_eq!(
            create_queue(&null_args),
            0,
            "a null out-parameter is harmless"
        );
    }

    /// The first refused segment is kept, a later one does not replace it, a drawn one is not
    /// kept, and taking it empties the store.
    #[test]
    fn the_first_refused_segment_is_kept() {
        let _serial = serial();
        let _ = super::take_refused_submission();
        let first = super::Submission {
            guest_memory_base: 1,
            ..super::Submission::default()
        };
        let second = super::Submission {
            guest_memory_base: 2,
            ..super::Submission::default()
        };
        assert!(super::keeping_refused(&second, Ok(())).is_ok());
        assert!(super::take_refused_submission().is_none(), "a drawn one");
        assert!(super::keeping_refused(&first, Err("no".to_owned())).is_err());
        assert!(super::keeping_refused(&second, Err("no".to_owned())).is_err());
        let kept = super::take_refused_submission().map(|s| s.guest_memory_base);
        assert_eq!(kept, Some(1), "the first");
        assert!(super::take_refused_submission().is_none());
    }

    /// These tests share the process-global region and last-submission stores, the last-written
    /// target and its write protection, so they run one at a time - any test reaching `draw_over`,
    /// `read_target` or a submission included. A poisoned lock is recovered rather than cascading a panic across the others.
    ///
    /// Each test starts with no frame bookkeeping left by the one before, finished or panicked:
    /// deferred copies and protection are kept by whole host pages, and a page one test left
    /// guarded can hold the next test's freshly allocated buffer. That copy is then carried out
    /// under the next test's write allowance, refused, and fails whatever touched the page.
    fn serial() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: Mutex<()> = Mutex::new(());
        let guard = LOCK.lock().unwrap_or_else(PoisonError::into_inner);
        // Each test starts on a queue nothing has stopped.
        *super::queue_stall()
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = None;
        super::held()
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
        super::earlier_written()
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
        *super::last_written()
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = None;
        // The fake protection changes no page, so forgetting it restores nothing.
        *super::write_protected()
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = None;
        super::deferred()
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
        super::resolved()
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
        super::set_pending(None);
        guard
    }

    /// A target whose own frame is still deferred onto it from its flip is held by the drawer
    /// (D801): read again to be drawn over, it answers so without the frame being read back or
    /// memory written, and the copy stays deferred.
    #[test]
    fn a_target_under_its_own_deferred_frame_is_held() {
        use super::{ColourTarget, ComponentSwap, GuestCp, MappedRegions, TargetRead, Written};
        use fake::TAKEN;
        use std::sync::Arc;
        use std::sync::atomic::Ordering;
        let _guard = serial();
        let span = crate::tiling::surface_words_64kb_rx_bpp4(16, 8) * 4;
        let memory: &'static mut [u8] = Box::leak(vec![0xAAu8; span].into_boxed_slice());
        let base = memory.as_ptr() as usize as u64;
        let target = ColourTarget {
            base,
            width: 16,
            height: 8,
            pipe_bank_xor: 0,
            layout: crate::tiling::SurfaceLayout::Rx64Kb,
            place: crate::registers::Place::Whole,
        };
        allow_writes_to(base, span as u64);
        fake::install();
        super::forget_written();
        *super::last_written().lock().expect("slot") = Some(Written {
            target,
            contents: super::Contents::Bytes(Arc::new(memory.to_vec())),
            since: None,
        });
        super::set_pending(Some((target, ComponentSwap::Standard)));
        let before = memory.to_vec();
        assert!(super::write_back_at_this_flip());
        let taken = TAKEN.load(Ordering::SeqCst);
        let cp = GuestCp {
            memory: MappedRegions::current(),
            submission: None,
        };
        assert!(matches!(
            super::read_target(&cp, target, ComponentSwap::Standard),
            Some(TargetRead::HeldOverDeferred)
        ));
        assert_eq!(
            TAKEN.load(Ordering::SeqCst),
            taken,
            "the frame is not read back"
        );
        assert_eq!(memory, before.as_slice(), "nor written");
        assert!(
            super::deferred()
                .lock()
                .expect("list")
                .iter()
                .any(|copy| copy.destination == base),
            "the write-back stays deferred"
        );
        super::drop_covered(base, span as u64);
    }

    /// A frame brought back while the fault handler answers a guest access is noted with that
    /// access, once and first only; one brought back by anything else is not; and the note is
    /// forgotten once taken.
    #[test]
    fn a_frame_brought_back_by_a_guest_access_names_the_access() {
        let _guard = serial();
        let _ = super::take_early_touch(0);
        let read = super::GuestTouch {
            rip: 0x4000_1234,
            address: 0x7_0000_0040,
            write: false,
        };
        let write = super::GuestTouch {
            write: true,
            ..read
        };
        super::with_guest_touch(read, || super::note_touched(0x7_0000_0000));
        super::with_guest_touch(write, || super::note_touched(0x7_0000_0000));
        super::note_touched(0x8_0000_0000);
        assert_eq!(super::take_early_touch(0x7_0000_0000), Some(read));
        assert_eq!(
            super::take_early_touch(0x7_0000_0000),
            None,
            "forgotten once taken"
        );
        assert_eq!(
            super::take_early_touch(0x8_0000_0000),
            None,
            "no guest access, no note"
        );
    }

    /// Two targets read in turn, as a frame's display buffers alternate: the second read of the
    /// first answers unchanged while its memory holds the bytes it held when read (D801), and a
    /// target whose memory changed since is read again.
    #[test]
    fn an_earlier_target_whose_bytes_are_unchanged_is_held() {
        use super::{ColourTarget, ComponentSwap, GuestCp, MappedRegions, TargetRead};
        let _guard = serial();
        let span = crate::tiling::surface_words_64kb_rx_bpp4(16, 8) * 4;
        let page = super::HOST_PAGE as usize;
        let stride = span.div_ceil(page) * page;
        let memory: &'static mut [u8] = Box::leak(vec![0u8; 2 * stride + page].into_boxed_slice());
        let aligned = (page - memory.as_ptr() as usize % page) % page;
        let memory = &mut memory[aligned..aligned + 2 * stride];
        for (at, byte) in memory.iter_mut().enumerate() {
            *byte = (at * 7 % 251) as u8;
        }
        let base = memory.as_ptr() as usize as u64;
        let target = |at: u64| ColourTarget {
            base: at,
            width: 16,
            height: 8,
            pipe_bank_xor: 0,
            layout: crate::tiling::SurfaceLayout::Rx64Kb,
            place: crate::registers::Place::Whole,
        };
        let (first, second) = (target(base), target(base + stride as u64));
        set_guest_regions(vec![region_of(memory)]);
        super::forget_written();
        let cp = &mut GuestCp {
            memory: MappedRegions::current(),
            submission: None,
        };
        let read = |cp: &GuestCp<'_>, at| super::read_target(cp, at, ComponentSwap::Standard);
        assert!(matches!(read(cp, first), Some(TargetRead::Changed(_))));
        assert!(matches!(read(cp, second), Some(TargetRead::Changed(_))));
        assert!(
            matches!(read(cp, first), Some(TargetRead::Unchanged)),
            "the first, read before the last, still holds its bytes"
        );
        // The second is no longer the last, nor protected: a write lands, and it is read again.
        memory[stride] ^= 0xff;
        assert!(matches!(read(cp, second), Some(TargetRead::Changed(_))));
    }

    /// A one-byte texel of an alpha target (`SWAP_ALT_REV`) is the frame's alpha, the other
    /// channels zero, and is stored back from it; a red one's is the frame's red (D854).
    #[test]
    fn a_one_byte_alpha_texel_is_the_frame_s_alpha() {
        let target = crate::registers::ColourTarget {
            base: 0,
            width: 1,
            height: 1,
            pipe_bank_xor: 0,
            layout: crate::tiling::SurfaceLayout::Rx64KbBpp1,
            place: crate::registers::Place::Whole,
        };
        let alpha = crate::registers::ComponentSwap::Other(3);
        assert_eq!(super::loaded(0x0000_00AB, target, alpha), 0xAB00_0000);
        assert_eq!(super::stored(0xCD12_3456, target, alpha), 0xCD);
        let red = crate::registers::ComponentSwap::Standard;
        assert_eq!(super::loaded(0x0000_00AB, target, red), 0xFF00_00AB);
        assert_eq!(super::stored(0xCD12_3456, target, red), 0x56);
    }

    /// A fill whose word repeats one texel is uniform; one whose texels differ within the word is
    /// not, at two bytes a texel as at one (D852).
    #[test]
    fn a_fill_is_uniform_only_when_its_texels_are_alike() {
        let target = |layout| crate::registers::ColourTarget {
            base: 0,
            width: 4,
            height: 4,
            pipe_bank_xor: 0,
            layout,
            place: crate::registers::Place::Whole,
        };
        let swap = crate::registers::ComponentSwap::Standard;
        let two = target(crate::tiling::SurfaceLayout::Rx64KbBpp2);
        assert!(matches!(
            super::TargetRead::of_pattern(0x0007_0007, two, swap),
            super::TargetRead::Uniform(7)
        ));
        assert!(matches!(
            super::TargetRead::of_pattern(0x0001_0002, two, swap),
            super::TargetRead::Changed(_)
        ));
    }

    /// A two-byte texel is the low half of its word, loaded as it is and stored as its low half,
    /// with no byte order: one 16-bit integer, red (D852).
    #[test]
    fn a_two_byte_texel_is_its_low_half() {
        let target = crate::registers::ColourTarget {
            base: 0,
            width: 1,
            height: 1,
            pipe_bank_xor: 0,
            layout: crate::tiling::SurfaceLayout::Rx64KbBpp2,
            place: crate::registers::Place::Whole,
        };
        let swap = crate::registers::ComponentSwap::Standard;
        assert_eq!(super::loaded(0xABCD_1234, target, swap), 0x1234);
        assert_eq!(super::stored(0x0001_5678, target, swap), 0x5678);
    }

    /// A submission that stops holds up every later one on the queue: the fence stream submitted
    /// after it is not carried out, so its fence stays unwritten (D705, D710). A command processor
    /// retires its stream in order, and nothing runs the stopped packet.
    #[test]
    fn a_stopped_submission_holds_back_every_later_one() {
        let _guard = serial();
        let fence: &'static mut [u8] = Box::leak(vec![0u8; 64].into_boxed_slice());
        let fence_at = fence.as_ptr() as usize as u64;
        allow_writes_to(fence_at, 64);
        let words = |w: &[u32]| -> Vec<u8> { w.iter().flat_map(|w| w.to_le_bytes()).collect() };
        // An indirect draw: its arguments are in memory at execution time, never carried out here.
        let stopping = words(&[crate::packet::build::command_header(0x24, 4), 0, 0, 0, 0]);
        let release = words(&[
            crate::packet::build::command_header(crate::packet::build::measured::RELEASE_MEM, 7),
            0x0660_3514,
            1 << 29,
            fence_at as u32,
            (fence_at >> 32) as u32,
            0xbeef_cafe,
            0,
            0,
        ]);
        set_guest_regions(vec![
            region_of(&stopping),
            region_of(&release),
            region_of(fence),
        ]);

        let desc = descriptor(&stopping);
        assert_eq!(submit_dcb(&args(desc.as_ptr() as usize as u64)), SUBMIT_OK);
        let desc = descriptor(&release);
        assert_eq!(submit_dcb(&args(desc.as_ptr() as usize as u64)), SUBMIT_OK);

        assert_eq!(&fence[..4], &[0; 4], "no fence for a queue that stopped");
        let record = super::execution();
        assert!(
            matches!(
                record.last.map(|l| l.stopped),
                Some(crate::cp::Stopped::NeedsGpu { opcode: 0x24, .. })
            ),
            "the wall is the stopped submission, not the one behind it: {record:?}"
        );
        assert!(record.held_back >= 1, "{record:?}");
        assert!(
            super::held_writes()
                .iter()
                .any(|held| held.address == fence_at),
            "the fence held back is named among the writes the queue holds (D832)"
        );
    }

    /// The `(start, end)` region a heap buffer occupies, its address exposed for the handler's
    /// read.
    fn region_of(buffer: &[u8]) -> (u64, u64) {
        let base = buffer.as_ptr() as usize as u64;
        (base, base + buffer.len() as u64)
    }

    /// A leaked buffer of `len` bytes holding `byte`, starting off a host page, so a fill over it is
    /// carried out at once rather than deferred (D719). Where a heap buffer starts is the
    /// allocator's choice, and one that happened to be page-aligned would have its fill deferred.
    fn off_page(len: usize, byte: u8) -> &'static mut [u8] {
        let page = super::HOST_PAGE as usize;
        let memory: &'static mut [u8] = Box::leak(vec![byte; len + 2 * page].into_boxed_slice());
        let at = (page - memory.as_ptr() as usize % page) % page + 0x100;
        &mut memory[at..at + len]
    }

    /// Two `SET_CONTEXT_REG` packets (`0xc0016900`, offset, value), as the guest would have built.
    fn command_buffer() -> Vec<u8> {
        [
            0xc001_6900u32,
            0x0000_0318,
            0x4001_4000,
            0xc001_6900,
            0x0000_03b0,
            0x000f_c03f,
        ]
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect()
    }

    /// A minimal shader that translates: `v_mov_b32 v0, 9 ; s_endpgm`.
    fn shader_bytes() -> Vec<u8> {
        [0x7E00_0000u32 | (1 << 9) | (128 + 9), 0xBF81_0000]
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect()
    }

    /// A heap buffer holding the shader at a 256-byte-aligned address (a shader-address register
    /// stores the address in 256-byte units), and that address. The whole buffer is the region to
    /// register.
    fn aligned_shader() -> (Vec<u8>, u64) {
        let shader = shader_bytes();
        let mut buffer = vec![0u8; shader.len() + 512];
        let base = buffer.as_ptr() as usize;
        let aligned = (base + 255) & !255usize;
        let offset = aligned - base;
        buffer[offset..offset + shader.len()].copy_from_slice(&shader);
        (buffer, aligned as u64)
    }

    /// A command stream that writes a graphics shader's address registers to `address`, as a guest
    /// would - the register numbers coming from the crate's own vocabulary, not invented here.
    fn command_naming_a_shader(address: u64) -> Vec<u8> {
        let vocabulary = crate::registers::Vocabulary::builtin().expect("vocabulary");
        // The first non-compute stage's address register pair - a submit uses the graphics queue.
        let mut stage_wanted: Option<String> = None;
        let (mut low, mut high) = (None, None);
        for (register, (stage, is_high)) in vocabulary.shader_registers() {
            if stage == "compute" {
                continue;
            }
            let stage = stage.clone();
            if stage_wanted.get_or_insert(stage.clone()) != &stage {
                continue;
            }
            if *is_high {
                high = Some(*register);
            } else {
                low = Some(*register);
            }
        }
        let (low, high) = (
            low.expect("a graphics shader address register"),
            high.expect("a graphics shader address register"),
        );
        let (opcode, base) = vocabulary
            .opcode_for_register(low)
            .expect("an opcode reaching the shader address registers");
        let mut words: Vec<u32> = Vec::new();
        for (register, value) in [
            (
                low,
                u32::try_from((address >> 8) & 0xFFFF_FFFF).expect("low half"),
            ),
            (high, u32::try_from(address >> 40).expect("high half")),
        ] {
            words.push((3 << 30) | ((2 - 1) << 16) | (u32::from(opcode) << 8));
            words.push(register - base);
            words.push(value);
        }
        words.iter().flat_map(|w| w.to_le_bytes()).collect()
    }

    /// A 16-byte submit descriptor pointing at `buffer`. The pointer is cast through `usize`, which
    /// exposes its provenance for the handler's read.
    fn descriptor(buffer: &[u8]) -> [u8; 16] {
        let mut d = [0u8; 16];
        d[0..8].copy_from_slice(&(buffer.as_ptr() as usize as u64).to_le_bytes());
        d[8..12].copy_from_slice(&u32::try_from(buffer.len() / 4).unwrap().to_le_bytes());
        d
    }

    fn args(descriptor_ptr: u64) -> [u64; GUEST_ARG_REGISTERS] {
        let mut a = [0u64; GUEST_ARG_REGISTERS];
        a[0] = descriptor_ptr;
        a
    }

    /// A submitted command buffer inside a region is walked into a report, and a null descriptor
    /// returns success without faulting.
    #[test]
    fn a_submitted_command_buffer_in_a_region_is_reported() {
        let _guard = serial();
        let buffer = command_buffer();
        set_guest_regions(vec![region_of(&buffer)]);
        let desc = descriptor(&buffer);
        assert_eq!(submit_dcb(&args(desc.as_ptr() as usize as u64)), SUBMIT_OK);
        let report = last_submission_report().expect("a report was recorded");
        assert_eq!(report.packets, 2, "two SET_CONTEXT_REG packets: {report:?}");
        assert!(
            report.register_writes >= 1,
            "registers extracted: {report:?}"
        );

        // A null descriptor: success, no fault, nothing to read.
        assert_eq!(submit_dcb(&args(0)), SUBMIT_OK);
    }

    /// `SubmitCommandBuffer(queue, desc)` reads its descriptor from argument one.
    ///
    /// The same command buffer walks into the same report as through `SubmitDcb`, and a descriptor
    /// left in argument zero is ignored, since that is where the queue handle goes.
    #[test]
    fn submit_command_buffer_reads_the_descriptor_after_the_queue() {
        let _guard = serial();
        let buffer = command_buffer();
        set_guest_regions(vec![region_of(&buffer)]);
        let desc = descriptor(&buffer);
        let mut with_queue = [0u64; GUEST_ARG_REGISTERS];
        with_queue[1] = desc.as_ptr() as usize as u64;
        assert_eq!(submit_command_buffer(&with_queue), SUBMIT_OK);
        let report = last_submission_report().expect("a report was recorded");
        assert_eq!(report.packets, 2, "the same two packets: {report:?}");

        // The descriptor in argument zero and nothing in argument one: nothing to read.
        set_guest_regions(vec![region_of(&buffer)]);
        *super::last_submission().lock().expect("slot") = None;
        assert_eq!(
            submit_command_buffer(&args(desc.as_ptr() as usize as u64)),
            SUBMIT_OK
        );
        assert!(
            last_submission_report().is_none(),
            "argument zero is the queue, never read as a descriptor"
        );
    }

    /// A shader named at an address inside a region resolves; one outside every region does not
    /// (D130). The command buffer's region is registered in both cases, so the difference is the
    /// shader's alone.
    #[test]
    fn a_shader_address_resolves_in_a_region_and_not_outside_one() {
        let _guard = serial();
        let (shader, shader_addr) = aligned_shader();
        let stream = command_naming_a_shader(shader_addr);

        // Resolved: both the command buffer and the shader are in registered regions.
        set_guest_regions(vec![region_of(&stream), region_of(&shader)]);
        let desc = descriptor(&stream);
        assert_eq!(submit_dcb(&args(desc.as_ptr() as usize as u64)), SUBMIT_OK);
        let resolved = last_submission_report().expect("a report");
        assert!(
            resolved.shaders_found >= 1,
            "an address was named: {resolved:?}"
        );
        assert_eq!(resolved.addresses_resolved, 1, "it resolved: {resolved:?}");
        assert_eq!(resolved.addresses_unresolved, 0, "{resolved:?}");
        assert!(
            resolved.shaders_translated >= 1,
            "the resolved shader translated, yielding a BindShader: {resolved:?}"
        );

        // Unresolved: only the command buffer's region is registered, so the shader is out of
        // bounds.
        set_guest_regions(vec![region_of(&stream)]);
        assert_eq!(submit_dcb(&args(desc.as_ptr() as usize as u64)), SUBMIT_OK);
        let unresolved = last_submission_report().expect("a report");
        assert!(unresolved.shaders_found >= 1, "still named: {unresolved:?}");
        assert_eq!(unresolved.addresses_resolved, 0, "{unresolved:?}");
        assert_eq!(
            unresolved.addresses_unresolved, 1,
            "unresolved: {unresolved:?}"
        );
        assert_eq!(unresolved.shaders_translated, 0, "{unresolved:?}");
    }

    /// Lets the command processor write `[base, base + len)` and nothing else - a test's own
    /// buffer, never an address another test names.
    fn allow_writes_to(base: u64, len: u64) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static ALLOWED: (AtomicU64, AtomicU64) = (AtomicU64::new(0), AtomicU64::new(0));
        ALLOWED.0.store(base, Ordering::SeqCst);
        ALLOWED.1.store(len, Ordering::SeqCst);
        super::install_write_lookup(|address, length| {
            let (start, len) = (
                ALLOWED.0.load(Ordering::SeqCst),
                ALLOWED.1.load(Ordering::SeqCst),
            );
            address >= start && address.saturating_add(length) <= start.saturating_add(len)
        });
    }

    /// A buffer's listed words written straight from the list leave memory as writing them run by
    /// run does - consecutive and scattered words, the last cut where the range ends - and a buffer
    /// outside what may be written writes nothing.
    /// Each translated program's dispatches are counted with what they wrote back: the bytes they
    /// changed, and whether any of them is not zero - what says a dispatch that runs is producing
    /// nothing. Zeros through a run, a scattered word or a fill count as written but not as lit.
    #[test]
    fn a_programs_dispatches_are_counted_with_what_they_wrote() {
        use super::{GuestWrites, Scattered, dispatch_writes_by_module, note_dispatch_writes};
        let module = 0x5eed_0000_0000_0883_u64;
        let mut zeros = GuestWrites::default();
        zeros.push(0x1000, &[0, 0, 0, 0]);
        let indices = [0u32];
        let values = [0u32];
        let quiet = Scattered {
            address: 0x2000,
            bytes: 4,
            indices: &indices,
            values: &values,
        };
        let fill = crate::cp::Fill {
            destination: 0x3000,
            pattern: 0,
            count: 8,
        };
        let images = crate::pipeline::DispatchImages {
            fetched_span: Some((0x7400_5b89_1000, 0x80_0000)),
            ..crate::pipeline::DispatchImages::default()
        };
        // Two buffers read: one of zeros, one holding something; and one only stored to.
        let read = [
            BufferBefore::Words(vec![0, 0]),
            BufferBefore::Words(vec![0, 5]),
            BufferBefore::Unread(4),
        ];
        note_dispatch_writes(
            (module, 0x7400_4314_7600),
            (&zeros, &[quiet], &[fill]),
            (&images, &[(0x5000, 8), (0x6000, 8), (0x7000, 16)], &read),
        );
        let found = |module| {
            dispatch_writes_by_module()
                .into_iter()
                .find(|(key, _)| *key == module)
                .map(|(_, writes)| writes)
        };
        let first = found(module).expect("counted");
        assert_eq!((first.dispatches, first.bytes, first.lit), (1, 16, false));
        // Where its images lie, so a texture a draw reads can be matched to the program writing it.
        assert_eq!(first.fetched, Some((0x7400_5b89_1000, 0x80_0000)));
        assert_eq!(first.stored, None);
        // The guest program it runs, so the module can be read against its code.
        assert_eq!(first.program, Some(0x7400_4314_7600));
        // What it read through its buffers: where, how much, and whether any of it is not zero.
        assert_eq!(
            first.buffers,
            vec![
                (0x5000, 8, Some(false)),
                (0x6000, 8, Some(true)),
                (0x7000, 16, None)
            ]
        );
        let mut lit = GuestWrites::default();
        lit.push(0x1000, &[0, 7]);
        note_dispatch_writes(
            (module, 0x7400_4314_7600),
            (&lit, &[], &[]),
            (&crate::pipeline::DispatchImages::default(), &[], &[]),
        );
        let both = found(module).expect("counted");
        assert_eq!((both.dispatches, both.bytes, both.lit), (2, 18, true));
    }

    #[test]
    fn scattered_words_write_back_as_their_runs_do() {
        use super::{GuestCp, GuestWrites, MappedRegions, Scattered};
        let _guard = serial();
        let span = 0x2000usize;
        let memory: &'static mut [u8] = Box::leak(vec![0x5au8; span].into_boxed_slice());
        let base = memory.as_ptr() as usize as u64;
        allow_writes_to(base, span as u64);
        let indices = [0u32, 1, 7, 300, 301, 302, 0x3fe];
        let values: Vec<u32> = indices.iter().map(|&i| 0x1000_0000 | i).collect();
        // The range ends two bytes into the last word.
        let buffer = Scattered {
            address: base + 0x10,
            bytes: 0x3fe * 4 + 2,
            indices: &indices,
            values: &values,
        };
        let mut cp = GuestCp {
            memory: MappedRegions::current(),
            submission: None,
        };
        cp.write_back_dispatch(&GuestWrites::default(), &[buffer], &[])
            .expect("written");
        let scattered = memory.to_vec();

        memory.fill(0x5a);
        let mut runs = GuestWrites::default();
        buffer.push_runs(&mut runs);
        assert_eq!(runs.runs.len(), 4, "consecutive words make one run");
        cp.write_back_dispatch(&runs, &[], &[]).expect("written");
        assert_eq!(scattered, memory.to_vec());
        let last = 0x10 + 0x3fe * 4;
        assert_eq!(&scattered[last..last + 4], &[0xfe, 0x03, 0x5a, 0x5a]);

        memory.fill(0x5a);
        let outside = Scattered {
            address: base + span as u64,
            ..buffer
        };
        assert!(
            cp.write_back_dispatch(&GuestWrites::default(), &[outside], &[])
                .is_err()
        );
        assert!(memory.iter().all(|&byte| byte == 0x5a), "nothing written");
    }

    /// The lazy-copy hooks every test here shares (the first install wins): snapshots and
    /// protection that are counted, not real. No page's protection changes, so a test drives the
    /// fault path by calling what the handler would.
    mod fake {
        use std::sync::atomic::{AtomicU64, Ordering};

        pub(super) static SNAPSHOTS: AtomicU64 = AtomicU64::new(0);
        pub(super) static TAKEN: AtomicU64 = AtomicU64::new(0);
        pub(super) static DISCARDED: AtomicU64 = AtomicU64::new(0);
        pub(super) static PROTECTED: AtomicU64 = AtomicU64::new(0);
        pub(super) static GUARDS: AtomicU64 = AtomicU64::new(0);
        pub(super) static RELEASES: AtomicU64 = AtomicU64::new(0);

        /// The frame every snapshot holds: a 16x8 ramp.
        pub(super) fn frame() -> Vec<u32> {
            (0..128u32).map(|i| 0xff00_0000 | (i * 0x0101)).collect()
        }

        pub(super) fn install() {
            super::super::install_lazy_copies(super::super::LazyCopies {
                snapshot: || Some(SNAPSHOTS.fetch_add(1, Ordering::SeqCst)),
                show: || Some(SNAPSHOTS.fetch_add(1, Ordering::SeqCst)),
                take: |_, _| {
                    TAKEN.fetch_add(1, Ordering::SeqCst);
                    Some(frame())
                },
                discard: |_| {
                    DISCARDED.fetch_add(1, Ordering::SeqCst);
                },
                replace: |_| {
                    DISCARDED.fetch_add(1, Ordering::SeqCst);
                    Some(SNAPSHOTS.fetch_add(1, Ordering::SeqCst))
                },
                guard: |_, _| {
                    GUARDS.fetch_add(1, Ordering::SeqCst);
                    Some(0)
                },
                protect_writes: |_, _| {
                    PROTECTED.fetch_add(1, Ordering::SeqCst);
                    Some(4)
                },
                release: |_, _, _| {
                    RELEASES.fetch_add(1, Ordering::SeqCst);
                    true
                },
            });
        }
    }

    /// A target is compared only when it may have been written (D720).
    ///
    /// Read once, the target is protected; read again, it is unchanged on the protection's word,
    /// even with memory changed behind the fake protection's back. A write then faults
    /// (`written_at`), and the next read compares and finds the change. A command-processor fill
    /// releases the protection.
    #[test]
    fn a_protected_target_is_trusted_until_a_write_to_it_faults() {
        use super::{
            ColourTarget, ComponentSwap, GuestCp, MappedRegions, TargetRead, Written, cp::CpMemory,
        };
        use std::sync::Arc;
        let _guard = serial();
        let span = crate::tiling::surface_words_64kb_rx_bpp4(16, 8) * 4;
        let memory: &'static mut [u8] = Box::leak(vec![0x10u8; span].into_boxed_slice());
        let base = memory.as_ptr() as usize as u64;
        allow_writes_to(base, span as u64);
        set_guest_regions(vec![region_of(memory)]);
        fake::install();
        let target = ColourTarget {
            base,
            width: 16,
            height: 8,
            pipe_bank_xor: 0,
            layout: crate::tiling::SurfaceLayout::Rx64Kb,
            place: crate::registers::Place::Whole,
        };
        *super::last_written().lock().expect("slot") = Some(Written {
            target,
            contents: super::Contents::Bytes(Arc::new(memory.to_vec())),
            since: None,
        });
        let mut cp = GuestCp {
            memory: MappedRegions::current(),
            submission: None,
        };
        let read = |cp: &GuestCp<'_>| {
            super::read_target(cp, target, ComponentSwap::Standard).expect("readable")
        };

        assert!(
            matches!(read(&cp), TargetRead::Unchanged),
            "compared: the bytes kept"
        );
        memory[0] = 0x99;
        assert!(
            matches!(read(&cp), TargetRead::Unchanged),
            "protected and unwritten: trusted, not compared"
        );
        assert!(
            super::written_at(base),
            "a write to it faults, and is answered"
        );
        assert!(
            !super::written_at(base),
            "and only once: its pages are its own again"
        );
        assert!(
            matches!(read(&cp), TargetRead::Changed(_)),
            "compared, and changed"
        );

        assert!(matches!(read(&cp), TargetRead::Unchanged));
        assert!(cp.fill(base, 0, 4), "a fill of its first word");
        assert!(
            matches!(read(&cp), TargetRead::Changed(_)),
            "the command processor's own write is seen too"
        );
        super::forget_written();
    }

    /// A flipped frame reaches memory when something reads it, and not before (D719).
    ///
    /// A submission's preparation reads guest memory with earlier writes settled: a read that
    /// misses the target a frame is pending for leaves it pending, and one that reaches it writes the
    /// frame back first, so a texture one submission drew is what the next samples. mesa-cube
    /// builds its checkerboard's levels this way, and read zeros.
    #[test]
    fn a_read_while_preparing_settles_a_frame_pending_over_it() {
        use super::{
            ColourTarget, ComponentSwap, GuestMemory, MappedRegions, SettledReads, Written,
        };
        use fake::frame;
        use std::sync::Arc;
        let _guard = serial();
        let span = crate::tiling::surface_words_64kb_rx_bpp4(16, 8) * 4;
        let memory: &'static mut [u8] = Box::leak(vec![0xAAu8; span].into_boxed_slice());
        let base = memory.as_ptr() as usize as u64;
        let target = ColourTarget {
            base,
            width: 16,
            height: 8,
            pipe_bank_xor: 0,
            layout: crate::tiling::SurfaceLayout::Rx64Kb,
            place: crate::registers::Place::Whole,
        };
        allow_writes_to(base, span as u64);
        fake::install();
        *super::last_written().lock().expect("slot") = Some(Written {
            target,
            contents: super::Contents::Bytes(Arc::new(memory.to_vec())),
            since: None,
        });
        super::set_pending(Some((target, ComponentSwap::Standard)));
        let before = memory.to_vec();
        let regions = MappedRegions::current();
        let reads = SettledReads(&regions);

        let _ = reads.read(base + span as u64, 4);
        assert!(
            super::pending_target().is_some(),
            "a read elsewhere leaves it"
        );

        let _ = reads.read(base, 4);
        assert!(super::pending_target().is_none(), "written back");
        let mut expected = super::words_of(&before);
        crate::tiling::tile_surface_64kb_rx_bpp4_mapped(&frame(), 16, 8, 0, &mut expected, |w| w)
            .expect("tiles");
        assert_eq!(
            memory,
            zerocopy::IntoBytes::as_bytes(expected.as_slice()),
            "memory holds the frame before the read returns"
        );
    }

    /// The flip leaves memory as it was and keeps a snapshot; the first read carries it out, and
    /// memory then holds the frame tiled over what it held. A flip followed by a fill covering the
    /// whole target never reads its snapshot, and memory holds the fill.
    #[test]
    fn a_flipped_frame_is_written_back_on_first_read_or_dropped_by_a_covering_fill() {
        use super::{ColourTarget, ComponentSwap, GuestCp, MappedRegions, Written, cp::CpMemory};
        use fake::{DISCARDED, TAKEN, frame};
        use std::sync::Arc;
        use std::sync::atomic::Ordering;
        let _guard = serial();
        let target = ColourTarget {
            base: 0,
            width: 16,
            height: 8,
            pipe_bank_xor: 0,
            layout: crate::tiling::SurfaceLayout::Rx64Kb,
            place: crate::registers::Place::Whole,
        };
        let span = crate::tiling::surface_words_64kb_rx_bpp4(16, 8) * 4;
        let memory = off_page(span, 0xAA);
        let base = memory.as_ptr() as usize as u64;
        let target = ColourTarget { base, ..target };
        allow_writes_to(base, span as u64);
        fake::install();
        let pend = |memory: &[u8]| {
            *super::last_written().lock().expect("slot") = Some(Written {
                target,
                contents: super::Contents::Bytes(Arc::new(memory.to_vec())),
                since: None,
            });
            super::set_pending(Some((target, ComponentSwap::Standard)));
        };

        pend(memory);
        let before = memory.to_vec();
        assert!(super::write_back_at_this_flip());
        assert!(super::pending_target().is_none(), "nothing left pending");
        assert_eq!(memory, before.as_slice(), "nothing written at the flip");
        assert!(super::carry_out_before_reading(base, 4));
        let mut expected = super::words_of(&before);
        crate::tiling::tile_surface_64kb_rx_bpp4_mapped(&frame(), 16, 8, 0, &mut expected, |w| w)
            .expect("tiles");
        assert_eq!(
            memory,
            zerocopy::IntoBytes::as_bytes(expected.as_slice()),
            "the first read finds the frame, as D714 wrote it"
        );

        let taken = TAKEN.load(Ordering::SeqCst);
        let discarded = DISCARDED.load(Ordering::SeqCst);
        pend(memory);
        assert!(super::write_back_at_this_flip());
        let mut cp = GuestCp {
            memory: MappedRegions::current(),
            submission: None,
        };
        assert!(cp.fill(base, 0x1122_3344, span));
        assert_eq!(
            TAKEN.load(Ordering::SeqCst),
            taken,
            "a covered frame is never read"
        );
        assert_eq!(
            DISCARDED.load(Ordering::SeqCst),
            discarded + 1,
            "its snapshot is let go"
        );
        assert!(
            memory
                .chunks_exact(4)
                .all(|w| w == 0x1122_3344u32.to_le_bytes()),
            "memory holds the fill"
        );
        assert!(super::deferred().lock().expect("list").is_empty());
        super::forget_written();
    }

    /// A frame let go of for a draw into another target is kept on the device, not written back
    /// (D719), and the first read carries it out, the frame tiled in the target's order. That
    /// target's frame is written back at the next switch rather than kept: something reads it.
    #[test]
    fn a_frame_switched_from_is_kept_until_read_then_written_at_the_switch() {
        use super::{ColourTarget, ComponentSwap, GuestCp, MappedRegions, Written};
        use fake::{SNAPSHOTS, frame};
        use std::sync::Arc;
        use std::sync::atomic::Ordering;
        let _guard = serial();
        let span = crate::tiling::surface_words_64kb_rx_bpp4(16, 8) * 4;
        let memory = off_page(span, 0x5A);
        let base = memory.as_ptr() as usize as u64;
        let target = ColourTarget {
            base,
            width: 16,
            height: 8,
            pipe_bank_xor: 0,
            layout: crate::tiling::SurfaceLayout::Rx64Kb,
            place: crate::registers::Place::Whole,
        };
        allow_writes_to(base, span as u64);
        fake::install();
        super::install_frame_reader(|| Some(frame()));
        // The alternate order, so memory holds the frame as stored, not as drawn.
        let pend = |memory: &[u8]| {
            *super::last_written().lock().expect("slot") = Some(Written {
                target,
                contents: super::Contents::Bytes(Arc::new(memory.to_vec())),
                since: None,
            });
            super::set_pending(Some((target, ComponentSwap::Alternate)));
        };
        let before = memory.to_vec();
        let mut expected = super::words_of(&before);
        crate::tiling::tile_surface_64kb_rx_bpp4_mapped(&frame(), 16, 8, 0, &mut expected, |w| {
            super::stored(w, target, ComponentSwap::Alternate)
        })
        .expect("tiles");
        let expected = zerocopy::IntoBytes::as_bytes(expected.as_slice()).to_vec();
        let mut cp = GuestCp {
            memory: MappedRegions::current(),
            submission: None,
        };

        pend(memory);
        assert!(super::hold_pending(&mut cp));
        assert!(super::pending_target().is_none(), "let go of");
        assert_eq!(memory, before.as_slice(), "and not written back");
        assert!(super::carry_out_before_reading(base, 4));
        assert_eq!(
            memory,
            expected.as_slice(),
            "the first read finds the frame"
        );

        // Memory as it was, so only a write-back at the switch puts the frame there.
        memory.copy_from_slice(&before);
        pend(memory);
        let snapshots = SNAPSHOTS.load(Ordering::SeqCst);
        assert!(super::hold_pending(&mut cp));
        assert!(super::pending_target().is_none(), "let go of again");
        assert_eq!(
            SNAPSHOTS.load(Ordering::SeqCst),
            snapshots,
            "read after its last switch, it is not kept"
        );
        assert!(super::deferred().lock().expect("list").is_empty());
        assert_eq!(memory, expected.as_slice(), "but written back");
        super::switch_held().lock().expect("held").clear();
        super::forget_written();
    }

    /// `sceAgcDriverAddEqEvent` answers `0` for a live queue and `0x80020009` for any other handle
    /// (obSCEne REQ-20260928T1338Z-5b78).
    #[test]
    fn add_eq_event_answers_as_measured() {
        super::install_queue_registration(|handle, _, _| handle == 0x5e2d_0000_ee40);
        let served = super::implementations()
            .iter()
            .find(|(name, _)| *name == "sceAgcDriverAddEqEvent")
            .map(|&(_, f)| f)
            .expect("served");
        assert_eq!(served(&[0x5e2d_0000_ee40, 0, 0, 0, 0, 0]), 0);
        assert_eq!(served(&[0x5e2d_0000_ee40, 1, 0, 0, 0, 0]), 0);
        assert_eq!(served(&[0xdead, 0, 0, 0, 0, 0]), 0x8002_0009);
    }

    /// `sceAgcDriverGetEqEventType` answers a kevent's `data` and `sceAgcDriverGetEqContextId` the
    /// part above its low sixteen bits, for the kevents obSCEne -eq01 handed them.
    #[test]
    fn eq_accessors_answer_as_measured() {
        let mut kevent: [u8; 32] = [
            0, 0, 0, 0, 0, 0, 0, 0, 0xf2, 0xff, 0x20, 0, 0, 0, 0, 0, //
            0, 0xff, 0x01, 0x01, 0, 0, 0, 0, 0x44, 0x33, 0x22, 0x11, 0, 0, 0, 0,
        ];
        let call = |kevent: &mut [u8; 32], f: fn(&[u64; GUEST_ARG_REGISTERS]) -> u64| {
            f(&[kevent.as_mut_ptr() as u64, 0, 0, 0, 0, 0])
        };
        assert_eq!(call(&mut kevent, super::get_eq_event_type), 0x101_ff00);
        assert_eq!(call(&mut kevent, super::get_eq_context_id), 0x101);
        for (data, context) in [
            (0x0800_0101_ff00_u64, 0x0800_0101),
            (0xdead_beef, 0xdead),
            (0xff00, 0),
            (0, 0),
        ] {
            kevent[16..24].copy_from_slice(&data.to_le_bytes());
            assert_eq!(call(&mut kevent, super::get_eq_event_type), data);
            assert_eq!(call(&mut kevent, super::get_eq_context_id), context);
        }
    }

    /// `sceAgcDriverSetHsOffchipParam` is served and answers the measured `0` for PPSA02664's
    /// arguments and for zeros (obSCEne REQ-20260927T1130Z-b3c2).
    #[test]
    fn set_hs_offchip_param_answers_zero_as_measured() {
        let served = super::implementations()
            .iter()
            .find(|(name, _)| *name == "sceAgcDriverSetHsOffchipParam")
            .map(|&(_, f)| f)
            .expect("served");
        assert_eq!(served(&[0, 0x1ff, 0x7400, 0, 3, 0x1e44]), 0);
        assert_eq!(served(&[0; GUEST_ARG_REGISTERS]), 0);
    }

    /// A DMA copy over the whole of a deferred copy's destination drops it unread, as a covering
    /// fill does (D719). Bugdom copies its target into one buffer at the end of every submission;
    /// carrying the last frame's copy out only for the next frame's first copy to overwrite it was
    /// a whole-frame tiling per frame.
    #[test]
    fn a_copy_over_a_deferred_frame_drops_it_unread() {
        use super::{ColourTarget, ComponentSwap, GuestCp, MappedRegions, Written, cp::CpMemory};
        use fake::{DISCARDED, TAKEN};
        use std::sync::Arc;
        use std::sync::atomic::Ordering;
        let _guard = serial();
        let span = crate::tiling::surface_words_64kb_rx_bpp4(16, 8) * 4;
        // The copy's source, then the target it is copied over, a host page apart: a deferred copy
        // is guarded in whole pages, and a source sharing one would be read through it.
        let page = super::HOST_PAGE as usize;
        let memory: &'static mut [u8] =
            Box::leak(vec![0xAAu8; 2 * span + 3 * page].into_boxed_slice());
        let aligned = (page - memory.as_ptr() as usize % page) % page;
        let memory = &mut memory[aligned..];
        let gap = span + page;
        memory[..span].fill(0x5C);
        let source = memory.as_ptr() as usize as u64;
        let base = source + gap as u64;
        let target = ColourTarget {
            base,
            width: 16,
            height: 8,
            pipe_bank_xor: 0,
            layout: crate::tiling::SurfaceLayout::Rx64Kb,
            place: crate::registers::Place::Whole,
        };
        allow_writes_to(base, span as u64);
        set_guest_regions(vec![region_of(memory)]);
        fake::install();
        *super::last_written().lock().expect("slot") = Some(Written {
            target,
            contents: super::Contents::Bytes(Arc::new(memory[gap..gap + span].to_vec())),
            since: None,
        });
        super::set_pending(Some((target, ComponentSwap::Standard)));
        assert!(super::write_back_at_this_flip());
        assert!(
            !super::deferred().lock().expect("list").is_empty(),
            "deferred"
        );

        let taken = TAKEN.load(Ordering::SeqCst);
        let discarded = DISCARDED.load(Ordering::SeqCst);
        let mut cp = GuestCp {
            memory: MappedRegions::current(),
            submission: None,
        };
        assert!(cp.copy(source, base, span));
        assert_eq!(
            TAKEN.load(Ordering::SeqCst),
            taken,
            "a covered frame is never read"
        );
        assert_eq!(
            DISCARDED.load(Ordering::SeqCst),
            discarded + 1,
            "its snapshot is let go"
        );
        assert!(
            memory[gap..gap + span].iter().all(|&b| b == 0x5C),
            "memory holds the copy"
        );
        assert!(super::deferred().lock().expect("list").is_empty());
        super::forget_written();
    }

    /// A fill of a whole target is deferred (D719): memory is untouched until something reads it,
    /// the next draw is handed the fill as a uniform target without memory being read, and a read
    /// finds the fill. Bugdom clears its colour and depth targets every frame, 8 MB each.
    #[test]
    fn a_whole_target_fill_waits_until_read_and_answers_as_uniform() {
        use super::{
            ColourTarget, ComponentSwap, GuestCp, MappedRegions, TargetRead, cp::CpMemory,
        };
        let _guard = serial();
        let span = crate::tiling::surface_words_64kb_rx_bpp4(16, 8) * 4;
        let page = super::HOST_PAGE as usize;
        let memory: &'static mut [u8] = Box::leak(vec![0xAAu8; span + page].into_boxed_slice());
        let aligned = (page - memory.as_ptr() as usize % page) % page;
        let memory = &mut memory[aligned..aligned + span];
        let base = memory.as_ptr() as usize as u64;
        let target = ColourTarget {
            base,
            width: 16,
            height: 8,
            pipe_bank_xor: 0,
            layout: crate::tiling::SurfaceLayout::Rx64Kb,
            place: crate::registers::Place::Whole,
        };
        allow_writes_to(base, span as u64);
        set_guest_regions(vec![region_of(memory)]);
        fake::install();
        super::forget_written();
        let mut cp = GuestCp {
            memory: MappedRegions::current(),
            submission: None,
        };

        assert!(cp.fill(base, 0xff00_0000, span));
        assert!(
            memory.iter().all(|&b| b == 0xAA),
            "nothing written at the fill"
        );
        let read = |cp: &GuestCp<'_>| {
            super::read_target(cp, target, ComponentSwap::Standard).expect("readable")
        };
        assert!(
            matches!(read(&cp), TargetRead::Uniform(0xff00_0000)),
            "the fill, as the draw's starting point"
        );
        assert!(
            memory.iter().all(|&b| b == 0xAA),
            "and still nothing read or written"
        );
        assert!(
            matches!(read(&cp), TargetRead::Unchanged),
            "asked again: unchanged"
        );

        assert!(super::carry_out_before_reading(base, 4));
        assert!(
            memory
                .chunks_exact(4)
                .all(|w| w == 0xff00_0000u32.to_le_bytes()),
            "a read finds the fill"
        );
        assert!(super::deferred().lock().expect("list").is_empty());
        super::forget_written();
    }

    /// A frame drawn over a deferred fill and flipped supersedes the fill, and the first read finds
    /// the frame tiled over the fill's words - the fill never written on its own.
    #[test]
    fn a_frame_flipped_over_a_deferred_fill_is_tiled_over_its_words() {
        use super::{
            ColourTarget, ComponentSwap, GuestCp, MappedRegions, TargetRead, cp::CpMemory,
        };
        use fake::frame;
        let _guard = serial();
        let span = crate::tiling::surface_words_64kb_rx_bpp4(16, 8) * 4;
        let page = super::HOST_PAGE as usize;
        let memory: &'static mut [u8] = Box::leak(vec![0xAAu8; span + page].into_boxed_slice());
        let aligned = (page - memory.as_ptr() as usize % page) % page;
        let memory = &mut memory[aligned..aligned + span];
        let base = memory.as_ptr() as usize as u64;
        let target = ColourTarget {
            base,
            width: 16,
            height: 8,
            pipe_bank_xor: 0,
            layout: crate::tiling::SurfaceLayout::Rx64Kb,
            place: crate::registers::Place::Whole,
        };
        allow_writes_to(base, span as u64);
        set_guest_regions(vec![region_of(memory)]);
        fake::install();
        super::forget_written();
        let cp = &mut GuestCp {
            memory: MappedRegions::current(),
            submission: None,
        };
        assert!(cp.fill(base, 0x1234_5678, span));
        assert!(matches!(
            super::read_target(cp, target, ComponentSwap::Standard),
            Some(TargetRead::Uniform(_))
        ));
        super::set_pending(Some((target, ComponentSwap::Standard)));
        assert!(super::write_back_at_this_flip());
        assert_eq!(
            super::deferred().lock().expect("list").len(),
            1,
            "the fill superseded by the frame"
        );
        assert!(memory.iter().all(|&b| b == 0xAA), "nothing written yet");

        assert!(super::carry_out_before_reading(base, 4));
        let mut expected = vec![0x1234_5678u32; span / 4];
        crate::tiling::tile_surface_64kb_rx_bpp4_mapped(&frame(), 16, 8, 0, &mut expected, |w| w)
            .expect("tiles");
        assert_eq!(memory, zerocopy::IntoBytes::as_bytes(expected.as_slice()));
        super::forget_written();
    }

    /// A buffer filled again whole while its last fill is still deferred keeps the one guard: the
    /// deferral becomes the new fill rather than giving its pages back to be guarded again, and
    /// memory, once carried out, holds the second fill's word.
    #[test]
    fn a_buffer_filled_again_keeps_its_guard() {
        use super::{GuestCp, MappedRegions, cp::CpMemory};
        use fake::{GUARDS, RELEASES};
        use std::sync::atomic::Ordering;
        let _guard = serial();
        let span = 0x2_0000;
        let page = super::HOST_PAGE as usize;
        let memory: &'static mut [u8] = Box::leak(vec![0xAAu8; span + page].into_boxed_slice());
        let aligned = (page - memory.as_ptr() as usize % page) % page;
        let memory = &mut memory[aligned..aligned + span];
        let base = memory.as_ptr() as usize as u64;
        allow_writes_to(base, span as u64);
        set_guest_regions(vec![region_of(memory)]);
        fake::install();
        super::forget_written();
        let cp = &mut GuestCp {
            memory: MappedRegions::current(),
            submission: None,
        };
        let changes = || GUARDS.load(Ordering::SeqCst) + RELEASES.load(Ordering::SeqCst);
        let before = changes();
        assert!(cp.fill(base, 0x1111_1111, span));
        assert!(cp.fill(base, 0x2222_2222, span));
        assert_eq!(changes(), before + 1, "guarded once, by the first fill");
        assert_eq!(super::deferred().lock().expect("list").len(), 1);
        assert!(memory.iter().all(|&b| b == 0xAA), "nothing written yet");
        assert!(super::carry_out_before_reading(base, 4));
        assert!(memory.iter().all(|&b| b == 0x22), "the second fill");
        super::forget_written();
    }

    /// A copy of a frame drawn over a deferred fill is deferred without carrying the fill out:
    /// Bugdom copies its target into a readback buffer after every submission, and each carried
    /// out its 8 MB clear. The copy's destination later reads as the frame over the fill's words.
    #[test]
    fn a_copy_of_a_frame_over_a_deferred_fill_leaves_the_fill_deferred() {
        use super::{
            ColourTarget, ComponentSwap, GuestCp, MappedRegions, TargetRead, cp::CpMemory,
        };
        use fake::frame;
        let _guard = serial();
        let span = crate::tiling::surface_words_64kb_rx_bpp4(16, 8) * 4;
        let page = super::HOST_PAGE as usize;
        let gap = span + page;
        let memory: &'static mut [u8] =
            Box::leak(vec![0xAAu8; gap + span + page].into_boxed_slice());
        let aligned = (page - memory.as_ptr() as usize % page) % page;
        let memory = &mut memory[aligned..aligned + gap + span];
        let base = memory.as_ptr() as usize as u64;
        let readback = base + gap as u64;
        let target = ColourTarget {
            base,
            width: 16,
            height: 8,
            pipe_bank_xor: 0,
            layout: crate::tiling::SurfaceLayout::Rx64Kb,
            place: crate::registers::Place::Whole,
        };
        allow_writes_to(base, memory.len() as u64);
        set_guest_regions(vec![region_of(memory)]);
        fake::install();
        super::forget_written();
        let cp = &mut GuestCp {
            memory: MappedRegions::current(),
            submission: None,
        };
        assert!(cp.fill(base, 0x1234_5678, span));
        assert!(matches!(
            super::read_target(cp, target, ComponentSwap::Standard),
            Some(TargetRead::Uniform(_))
        ));
        super::set_pending(Some((target, ComponentSwap::Standard)));

        assert!(cp.copy(base, readback, span));
        assert!(
            memory.iter().all(|&b| b == 0xAA),
            "neither the fill nor the copy written"
        );
        assert!(super::carry_out_before_reading(readback, 4));
        let mut expected = vec![0x1234_5678u32; span / 4];
        crate::tiling::tile_surface_64kb_rx_bpp4_mapped(&frame(), 16, 8, 0, &mut expected, |w| w)
            .expect("tiles");
        assert_eq!(
            &memory[gap..],
            zerocopy::IntoBytes::as_bytes(expected.as_slice())
        );
        assert!(
            memory[..span].iter().all(|&b| b == 0xAA),
            "the target's fill still deferred"
        );
        super::set_pending(None);
        assert!(super::carry_out_before_reading(base, span as u64));
        super::forget_written();
    }

    /// A copy of the frame's first bytes - a GL context copying its tiled target by the size it
    /// would have linear - is deferred as a whole copy is, and its destination later reads as the
    /// tiled frame cut short, with nothing past the copy written.
    #[test]
    fn a_copy_of_the_frame_s_first_bytes_is_deferred() {
        use super::{
            ColourTarget, ComponentSwap, GuestCp, MappedRegions, TargetRead, cp::CpMemory,
        };
        use fake::frame;
        let _guard = serial();
        let span = crate::tiling::surface_words_64kb_rx_bpp4(16, 8) * 4;
        let short = span - 0x1000;
        let page = super::HOST_PAGE as usize;
        let gap = span + page;
        let memory: &'static mut [u8] =
            Box::leak(vec![0xAAu8; gap + span + page].into_boxed_slice());
        let aligned = (page - memory.as_ptr() as usize % page) % page;
        let memory = &mut memory[aligned..aligned + gap + span];
        let base = memory.as_ptr() as usize as u64;
        let readback = base + gap as u64;
        let target = ColourTarget {
            base,
            width: 16,
            height: 8,
            pipe_bank_xor: 0,
            layout: crate::tiling::SurfaceLayout::Rx64Kb,
            place: crate::registers::Place::Whole,
        };
        allow_writes_to(base, memory.len() as u64);
        set_guest_regions(vec![region_of(memory)]);
        fake::install();
        super::forget_written();
        let cp = &mut GuestCp {
            memory: MappedRegions::current(),
            submission: None,
        };
        assert!(cp.fill(base, 0x1234_5678, span));
        assert!(matches!(
            super::read_target(cp, target, ComponentSwap::Standard),
            Some(TargetRead::Uniform(_))
        ));
        super::set_pending(Some((target, ComponentSwap::Standard)));

        assert!(cp.copy(base, readback, short));
        assert!(
            memory[gap..].iter().all(|&b| b == 0xAA),
            "the copy deferred, not written"
        );
        assert!(super::carry_out_before_reading(readback, 4));
        let mut expected = vec![0x1234_5678u32; span / 4];
        crate::tiling::tile_surface_64kb_rx_bpp4_mapped(&frame(), 16, 8, 0, &mut expected, |w| w)
            .expect("tiles");
        assert_eq!(
            &memory[gap..gap + short],
            &zerocopy::IntoBytes::as_bytes(expected.as_slice())[..short]
        );
        assert!(
            memory[gap + short..].iter().all(|&b| b == 0xAA),
            "nothing past the copy written"
        );
        super::set_pending(None);
        assert!(super::carry_out_before_reading(base, span as u64));
        super::forget_written();
    }

    /// A copy out of a deferred fill with nothing drawn over it is a fill of its destination: the
    /// source is neither carried out nor read. Bugdom's loading screen clears and copies every
    /// frame without drawing.
    #[test]
    fn a_copy_out_of_a_deferred_fill_fills_its_destination() {
        use super::{GuestCp, MappedRegions, cp::CpMemory};
        let _guard = serial();
        let span = 0x1_0000usize;
        let page = super::HOST_PAGE as usize;
        let gap = span + page;
        let memory: &'static mut [u8] =
            Box::leak(vec![0xAAu8; gap + span + page].into_boxed_slice());
        let aligned = (page - memory.as_ptr() as usize % page) % page;
        let memory = &mut memory[aligned..aligned + gap + span];
        let source = memory.as_ptr() as usize as u64;
        let destination = source + gap as u64;
        allow_writes_to(source, memory.len() as u64);
        set_guest_regions(vec![region_of(memory)]);
        fake::install();
        super::forget_written();
        super::set_pending(None);
        let cp = &mut GuestCp {
            memory: MappedRegions::current(),
            submission: None,
        };
        assert!(cp.fill(source, 0x0102_0304, span));
        assert!(cp.copy(source, destination, span));
        assert!(memory.iter().all(|&b| b == 0xAA), "nothing written or read");
        assert!(super::carry_out_before_reading(destination, span as u64));
        assert!(
            memory[gap..]
                .chunks_exact(4)
                .all(|w| w == 0x0102_0304u32.to_le_bytes()),
            "the destination holds the fill"
        );
        assert!(
            memory[..span].iter().all(|&b| b == 0xAA),
            "the source still deferred"
        );
        assert!(super::carry_out_before_reading(source, span as u64));
        super::forget_written();
    }

    /// A guest write to a target whose frame is still on the device lands on that frame: on hardware
    /// the draws are in memory by then, so the pending frame is written back before the faulting
    /// write runs. Bugdom's menu writes a few words into its target between a frame's submissions;
    /// letting the write through onto the pre-draw memory handed the next submission the clear, and
    /// every object drawn before it was lost.
    #[test]
    fn a_guest_write_to_a_pending_target_lands_on_its_frame() {
        use super::{ColourTarget, ComponentSwap, Written};
        use fake::frame;
        use std::sync::Arc;
        let _guard = serial();
        let span = crate::tiling::surface_words_64kb_rx_bpp4(16, 8) * 4;
        let memory: &'static mut [u8] = Box::leak(vec![0xAAu8; span].into_boxed_slice());
        let base = memory.as_ptr() as usize as u64;
        let target = ColourTarget {
            base,
            width: 16,
            height: 8,
            pipe_bank_xor: 0,
            layout: crate::tiling::SurfaceLayout::Rx64Kb,
            place: crate::registers::Place::Whole,
        };
        allow_writes_to(base, span as u64);
        set_guest_regions(vec![region_of(memory)]);
        fake::install();
        // The device's frame, as the worker's reader answers it.
        super::install_frame_reader(|| Some(frame()));
        let before = memory.to_vec();
        *super::last_written().lock().expect("slot") = Some(Written {
            target,
            contents: super::Contents::Bytes(Arc::new(before.clone())),
            since: None,
        });
        super::set_pending(Some((target, ComponentSwap::Standard)));
        super::protect_target(target);

        assert!(
            super::written_at(base),
            "the write is the protected target's"
        );
        assert!(
            super::pending_target().is_none(),
            "the frame is no longer pending"
        );
        // Written back before its flip, it is still the frame the flip of its buffer shows.
        assert_eq!(
            super::frame_drawn_into(base + 0x1000),
            None,
            "another buffer"
        );
        super::set_pending(Some((target, ComponentSwap::Standard)));
        super::protect_target(target);
        assert!(super::written_at(base), "written back again");
        assert_eq!(super::frame_drawn_into(base), Some(base));
        assert_eq!(super::frame_drawn_into(base), None, "asked once a flip");
        let mut expected = super::words_of(&before);
        crate::tiling::tile_surface_64kb_rx_bpp4_mapped(&frame(), 16, 8, 0, &mut expected, |w| w)
            .expect("tiles");
        assert_eq!(
            memory,
            zerocopy::IntoBytes::as_bytes(expected.as_slice()),
            "memory holds the frame before the guest's write runs"
        );
        super::forget_written();
    }

    /// A guest touch of a pending target under a deferred fill lands on its frame, as a write to a
    /// protected one does: the fill is carried out and the frame written back over it before the
    /// access runs. Bugdom's menu writes into its cleared target between submissions; carrying out
    /// only the fill handed the next submission the clear, and the menu drew as its last layer
    /// alone.
    #[test]
    fn a_guest_touch_of_a_pending_target_under_a_deferred_fill_lands_on_its_frame() {
        use super::{
            ColourTarget, ComponentSwap, GuestCp, MappedRegions, TargetRead, cp::CpMemory,
        };
        use fake::frame;
        let _guard = serial();
        let span = crate::tiling::surface_words_64kb_rx_bpp4(16, 8) * 4;
        let page = super::HOST_PAGE as usize;
        let memory: &'static mut [u8] = Box::leak(vec![0xAAu8; span + page].into_boxed_slice());
        let aligned = (page - memory.as_ptr() as usize % page) % page;
        let memory = &mut memory[aligned..aligned + span];
        let base = memory.as_ptr() as usize as u64;
        let target = ColourTarget {
            base,
            width: 16,
            height: 8,
            pipe_bank_xor: 0,
            layout: crate::tiling::SurfaceLayout::Rx64Kb,
            place: crate::registers::Place::Whole,
        };
        allow_writes_to(base, span as u64);
        set_guest_regions(vec![region_of(memory)]);
        fake::install();
        super::install_frame_reader(|| Some(frame()));
        super::forget_written();
        let cp = &mut GuestCp {
            memory: MappedRegions::current(),
            submission: None,
        };
        assert!(cp.fill(base, 0x1234_5678, span));
        assert!(matches!(
            super::read_target(cp, target, ComponentSwap::Standard),
            Some(TargetRead::Uniform(_))
        ));
        super::set_pending(Some((target, ComponentSwap::Standard)));

        assert!(super::carry_out_at(base), "the fault is answered");
        assert!(
            super::pending_target().is_none(),
            "the frame is no longer pending"
        );
        let mut expected = vec![0x1234_5678u32; span / 4];
        crate::tiling::tile_surface_64kb_rx_bpp4_mapped(&frame(), 16, 8, 0, &mut expected, |w| w)
            .expect("tiles");
        assert_eq!(
            memory,
            zerocopy::IntoBytes::as_bytes(expected.as_slice()),
            "memory holds the frame over the fill before the guest's access runs"
        );
        super::forget_written();
    }

    /// Before a DCC target's draws, its blocks are left uncompressed under uncompressed keys: keys
    /// cleared to `0000` expand to a zero surface with every key `0xFF`, keys already uncompressed
    /// change nothing, and any other key state is refused with nothing written. A mip chain's
    /// blocks and keys are taken whole: a 256x128 chain of three levels is the tail's block and two
    /// levels' (256 KiB), under the tail's metadata block and one per level (12 KiB).
    #[test]
    fn a_dcc_target_is_expanded_before_its_draws_or_refused() {
        use super::{GuestCp, MappedRegions};
        use crate::registers::{ColourTargetDcc, ColourTargetExtent};
        let _guard = serial();
        for (width, height, levels, span, keys) in [
            (16, 8, 1, 0x1_0000, 0x1000),
            (256, 128, 3, 0x4_0000, 0x3000),
        ] {
            let buffer = off_page(span + keys + 0x2000, 0);
            let buffer_at = buffer.as_ptr() as usize as u64;
            allow_writes_to(buffer_at, buffer.len() as u64);
            set_guest_regions(vec![region_of(buffer)]);
            // The keys start on a 4 KiB metadata block, as addrlib aligns them.
            let meta = (buffer_at + span as u64 + 0xFFF) & !0xFFF;
            let target = ColourTargetDcc {
                dcc: crate::dcc::Dcc {
                    base: meta | 0x300,
                    pipe_aligned: true,
                },
                surface: buffer_at,
                extent: ColourTargetExtent { width, height },
                levels,
                single_slice: true,
                single_sample: true,
                clear: None,
                pipe_bank_xor: 0,
            };
            let at = |address: u64| usize::try_from(address - buffer_at).expect("inside");
            let mut cp = GuestCp {
                memory: MappedRegions::current(),
                submission: None,
            };

            buffer[..span + 0x10].fill(0xAB);
            buffer[at(meta)..at(meta) + keys].fill(0x00);
            assert_eq!(
                cp.expand_dcc(target, crate::tiling::SurfaceLayout::Rx64Kb),
                Ok(())
            );
            assert!(
                buffer[..span].iter().all(|&b| b == 0),
                "the cleared blocks' zeros, {width}x{height}"
            );
            assert_eq!(buffer[span], 0xAB, "nothing past the surface");
            assert!(
                buffer[at(meta)..at(meta) + keys].iter().all(|&b| b == 0xFF),
                "every key uncompressed"
            );

            buffer[..span].fill(0x5A);
            assert_eq!(
                cp.expand_dcc(target, crate::tiling::SurfaceLayout::Rx64Kb),
                Ok(())
            );
            assert!(buffer[..span].iter().all(|&b| b == 0x5A), "left as it is");

            buffer[at(meta) + keys - 1] = 0x20;
            let expanded = cp.expand_dcc(target, crate::tiling::SurfaceLayout::Rx64Kb);
            if levels == 1 {
                // The last key byte describes no block of a 16 x 8 surface (D873): its blocks'
                // keys say uncompressed, so nothing moves.
                assert_eq!(expanded, Ok(()));
            } else {
                // A chain's keys are not read block by block.
                assert!(expanded.is_err());
                assert_eq!(buffer[at(meta) + keys - 1], 0x20);
            }
            assert!(buffer[..span].iter().all(|&b| b == 0x5A), "nothing written");
            super::forget_written();
        }
    }

    /// A DCC target whose keys mix clears is expanded block by block (D873): each compress block
    /// by the key the GFX10 equation finds for it - zeros for a clear to `0000`, the clear colour
    /// for a clear to the register - and a compressed block is refused with nothing written.
    #[test]
    fn a_dcc_target_s_mixed_clears_are_expanded_block_by_block() {
        use super::{GuestCp, MappedRegions};
        use crate::registers::{ColourTargetDcc, ColourTargetExtent};
        let _guard = serial();
        let (span, keys) = (0x1_0000, 0x1000);
        let buffer = off_page(span + keys + 0x2000, 0);
        let buffer_at = buffer.as_ptr() as usize as u64;
        allow_writes_to(buffer_at, buffer.len() as u64);
        set_guest_regions(vec![region_of(buffer)]);
        let meta = (buffer_at + span as u64 + 0xFFF) & !0xFFF;
        let target = ColourTargetDcc {
            dcc: crate::dcc::Dcc {
                base: meta | 0x300,
                pipe_aligned: true,
            },
            surface: buffer_at,
            extent: ColourTargetExtent {
                width: 16,
                height: 8,
            },
            levels: 1,
            single_slice: true,
            single_sample: true,
            clear: Some([0x1122_3344, 0]),
            pipe_bank_xor: 0,
        };
        let at = |address: u64| usize::try_from(address - buffer_at).expect("inside");
        let layout = crate::tiling::SurfaceLayout::Rx64Kb;
        let equation = crate::dcc::key_equation(4, true).expect("four-byte texels");
        let key_of = |x, y| {
            at(meta)
                + usize::try_from(equation.key_offset((x, y), equation.pitch(16), 3))
                    .expect("small")
        };
        let block_of = |x, y| layout.compress_block_offset(x, y, 16).expect("placed") & !0xFF;
        let mut cp = GuestCp {
            memory: MappedRegions::current(),
            submission: None,
        };

        buffer[..span].fill(0xAB);
        buffer[at(meta)..at(meta) + keys].fill(0xFF);
        buffer[key_of(0, 0)] = 0x00;
        buffer[key_of(8, 0)] = 0x20;
        assert_eq!(cp.expand_dcc(target, layout), Ok(()));
        let (zeroed, coloured) = (block_of(0, 0), block_of(8, 0));
        assert!(
            buffer[zeroed..zeroed + 256].iter().all(|&b| b == 0),
            "cleared to 0000"
        );
        assert!(
            buffer[coloured..coloured + 256]
                .chunks(4)
                .all(|texel| texel == 0x1122_3344_u32.to_le_bytes()),
            "cleared to the register's colour"
        );
        let untouched = (0..span)
            .filter(|&byte| !(zeroed..zeroed + 256).contains(&byte))
            .filter(|&byte| !(coloured..coloured + 256).contains(&byte))
            .all(|byte| buffer[byte] == 0xAB);
        assert!(untouched, "nothing but the two blocks moved");
        assert!(buffer[at(meta)..at(meta) + keys].iter().all(|&b| b == 0xFF));

        buffer[..span].fill(0xAB);
        buffer[key_of(0, 0)] = 0x00;
        buffer[key_of(8, 0)] = 0x44;
        assert!(cp.expand_dcc(target, layout).is_err(), "a compressed block");
        assert!(buffer[..span].iter().all(|&b| b == 0xAB), "nothing written");
        super::forget_written();
    }

    /// A fill over the whole pending target drops the frame rather than writing it back (D719), and
    /// forgets what memory held, so the next read is a full one. No frame reader is installed, so a
    /// write-back would fail the fill.
    #[test]
    fn a_fill_over_the_whole_pending_target_drops_the_frame_unwritten() {
        use super::{ColourTarget, ComponentSwap, GuestCp, MappedRegions, Written, cp::CpMemory};
        use std::sync::Arc;
        let _guard = serial();
        let span = crate::tiling::surface_words_64kb_rx_bpp4(16, 8) * 4;
        let memory = off_page(span, 0);
        let base = memory.as_ptr() as usize as u64;
        allow_writes_to(base, span as u64);
        let target = ColourTarget {
            base,
            width: 16,
            height: 8,
            pipe_bank_xor: 0,
            layout: crate::tiling::SurfaceLayout::Rx64Kb,
            place: crate::registers::Place::Whole,
        };
        *super::last_written().lock().expect("slot") = Some(Written {
            target,
            contents: super::Contents::Bytes(Arc::new(memory.to_vec())),
            since: None,
        });
        super::set_pending(Some((target, ComponentSwap::Standard)));
        let mut cp = GuestCp {
            memory: MappedRegions::current(),
            submission: None,
        };
        assert!(
            cp.fill(base, 0x5566_7788, span),
            "filled, not written back first"
        );
        assert!(super::pending_target().is_none(), "the frame is dropped");
        assert!(
            super::last_written().lock().expect("slot").is_none(),
            "and the next read of the target is a full one"
        );
        assert!(
            memory
                .chunks_exact(4)
                .all(|w| w == 0x5566_7788u32.to_le_bytes())
        );
    }

    /// Asynchronously, a submit returns before its work runs: the label its last packet writes is
    /// not there until the command processor has carried the submission out, and is after (D821).
    /// The command processor is held off by holding the queue's stall slot, which carrying out a
    /// submission takes first.
    #[test]
    fn an_asynchronous_submission_writes_its_label_only_when_carried_out() {
        use crate::packet::build::command_header;
        let _guard = serial();
        let label = off_page(8, 0);
        let label_at = label.as_ptr() as usize as u64;
        allow_writes_to(label_at, 8);
        // WRITE_DATA to memory through L2, write-confirmed: the label, as a frame loop writes it.
        let words = [
            command_header(crate::cp::WRITE_DATA, 4),
            (2 << 8) | (1 << 20),
            label_at as u32,
            (label_at >> 32) as u32,
            0xfeed_f00d,
        ];
        let stream: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        set_guest_regions(vec![region_of(&stream), (label_at, label_at + 8)]);
        let desc = descriptor(&stream);

        super::set_asynchronous(true);
        let held = super::queue_stall()
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        assert_eq!(submit_dcb(&args(desc.as_ptr() as usize as u64)), SUBMIT_OK);
        assert!(
            !super::settle(std::time::Duration::from_millis(50)),
            "the submission waits"
        );
        assert_eq!(
            label[..4],
            [0; 4],
            "the submit returned before its work ran"
        );
        drop(held);
        let carried_out = super::settle(std::time::Duration::from_secs(5));
        super::set_asynchronous(false);
        assert!(carried_out, "the queue drains once the processor can run");
        assert_eq!(
            label[..4],
            0xfeed_f00d_u32.to_le_bytes(),
            "and the label is written"
        );
    }

    /// Asynchronously, a texture a queued submission draws into is read only once that submission
    /// has been carried out (D874): the hardware runs it before the next submission samples, so
    /// preparing the next waits for the queue to drain. A read of nothing queued does not wait.
    /// The command processor is held off by holding the queue's stall slot.
    #[test]
    fn a_read_while_preparing_waits_for_a_queued_submission_drawing_over_it() {
        use super::{ColourTarget, GuestMemory, MappedRegions, SettledReads, Submission};
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::time::Duration;
        let _guard = serial();
        let span = crate::tiling::surface_words_64kb_rx_bpp4(16, 8) * 4;
        let memory: &'static mut [u8] = Box::leak(vec![0u8; span * 2].into_boxed_slice());
        let base = memory.as_ptr() as usize as u64;
        set_guest_regions(vec![region_of(memory)]);
        let target = ColourTarget {
            base,
            width: 16,
            height: 8,
            pipe_bank_xor: 0,
            layout: crate::tiling::SurfaceLayout::Rx64Kb,
            place: crate::registers::Place::Whole,
        };

        super::set_asynchronous(true);
        let held = super::queue_stall()
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        super::command_processor::queue(
            Vec::new(),
            Submission {
                drawn_targets: vec![target],
                ..Submission::default()
            },
            Some(Vec::new()),
        );
        let regions = MappedRegions::current();
        let _ = SettledReads(&regions).read(base + span as u64, 4);
        assert!(
            !super::settle(Duration::ZERO),
            "a read past the target leaves the submission queued"
        );
        let returned = AtomicBool::new(false);
        let drained = std::thread::scope(|scope| {
            let reader = scope.spawn(|| {
                let _ = SettledReads(&regions).read(base, 4);
                returned.store(true, Ordering::SeqCst);
                super::settle(Duration::ZERO)
            });
            std::thread::sleep(Duration::from_millis(100));
            let early = returned.load(Ordering::SeqCst);
            drop(held);
            let drained = reader.join().expect("the reader returns");
            (early, drained)
        });
        let carried_out = super::settle(Duration::from_secs(5));
        super::set_asynchronous(false);
        assert!(carried_out, "the queue drains once the processor can run");
        assert_eq!(
            drained,
            (false, true),
            "the read over the target waited for the submission drawing it, and returned after it"
        );
    }

    /// Asynchronously, memory a queued submission writes other than by drawing - through its
    /// command processor's packets or its dispatches - is read only once it has been carried out
    /// (D876): a preparation that read it as what it carries forward is made again after the queue
    /// drains, and does not wait holding the pipeline (D882). A read past what it writes is made
    /// once; a submission whose writes are not bounded holds every read. The command processor is
    /// held off by holding the queue's stall slot.
    #[test]
    fn a_preparation_reading_what_a_queued_submission_writes_is_made_again_after_it() {
        use super::{GuestMemory, MappedRegions, SettledReads, Submission};
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::time::Duration;
        let _guard = serial();
        let memory: &'static mut [u8] = Box::leak(vec![0u8; 0x2000].into_boxed_slice());
        let base = memory.as_ptr() as usize as u64;
        set_guest_regions(vec![region_of(memory)]);
        let regions = MappedRegions::current();
        // How many times a preparation reading `address`, as what it carries forward, is made.
        let preparations = |address: u64| {
            let made = AtomicUsize::new(0);
            super::command_processor::preparing(|| {
                made.fetch_add(1, Ordering::SeqCst);
                super::command_processor::carrying(|| {
                    let _ = SettledReads(&regions).read(address, 4);
                });
            });
            made.into_inner()
        };
        // Queues a submission writing `writes`, held off; reads `address` while it is queued, then
        // lets it run. Answers whether the preparation returned while it was held, how many times
        // it was made, and whether the queue had drained when it returned.
        let read_while_queued = |writes: Option<Vec<(u64, u64)>>, address: u64| {
            let held = super::queue_stall()
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            super::command_processor::queue(Vec::new(), Submission::default(), writes);
            std::thread::scope(|scope| {
                let reader = scope.spawn(|| (preparations(address), super::settle(Duration::ZERO)));
                std::thread::sleep(Duration::from_millis(100));
                let early = reader.is_finished();
                drop(held);
                let (made, drained) = reader.join().expect("the reader returns");
                (early, made, drained)
            })
        };

        super::set_asynchronous(true);
        let past = {
            let held = super::queue_stall()
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            super::command_processor::queue(
                Vec::new(),
                Submission::default(),
                Some(vec![(base, 0x100)]),
            );
            let made = preparations(base + 0x100);
            let queued = !super::settle(Duration::ZERO);
            drop(held);
            (made, queued)
        };
        let past_drained = super::settle(Duration::from_secs(5));
        let over = read_while_queued(Some(vec![(base, 0x100)]), base + 0x80);
        let unbounded = read_while_queued(None, base + 0x1000);
        let carried_out = super::settle(Duration::from_secs(5));
        super::set_asynchronous(false);
        assert!(
            past_drained && carried_out,
            "the queue drains once the processor can run"
        );
        assert_eq!(
            past,
            (1, true),
            "a read past what it writes is made once, leaving it queued"
        );
        assert_eq!(
            over,
            (false, 2, true),
            "a read over what it writes is made again once it has been carried out"
        );
        assert_eq!(
            unbounded,
            (false, 2, true),
            "a submission whose writes are not bounded holds any read"
        );
    }

    /// Asynchronously, a preparation reading for its draws a texture a queued submission draws
    /// into, or a buffer a queued dispatch stores to, does not wait for it and is not made again
    /// (D882): it is answered as read ahead of them, and the command processor prepares its draws
    /// again as it carries it out, after them. A read past both is not read ahead. The command
    /// processor is held off by holding the queue's stall slot.
    #[test]
    fn a_preparation_sampling_what_a_queued_submission_draws_runs_on_and_is_drawn_again() {
        use super::{ColourTarget, GuestMemory, MappedRegions, SettledReads, Submission};
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::time::Duration;
        let _guard = serial();
        let span = crate::tiling::surface_words_64kb_rx_bpp4(16, 8) * 4;
        let memory: &'static mut [u8] = Box::leak(vec![0u8; span * 2].into_boxed_slice());
        let base = memory.as_ptr() as usize as u64;
        set_guest_regions(vec![region_of(memory)]);
        let target = ColourTarget {
            base,
            width: 16,
            height: 8,
            pipe_bank_xor: 0,
            layout: crate::tiling::SurfaceLayout::Rx64Kb,
            place: crate::registers::Place::Whole,
        };
        let regions = MappedRegions::current();
        // How many times a preparation reading `address` is made, and whether it read ahead.
        let preparations = |address: u64| {
            let made = AtomicUsize::new(0);
            let ((), ahead) = super::command_processor::preparing(|| {
                made.fetch_add(1, Ordering::SeqCst);
                let _ = SettledReads(&regions).read(address, 4);
            });
            (made.into_inner(), ahead)
        };

        super::set_asynchronous(true);
        let held = super::queue_stall()
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        super::command_processor::queue(
            Vec::new(),
            Submission {
                drawn_targets: vec![target],
                ..Submission::default()
            },
            Some(Vec::new()),
        );
        let stored = base + span as u64 + 0x100;
        super::command_processor::queue(
            Vec::new(),
            Submission::default(),
            Some(vec![(stored, 0x100)]),
        );
        let over = preparations(base);
        let over_stored = preparations(stored);
        let past = preparations(base + span as u64);
        let still_queued = !super::settle(Duration::ZERO);
        drop(held);
        let carried_out = super::settle(Duration::from_secs(5));
        super::set_asynchronous(false);
        assert!(carried_out, "the queue drains once the processor can run");
        assert!(
            still_queued,
            "neither preparation waited for the submission drawing the target"
        );
        assert_eq!(
            over,
            (1, true),
            "a read over the target is made once and answered as read ahead of its draws"
        );
        assert_eq!(
            over_stored,
            (1, true),
            "so is a read over what a queued dispatch stores to"
        );
        assert_eq!(past, (1, false), "a read past the target is not read ahead");
    }

    /// A descriptor whose command buffer is outside every region is refused, not dereferenced: the
    /// submit returns success and records an empty report.
    #[test]
    fn a_descriptor_outside_every_region_returns_ok_without_reading() {
        let _guard = serial();
        set_guest_regions(vec![(0x1000, 0x2000)]);
        let mut desc = [0u8; 16];
        desc[0..8].copy_from_slice(&0xdead_0000_u64.to_le_bytes()); // gpu_addr in no region
        desc[8..12].copy_from_slice(&4u32.to_le_bytes()); // 16 bytes
        assert_eq!(submit_dcb(&args(desc.as_ptr() as usize as u64)), SUBMIT_OK);
        let report = last_submission_report().expect("an empty report was recorded");
        assert_eq!(report.packets, 0, "nothing was read: {report:?}");
    }
}
