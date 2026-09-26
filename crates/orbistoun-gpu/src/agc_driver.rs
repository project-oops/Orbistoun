//! `libSceAgcDriver` - the submission side of the current generation's graphics interface.
//!
//! Separate from [`super::agc`] because the platform separates them: a guest builds command
//! buffers with `libSceAgc` and hands them over with `libSceAgcDriver`, two distinct libraries in
//! an import table. Names come from real import tables; arities are unestablished, as
//! [`super::agc`] states.

use crate::cp;
use crate::pipeline::{GuestMemory, Pipeline, Queue, Submission, SubmissionReport};
use crate::registers::{ColourTarget, ComponentSwap, SwizzleMode};
use crate::tiling;
use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};
use orbistoun_hle::guest_module;
use orbistoun_translate::{Fidelity, Strategy, Width};
use std::sync::{Arc, Mutex, OnceLock};

guest_module! {
    "libSceAgcDriver" {
        "sceAgcDriverAddEqEvent" => 6,
        "sceAgcDriverCreateQueue" => 3,
        "sceAgcDriverGetDefaultOwner" => 6,
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

/// `sceAgcDriverRegisterOwner(owner_buf)` - stub, returns `0x8a6c9018`, writes nothing.
fn register_owner(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    RESOURCE_REGISTRATION_NOT_SUPPORTED
}

/// `sceAgcDriverRegisterResource(res, ...)` - stub, returns `0x8a6c9018`, writes nothing.
fn register_resource(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    RESOURCE_REGISTRATION_NOT_SUPPORTED
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

/// Record the regions a submit may read from - the allocated regions of the guest's memory map.
///
/// The worker calls this before entering the guest, from the map it records as `memory_map` in the
/// run conditions. A shader address inside a region is read and counted resolved; one outside
/// reads `None` and is counted unresolved (D130). Neither faults the host. Empty (the default),
/// nothing resolves.
pub fn set_guest_regions(regions: Vec<(u64, u64)>) {
    if let Ok(mut slot) = guest_regions().lock() {
        *slot = regions;
    }
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

/// The one colour target a submission's draws can be carried out into and written back to exactly:
/// a single base, a single resident target of the same extent, `64KB_R_X` tiling with its measured
/// whole-surface layout, and an `8_8_8_8` `UNORM` element in an order `CB_COLOR0_INFO` names.
/// Anything else is `None`, and the draws stay unexecuted.
fn writable_target(submission: &Submission) -> Option<(ColourTarget, ComponentSwap)> {
    let target = submission.colour_target?;
    let format = submission.colour_target_format?;
    let one_target = submission.colour_target_bases == 1
        && submission.targets.len() == 1
        && submission
            .targets
            .values()
            .all(|extent| (extent.width, extent.height) == (target.width, target.height));
    (one_target
        && submission.colour_target_tiling == Some(SwizzleMode::Tiled64KbRX)
        && format.is_rgba8_class())
    .then_some((target, format.swap))
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

impl GuestCp<'_> {
    /// Reads the target, has the executor draw over it, and - after every submission, or at the
    /// flip - writes the result back where the guest reads it (D714). `false`, with nothing
    /// written, at the first thing that is not exact.
    fn draw_into_target(&mut self) -> bool {
        let (Some(execute), Some(read)) = (draw_executor().get(), frame_reader().get()) else {
            return false;
        };
        let Some(submission) = self.submission else {
            return false;
        };
        let Some((target, swap)) = writable_target(submission) else {
            return false;
        };
        // The executor holds one frame per extent, so a frame still pending for another target is
        // written back before this one is drawn over it (D714).
        if pending_target().is_some_and(|(pending, _)| pending.base != target.base)
            && !write_back_pending(self)
        {
            return false;
        }
        if !write_back_at_flip() {
            return draw_over(self, target, swap, |before| {
                execute(submission, before).then(read).flatten()
            });
        }
        // Drawn and left on the device: the frame is written back when the guest flips, not after
        // each of the many submissions a GL frame takes (D714).
        let Some(before) = read_target(self, target, swap) else {
            return false;
        };
        if !execute(submission, before.before()) {
            forget_written();
            set_pending(None);
            return false;
        }
        set_pending(Some((target, swap)));
        true
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

fn set_pending(target: Option<(ColourTarget, ComponentSwap)>) {
    *pending()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = target;
}

/// Writes the pending frame, if there is one, into guest memory (D714). `true` when there was none
/// or it was written; `false` when it could not be, which drops it rather than leaving it pending
/// forever.
fn write_back_pending(memory: &mut dyn cp::CpMemory) -> bool {
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
    write_target(memory, target, swap, &after)
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
    let hooks = lazy_copies().get().ok_or(CannotWait)?;
    let (target, swap) = pending_target().ok_or(CannotWait)?;
    let span = tiling::surface_words_64kb_rx_bpp4(target.width, target.height) as u64 * 4;
    let memory_then = match last_written().lock().ok().and_then(|last| last.clone()) {
        Some(written) if written.base == target.base && written.bytes.len() as u64 == span => {
            written.bytes
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
        let old = list.remove(index);
        crate::perf::span(crate::perf::Span::DiscardSnapshot, || {
            (hooks.discard)(old.snapshot);
        });
        old.pages.2
    });
    let (touched, kept): (Vec<Deferred>, Vec<Deferred>) = std::mem::take(&mut *list)
        .into_iter()
        .partition(|copy| ranges_overlap((copy.pages.0, copy.pages.1), pages));
    *list = kept;
    let carried = !touched.is_empty();
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
    let Some(protection) = guard.or_else(|| (hooks.guard)(pages.0, pages.1)) else {
        (hooks.discard)(snapshot);
        return Err(CannotWait);
    };
    list.push(Deferred {
        destination: target.base,
        length: span,
        pages: (pages.0, pages.1, protection),
        snapshot,
        target,
        swap,
        memory_then,
    });
    set_pending(None);
    // The display's own copy, beside memory's: released by whoever shows it.
    let shown = show
        .then(|| crate::perf::span(crate::perf::Span::KeepSnapshot, hooks.snapshot))
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
    let span = tiling::surface_words_64kb_rx_bpp4(target.width, target.height) as u64 * 4;
    if target.base >= address && target.base.saturating_add(span) <= address.saturating_add(length)
    {
        set_pending(None);
        forget_written();
    }
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
        (hooks.discard)(copy.snapshot);
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
    let (width, height) = (target.width, target.height);
    let read_started = std::time::Instant::now();
    let length = tiling::surface_words_64kb_rx_bpp4(width, height) * 4;
    // Unchanged is asked of the host first: whether any page of the target has been written since
    // this last wrote or read it. Only where the host cannot say, or says it was, are the bytes
    // compared - a write of the same bytes is still unchanged. Read-only and unwritten (D720) is
    // asked first, since write-watch cannot answer for guest direct memory's mapped views. The
    // pages are protected again before they are compared or read, so a racing write is seen next
    // time rather than lost.
    let was_protected = protected_unwritten(target);
    protect_target(target);
    let unchanged = last_written().lock().is_ok_and(|last| {
        last.as_ref().is_some_and(|written| {
            written.base == target.base
                && written.bytes.len() == length
                && (was_protected
                    || written.since.and_then(|since| {
                        orbistoun_mem::watch::written_since(target.base, length as u64, since)
                    }) == Some(false)
                    || memory.holds(target.base, &written.bytes))
        })
    });
    if unchanged {
        crate::perf::add(crate::perf::Phase::ReadTarget, read_started.elapsed());
        return Some(TargetRead::Unchanged);
    }
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
        Some(word) => TargetRead::Uniform(swapped(word, swap)),
        None => TargetRead::Changed(tiling::detile_surface_64kb_rx_bpp4_mapped(
            &words_of(&bytes),
            width,
            height,
            |w| swapped(w, swap),
        )),
    };
    // What the drawer is handed is what memory holds now: with the frame kept on the device until
    // the flip, memory still holding these bytes means nothing else has written the target since
    // (D714).
    if let Ok(mut last) = last_written().lock() {
        *last = Some(Written {
            base: target.base,
            bytes: Arc::new(bytes),
            since,
        });
    }
    crate::perf::add(crate::perf::Phase::ReadTarget, read_started.elapsed());
    Some(before)
}

/// What reading a colour target found.
#[derive(Debug)]
enum TargetRead {
    /// Memory holds exactly what this last wrote or read there: the drawer already has it.
    Unchanged,
    /// Something else wrote it: its contents, as linear `Rgba8`.
    Changed(Vec<u32>),
    /// Something else wrote it with one word throughout - a clear: that word as linear `Rgba8`.
    Uniform(u32),
}

impl TargetRead {
    /// What a drawer is handed of it.
    fn before(&self) -> Before<'_> {
        match self {
            Self::Unchanged => Before::Held,
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
    let (width, height) = (target.width, target.height);
    crate::perf::measure(crate::perf::Phase::WriteTarget, || {
        // Tiled straight into the target, in place, rather than read out, converted and written
        // again.
        let words = tiling::surface_words_64kb_rx_bpp4(width, height);
        let mut tiled_ok = false;
        let mut written = Vec::new();
        let edited = memory.edit_words(target.base, words, &mut |tiled| {
            tiled_ok = tiling::tile_surface_64kb_rx_bpp4_mapped(after, width, height, tiled, |w| {
                swapped(w, swap)
            })
            .is_ok();
            // What memory holds now, for the next submission's unchanged check.
            written = zerocopy::IntoBytes::as_bytes(&*tiled).to_vec();
        });
        let wrote = edited && tiled_ok;
        // Marked after this write, so the next check does not see this write as someone else's.
        let since = orbistoun_mem::watch::mark(target.base, words as u64 * 4);
        if let Ok(mut last) = last_written().lock() {
            *last = wrote.then(|| Written {
                base: target.base,
                bytes: Arc::new(written),
                since,
            });
        }
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

/// A target's base, the bytes its memory held when this last wrote or read it, and the host's mark
/// of that moment when it keeps one.
#[derive(Debug, Clone)]
struct Written {
    base: u64,
    bytes: Arc<Vec<u8>>,
    since: Option<u64>,
}

/// Forgets [`last_written`], so the next submission hands its drawer the target as it is.
fn forget_written() {
    if let Ok(mut last) = last_written().lock() {
        *last = None;
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
    let span = tiling::surface_words_64kb_rx_bpp4(target.width, target.height) as u64 * 4;
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
    }
}

/// Whether `target` has been read-only, unwritten, since it was last written or read (D720).
fn protected_unwritten(target: ColourTarget) -> bool {
    write_protected()
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
pub fn written_at(address: u64) -> bool {
    let Some(hooks) = lazy_copies().get() else {
        return false;
    };
    let Ok(mut protected) = write_protected().lock() else {
        return false;
    };
    match *protected {
        Some(p) if ranges_overlap(p.pages, (address, 1)) => {
            release_protection(p, hooks);
            *protected = None;
            true
        }
        _ => false,
    }
}

/// Writes `bytes` into guest memory at `address` - `false`, writing nothing, when the range is not
/// writable guest memory.
fn write_guest(address: u64, bytes: &[u8]) -> bool {
    let writable = write_lookup()
        .get()
        .is_some_and(|allows| allows(address, bytes.len() as u64));
    let Ok(dest) = usize::try_from(address) else {
        return false;
    };
    if !writable {
        return false;
    }
    unprotect_overlapping(address, bytes.len() as u64);
    // SAFETY: the installed lookup vouched that `[address, address + len)` lies wholly inside one
    // identity-mapped guest mapping whose protection allows writes: `len` writable bytes of this
    // process. A deferred copy's destination and a protected target (D720) have their host
    // protection restored above, so nothing here faults.
    unsafe {
        std::ptr::copy_nonoverlapping(
            bytes.as_ptr(),
            std::ptr::with_exposed_provenance_mut::<u8>(dest),
            bytes.len(),
        );
    }
    true
}

/// Whether `[address, address + length)` touches any byte of `target`'s tiled surface.
fn overlaps_target(target: ColourTarget, address: u64, length: u64) -> bool {
    let span = tiling::surface_words_64kb_rx_bpp4(target.width, target.height) as u64 * 4;
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
    /// The kept frame's pixels as linear `Rgba8` words, releasing it.
    pub take: fn(u64) -> Option<Vec<u32>>,
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

/// A copy of a colour target into guest memory that has not been carried out yet.
struct Deferred {
    /// The copy's destination and length.
    destination: u64,
    length: u64,
    /// The guarded host pages covering it, and the protection they had.
    pages: (u64, u64, u32),
    /// The frame as the copy saw it, on the device.
    snapshot: u64,
    target: ColourTarget,
    swap: ComponentSwap,
    /// The target's memory as the copy saw it: every word the frame does not cover - a partial edge
    /// block's padding - comes from here.
    memory_then: Arc<Vec<u8>>,
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
        (hooks.discard)(copy.snapshot);
        return false;
    }
    if let Ok(mut resolved) = resolved().lock() {
        if resolved.len() >= RESOLVED_KEPT {
            resolved.remove(0);
        }
        resolved.push((base, len));
    }
    let Some(frame) = (hooks.take)(copy.snapshot) else {
        tracing::warn!(
            "a deferred copy's frame could not be read - {:#x} keeps what it held",
            copy.destination
        );
        return false;
    };
    let (width, height) = (copy.target.width, copy.target.height);
    let mut tiled = words_of(&copy.memory_then);
    if tiling::tile_surface_64kb_rx_bpp4_mapped(&frame, width, height, &mut tiled, |w| {
        swapped(w, copy.swap)
    })
    .is_err()
    {
        return false;
    }
    let bytes = zerocopy::IntoBytes::as_bytes(tiled.as_slice());
    let Some(bytes) = usize::try_from(copy.length)
        .ok()
        .and_then(|length| bytes.get(..length))
    else {
        return false;
    };
    write_guest(copy.destination, bytes)
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
    {
        let Ok(mut list) = deferred().lock() else {
            return false;
        };
        let (touched, kept): (Vec<Deferred>, Vec<Deferred>) = std::mem::take(&mut *list)
            .into_iter()
            .partition(|copy| ranges_overlap((copy.pages.0, copy.pages.1), (address, 1)));
        *list = kept;
        if !touched.is_empty() {
            return carry_out_all(&touched, hooks);
        }
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
        let Some(hooks) = lazy_copies().get() else {
            return false;
        };
        let Some((target, swap)) = pending_target() else {
            return false;
        };
        let span = tiling::surface_words_64kb_rx_bpp4(target.width, target.height) as u64 * 4;
        let length = count as u64;
        if source != target.base || length != span || overlaps_target(target, destination, length) {
            return false;
        }
        // The target's memory as this copy sees it: what the draws started from, unchanged since -
        // any memory work that touched it would have written the frame back and left nothing
        // pending.
        let memory_then = match last_written().lock().ok().and_then(|last| last.clone()) {
            Some(written) if written.base == target.base && written.bytes.len() as u64 == span => {
                written.bytes
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
                (Some(old.pages.2), Some(old.snapshot))
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
        let Some(protection) = guard.or_else(|| (hooks.guard)(pages.0, pages.1)) else {
            (hooks.discard)(snapshot);
            return false;
        };
        list.push(Deferred {
            destination,
            length,
            pages: (pages.0, pages.1, protection),
            snapshot,
            target,
            swap,
            memory_then,
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
                write_back_pending(self)
            }
            _ => true,
        }
    }
}

impl cp::CpMemory for GuestCp<'_> {
    fn run_draws(&mut self) -> bool {
        crate::perf::span(crate::perf::Span::RunDraws, || self.draw_into_target())
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
            drop_covered(address, count as u64);
            drop_pending_covered(address, count as u64);
        }
        if !carry_out_overlapping(address, count as u64)
            || !self.settle_pending(address, count as u64)
        {
            return false;
        }
        let writable = write_lookup()
            .get()
            .is_some_and(|allows| allows(address, count as u64));
        let Ok(dest) = usize::try_from(address) else {
            return false;
        };
        if !writable {
            return false;
        }
        unprotect_overlapping(address, count as u64);
        // SAFETY: the installed lookup vouched that `[address, address + count)` lies inside
        // identity- mapped guest mappings whose protection allows writes, and a protected target's
        // pages among them were given their protection back above: `count` writable bytes of this
        // process. The guest thread that owns them is blocked in this submit while they are filled.
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
        // The source is read now, so what is deferred into it is carried out first. The destination
        // is left to `defer_copy`, which drops a deferred copy this one overwrites completely.
        if !carry_out_overlapping(source, count as u64) {
            return false;
        }
        if Self::defer_copy(source, destination, count) {
            return true;
        }
        if !carry_out_overlapping(destination, count as u64)
            || !self.settle_pending(source, count as u64)
            || !self.settle_pending(destination, count as u64)
        {
            return false;
        }
        let Some(from) = self.memory.read(source, count) else {
            return false;
        };
        let writable = write_lookup()
            .get()
            .is_some_and(|allows| allows(destination, count as u64));
        let Ok(dest) = usize::try_from(destination) else {
            return false;
        };
        if !writable {
            return false;
        }
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
    /// The most recent submission's execution.
    pub last: Option<cp::CpExecution>,
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

/// Translated modules whose submission was not drawn, carried into the next one.
fn undelivered_modules() -> &'static Mutex<std::collections::BTreeMap<crate::ResourceId, Vec<u32>>>
{
    static PENDING: OnceLock<Mutex<std::collections::BTreeMap<crate::ResourceId, Vec<u32>>>> =
        OnceLock::new();
    PENDING.get_or_init(Default::default)
}

/// Reads one submit descriptor, walks the command buffer it names, and keeps the submission - the
/// body both submit entry points share.
fn submit_described(descriptor: u64) -> u64 {
    crate::perf::span(crate::perf::Span::SubmitCall, || {
        crate::perf::measure(crate::perf::Phase::Submit, || {
            submit_described_timed(descriptor)
        })
    })
}

/// [`submit_described`]'s body, timed whole as [`crate::perf::Phase::Submit`].
fn submit_described_timed(descriptor: u64) -> u64 {
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
    // One pipeline for the run, not one per submission: the pipeline caches translated shaders.
    let submission = crate::perf::measure(crate::perf::Phase::Prepare, || {
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
            .map(|pipeline| pipeline.placing_window_from_shaders().feeding_user_data());
        }
        live.as_mut()
            .map(|pipeline| pipeline.submit(&bytes, Queue::Draw, &[], &memory))
            .unwrap_or_default()
    });
    // A module travels once, with the first submission to use it, so one whose draws never reached
    // the backend is carried forward until a submission is drawn; re-sending one the backend has is
    // a no-op.
    let mut submission = submission;
    if let Ok(mut pending) = undelivered_modules().lock() {
        for (id, module) in std::mem::take(&mut *pending) {
            submission.modules.entry(id).or_insert(module);
        }
    }
    let submission = submission;
    // The command processor's own memory work, carried out synchronously as the submit returns. It
    // stops at the first packet needing the GPU, so a fence is written only when everything before
    // it ran (D705); draws are such work once an executor is installed and they can run as one into
    // a target they can be written back to.
    let executed = crate::perf::span(crate::perf::Span::CommandProcessor, || {
        cp::execute(
            &bytes,
            &mut GuestCp {
                memory,
                submission: Some(&submission),
            },
        )
    });
    if executed.draws == 0
        && let Ok(mut pending) = undelivered_modules().lock()
    {
        pending.extend(submission.modules.iter().map(|(id, m)| (*id, m.clone())));
    }
    if let Ok(mut record) = execution_record().lock() {
        record.submissions += 1;
        record.completed += u64::from(executed.stopped == cp::Stopped::Completed);
        record.drawn += u64::from(executed.draws > 0);
        record.bytes_written += executed.bytes_written;
        record.last = Some(executed);
    }
    if let Ok(mut slot) = last_submission().lock() {
        *slot = Some(submission);
    }
    SUBMIT_OK
}

/// Implementations this crate provides for `libSceAgcDriver`.
///
/// `sceAgcDriverCreateQueue` accepts the queue; the resource-registration family returns the
/// hardware's measured `0x8a6c9018`.
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("sceAgcDriverCreateQueue", create_queue),
        ("sceAgcDriverRegisterOwner", register_owner),
        ("sceAgcDriverRegisterResource", register_resource),
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
    use std::sync::{Mutex, PoisonError};

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
        };
        let expected = crate::tiling::detile_surface_64kb_rx_bpp4_mapped(
            &words_of(&memory.0),
            width,
            height,
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
        let mut memory = Block(vec![0x5a; 65536]);
        let target = ColourTarget {
            base: BASE + 0x1_0000,
            width: 16,
            height: 8,
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
    /// `64KB_R_X`, `8_8_8_8` `UNORM`; a second base, linear tiling or no format each refuses.
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
        assert!(writable_target(&submission).is_some());
        let refused = |change: fn(&mut Submission)| {
            let mut changed = submission.clone();
            change(&mut changed);
            writable_target(&changed).is_none()
        };
        assert!(refused(|s| s.colour_target_bases = 2));
        assert!(refused(
            |s| s.colour_target_tiling = Some(SwizzleMode::Linear)
        ));
        assert!(refused(|s| s.colour_target_format = None));
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

    /// These tests share the process-global region and last-submission stores, so they run one at a
    /// time. A poisoned lock is recovered rather than cascading a panic across the others.
    fn serial() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: Mutex<()> = Mutex::new(());
        LOCK.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The `(start, end)` region a heap buffer occupies, its address exposed for the handler's
    /// read.
    fn region_of(buffer: &[u8]) -> (u64, u64) {
        let base = buffer.as_ptr() as usize as u64;
        (base, base + buffer.len() as u64)
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

    /// The lazy-copy hooks every test here shares (the first install wins): snapshots and
    /// protection that are counted, not real. No page's protection changes, so a test drives the
    /// fault path by calling what the handler would.
    mod fake {
        use std::sync::atomic::{AtomicU64, Ordering};

        pub(super) static SNAPSHOTS: AtomicU64 = AtomicU64::new(0);
        pub(super) static TAKEN: AtomicU64 = AtomicU64::new(0);
        pub(super) static DISCARDED: AtomicU64 = AtomicU64::new(0);
        pub(super) static PROTECTED: AtomicU64 = AtomicU64::new(0);

        /// The frame every snapshot holds: a 16x8 ramp.
        pub(super) fn frame() -> Vec<u32> {
            (0..128u32).map(|i| 0xff00_0000 | (i * 0x0101)).collect()
        }

        pub(super) fn install() {
            super::super::install_lazy_copies(super::super::LazyCopies {
                snapshot: || Some(SNAPSHOTS.fetch_add(1, Ordering::SeqCst)),
                take: |_| {
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
                guard: |_, _| Some(0),
                protect_writes: |_, _| {
                    PROTECTED.fetch_add(1, Ordering::SeqCst);
                    Some(4)
                },
                release: |_, _, _| true,
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
        };
        *super::last_written().lock().expect("slot") = Some(Written {
            base,
            bytes: Arc::new(memory.to_vec()),
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
        };
        let span = crate::tiling::surface_words_64kb_rx_bpp4(16, 8) * 4;
        let memory: &'static mut [u8] = Box::leak(vec![0xAAu8; span].into_boxed_slice());
        let base = memory.as_ptr() as usize as u64;
        let target = ColourTarget { base, ..target };
        allow_writes_to(base, span as u64);
        fake::install();
        let pend = |memory: &[u8]| {
            *super::last_written().lock().expect("slot") = Some(Written {
                base,
                bytes: Arc::new(memory.to_vec()),
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
        crate::tiling::tile_surface_64kb_rx_bpp4_mapped(&frame(), 16, 8, &mut expected, |w| w)
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

    /// A fill over the whole pending target drops the frame rather than writing it back (D719), and
    /// forgets what memory held, so the next read is a full one. No frame reader is installed, so a
    /// write-back would fail the fill.
    #[test]
    fn a_fill_over_the_whole_pending_target_drops_the_frame_unwritten() {
        use super::{ColourTarget, ComponentSwap, GuestCp, MappedRegions, Written, cp::CpMemory};
        use std::sync::Arc;
        let _guard = serial();
        let span = crate::tiling::surface_words_64kb_rx_bpp4(16, 8) * 4;
        let memory: &'static mut [u8] = Box::leak(vec![0u8; span].into_boxed_slice());
        let base = memory.as_ptr() as usize as u64;
        allow_writes_to(base, span as u64);
        let target = ColourTarget {
            base,
            width: 16,
            height: 8,
        };
        *super::last_written().lock().expect("slot") = Some(Written {
            base,
            bytes: Arc::new(memory.to_vec()),
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
