//! Images a dispatch stores to, kept on the device with their write-back deferred (D990).
//!
//! An image the device keeps (`crate::resident`) that a dispatch stores to is not read back after
//! the dispatch. Its write-back is deferred over its own guarded pages, as a colour attachment's
//! is (D948, D719): the first touch of them - by the guest, by the command processor, or by a
//! preparation reading them - reads the image off the device and tiles it over memory as memory is
//! then, before the touch goes on. Until something touches them, the next dispatch binding the
//! image takes it from the device as the last left it, and nothing is read back or written between.

use super::{
    Deferred, Held, MappedRegions, carry_out_all, carry_out_overlapping, changed_runs, deferred,
    guard_pages, lazy_copies, overlaps_target, page_span, pending_target, ranges_overlap,
    unprotect_overlapping, write_guest, write_lookup,
};
use crate::pipeline::{DispatchImages, DispatchSurface, GuestMemory, StoredImage};

/// An image the device keeps whose write-back is deferred: its key and the generation of its
/// contents, and where it lies and the order its channels are in there.
type KeptImage = ((u64, u64), (DispatchSurface, Option<[u8; 4]>));

/// Reads an image the device keeps under a key, at a generation of its contents or a later one,
/// as linear texels in the shader's channel order: what a deferred write-back writes. Installed by
/// the worker, which runs it on the device's thread.
pub type KeptImageReader = fn(u64, u64) -> Option<Vec<u32>>;

fn reader() -> &'static std::sync::OnceLock<KeptImageReader> {
    static READER: std::sync::OnceLock<KeptImageReader> = std::sync::OnceLock::new();
    &READER
}

/// Installs what reads a kept image off the device. First install wins; without one, no
/// write-back is deferred.
pub fn install_kept_image_reader(read: KeptImageReader) {
    let _ = reader().set(read);
}

/// Where `image` lies, where its write-back can be deferred: an image of guest memory, of whole
/// host pages, no compression keys to mark, that the command processor may write and the device
/// does not map, and over no frame still pending on the device.
fn deferrable_span(image: &StoredImage) -> Option<(u64, u64)> {
    let (surface, _) = image.surface.as_ref()?;
    if image.keys_to_expand.is_some()
        || image.depth.is_some()
        || image.stencil.is_some()
        || reader().get().is_none()
        || lazy_copies().get().is_none()
    {
        return None;
    }
    let (base, length) = (surface.base(), surface.bytes() as u64);
    let pages = page_span(base, length);
    let whole = pages == (base, length);
    (whole
        && write_lookup()
            .get()
            .is_some_and(|allows| allows(base, length))
        && !orbistoun_mem::watch::guards_kept_off(base, length)
        && !pending_target().is_some_and(|(target, _)| overlaps_target(target, base, length)))
    .then_some((base, length))
}

/// Counts why a stored image's write-back is made at once, where it is: what deferring more of them
/// would take is read off these.
fn note_why_not(image: &StoredImage, used: Option<crate::resident::Use>) {
    use crate::perf::{Span, span};
    if used.is_some_and(|used| used.deferred) || image.surface.is_none() {
        return;
    }
    let counted = match image.surface.as_ref() {
        _ if used.is_none() => Span::StoredUnkept,
        Some((surface, _))
            if orbistoun_mem::watch::guards_kept_off(surface.base(), surface.bytes() as u64) =>
        {
            Span::StoredMapped
        }
        Some((surface, _))
            if pending_target().is_some_and(|(target, _)| {
                overlaps_target(target, surface.base(), surface.bytes() as u64)
            }) =>
        {
            Span::StoredPending
        }
        _ => Span::StoredOther,
    };
    span(counted, || ());
}

/// Marks each image `images` stores to that the device keeps, and whose write-back can wait, as
/// deferred: the device does not read it back after the dispatch. Not for a dispatch addressing
/// guest memory directly (D964), whose stores may reach the same pages.
pub(super) fn defer_where_possible(images: &mut DispatchImages, device_addressed: bool) {
    for (image, used) in images.stored.iter().zip(&mut images.resident.stored) {
        if let Some(used) = used
            && !used.deferred
            && !device_addressed
            && deferrable_span(image).is_some()
        {
            used.deferred = true;
        }
        note_why_not(image, *used);
    }
}

/// After a dispatch ran: each image it stored to with its write-back deferred is left on the device
/// over its guarded pages, in place of a write-back deferred there before; one that cannot wait
/// after all is read off the device and written back now. `false` where one of those could not be.
pub(super) fn defer_stored(images: &DispatchImages) -> bool {
    let mut ok = true;
    for (image, used) in images.stored.iter().zip(&images.resident.stored) {
        let Some(used) = used.filter(|used| used.deferred) else {
            continue;
        };
        let Some((surface, _)) = image.surface.as_ref() else {
            continue;
        };
        let held = ((used.key, used.generation), (*surface, image.order));
        if deferrable_span(image).is_some() && defer(held) {
            crate::perf::span(crate::perf::Span::ResidentDeferred, || ());
            continue;
        }
        // Waiting is not possible now: the device's texels go into memory at once, after whatever
        // else is deferred over its pages, which gives them back.
        let (base, length) = (surface.base(), surface.bytes() as u64);
        let written = carry_out_overlapping(base, length) && write_back(held);
        ok &= written;
    }
    ok
}

/// Leaves the image the device keeps under `key` at `generation`, lying as `surface`, on the device
/// with its write-back deferred over its guarded pages. `false` where it cannot wait.
fn defer(((key, generation), (surface, order)): KeptImage) -> bool {
    let Some(hooks) = lazy_copies().get() else {
        return false;
    };
    let (base, length) = (surface.base(), surface.bytes() as u64);
    let Ok(mut list) = deferred().lock() else {
        return false;
    };
    let held = Held::DispatchImage {
        key,
        generation,
        surface,
        order,
    };
    // The same image's write-back deferred already, its pages guarded still: the device holds what
    // it stored since, which this write-back writes in its place.
    if let Some(same) = list.iter_mut().find(|copy| {
        copy.destination == base
            && copy.length == length
            && matches!(copy.held, Held::DispatchImage { key: kept, .. } if kept == key)
    }) {
        crate::perf::span(crate::perf::Span::DeferredSuperseded, || ());
        same.held = held;
        orbistoun_mem::watch::owed_to(base, length);
        return true;
    }
    // Guarded another way now: a protection kept would be saved as the one to restore (D720).
    unprotect_overlapping(base, length);
    let (touched, kept): (Vec<Deferred>, Vec<Deferred>) = std::mem::take(&mut *list)
        .into_iter()
        .partition(|copy| ranges_overlap((copy.pages.0, copy.pages.1), (base, length)));
    *list = kept;
    if !carry_out_all(&touched, hooks) {
        return false;
    }
    let Some(protection) = crate::perf::span(crate::perf::Span::CopyGuard, || {
        guard_pages(hooks, base, length)
    }) else {
        return false;
    };
    list.push(Deferred {
        destination: base,
        length,
        pages: (base, length, protection),
        held,
    });
    // Until it is written back, every cache asking whether its bytes changed is told they did, so
    // reads them again - which writes it back first.
    orbistoun_mem::watch::owed_to(base, length);
    true
}

/// Writes the image the device keeps under `key`, at `generation` or a later one, into memory over
/// `surface` - tiled over what memory holds there now, only the bytes that change - and tells the
/// images kept that guest memory holds it. Its pages are writable: a deferred write-back's are given
/// back before this runs. `false` where it could not be read off the device or written.
pub(super) fn write_back(((key, generation), (surface, order)): KeptImage) -> bool {
    let written = crate::perf::span(crate::perf::Span::ResidentWrittenBack, || {
        write_back_now(((key, generation), (surface, order)))
    });
    // Owed no longer: what it is now, written back or left as it was where it could not be, is
    // what memory holds.
    orbistoun_mem::watch::settled(surface.base());
    if written {
        crate::resident::with_registry(|registry| {
            registry.carried_out(&crate::resident::HostWatch, (key, generation));
        });
    }
    written
}

/// [`write_back`]'s reading off the device and writing.
fn write_back_now(((key, generation), (surface, order)): KeptImage) -> bool {
    let Some(read) = reader().get() else {
        return false;
    };
    let Some(mut texels) =
        crate::perf::span(crate::perf::Span::CarryOutTake, || read(key, generation))
    else {
        tracing::warn!(
            "an image a dispatch stored to, kept on the device, could not be read - {:#x} keeps what it held",
            surface.base()
        );
        return false;
    };
    // Texels a descriptor reorders are in the shader's order; memory keeps its own.
    if let Some(order) = order {
        for texel in &mut texels {
            *texel = crate::pipeline::to_memory_channels(*texel, order);
        }
    }
    let (base, length) = (surface.base(), surface.bytes());
    let regions = MappedRegions::current();
    let Some(before) = regions.read(base, length).map(<[u8]>::to_vec) else {
        return false;
    };
    let mut after = before.clone();
    let tiled = crate::perf::span(crate::perf::Span::CarryOutTile, || {
        surface.tile(&texels, &mut after)
    });
    if tiled.is_err() {
        return false;
    }
    crate::perf::span(crate::perf::Span::CarryOutWrite, || {
        changed_runs(&before, &after)
            .into_iter()
            .all(|(first, bytes)| write_guest(base + first as u64, &after[first..first + bytes]))
    })
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::super::tests::{allow_writes_to, fake, region_of, serial};
    use super::super::{ColourTarget, GuestCp, Held, MappedRegions, deferred, set_guest_regions};
    use super::{defer, install_kept_image_reader};
    use crate::pipeline::DispatchSurface;

    static READ: AtomicU64 = AtomicU64::new(0);

    /// What the device holds of the image: every texel the same.
    #[allow(
        clippy::unnecessary_wraps,
        reason = "a kept image reader's own signature"
    )]
    fn read_kept(_: u64, _: u64) -> Option<Vec<u32>> {
        READ.fetch_add(1, Ordering::SeqCst);
        Some(vec![0xa1b2_c3d4; 16 * 8])
    }

    /// An image a dispatch stored to, kept on the device, is left there with its write-back
    /// deferred: its bytes answer written to every cache meanwhile, a later dispatch's stores
    /// supersede it without guarding its pages again, and the first read of its memory reads it
    /// off the device once and tiles it into memory, after which nothing is owed (D990).
    #[test]
    fn a_stored_image_stays_on_the_device_until_its_memory_is_read() {
        let _guard = serial();
        fake::install();
        install_kept_image_reader(read_kept);
        let span = crate::tiling::surface_words_64kb_rx_bpp4(16, 8) * 4;
        let memory: &'static mut [u8] = Box::leak(vec![0x10u8; span].into_boxed_slice());
        let base = memory.as_ptr() as usize as u64;
        allow_writes_to(base, span as u64);
        set_guest_regions(vec![region_of(memory)]);
        let surface = DispatchSurface::Tiled(ColourTarget {
            base,
            width: 16,
            height: 8,
            pipe_bank_xor: 0,
            layout: crate::tiling::SurfaceLayout::Rx64Kb,
            place: crate::registers::Place::Whole,
        });
        let key = 0x0990_0001;
        let read_before = READ.load(Ordering::SeqCst);
        let guards = fake::GUARDS.load(Ordering::SeqCst);
        assert!(defer(((key, 1), (surface, None))), "left on the device");
        assert_eq!(
            orbistoun_mem::watch::written_since(base, 4, u64::MAX),
            Some(true),
            "owed: every cache reads it again"
        );
        assert!(
            defer(((key, 2), (surface, None))),
            "the next dispatch's supersedes it"
        );
        assert_eq!(
            fake::GUARDS.load(Ordering::SeqCst),
            guards + 1,
            "guarded once"
        );
        assert!(deferred().lock().expect("list").iter().any(|copy| matches!(
            copy.held,
            Held::DispatchImage { key: kept, generation: 2, .. } if kept == key
        )));
        assert_eq!(
            READ.load(Ordering::SeqCst),
            read_before,
            "nothing read back"
        );

        let cp = GuestCp {
            memory: MappedRegions::current(),
            submission: None,
        };
        let bytes = crate::cp::CpMemory::read(&cp, base, 4).expect("readable");
        assert_eq!(READ.load(Ordering::SeqCst), read_before + 1, "read once");
        assert_eq!(
            bytes,
            0xa1b2_c3d4_u32.to_le_bytes(),
            "the image is in memory"
        );
        assert_ne!(
            orbistoun_mem::watch::written_since(base, 4, u64::MAX),
            Some(true),
            "nothing owed once written back"
        );
    }
}
