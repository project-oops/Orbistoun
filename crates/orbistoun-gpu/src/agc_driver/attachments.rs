//! Colour attachments after the first, kept on the device across the draw segments that draw into
//! them (D948).
//!
//! Each is written back as the first target's frame is when it is let go of (D719): a deferred
//! write over its own pages, which are guarded, so the first touch of them - by the guest, by the
//! command processor, or by a preparation reading them - writes the attachment back before the
//! touch goes on. Until something touches them, the next segment that draws into the attachment
//! draws over what the device holds, and nothing is read from memory or written back between.

use super::{
    ColourTarget, ComponentSwap, Contents, Deferred, Held, MoreTargetsSeeder, carry_out_all,
    carry_out_overlapping, deferred, guard_pages, lay_out_frame, lazy_copies, loaded,
    more_targets_io, note_carried_out, page_span, ranges_overlap, unprotect_overlapping,
    write_lookup,
};

/// A colour attachment after the first that a segment's draws go into, with what its memory held
/// when it was read: what is written around the frame when it goes back (D719). `held` where the
/// device holds it already, kept from an earlier segment and nothing read.
pub(super) struct More {
    pub(super) target: ColourTarget,
    pub(super) swap: ComponentSwap,
    pub(super) id: crate::ResourceId,
    pub(super) memory_then: Contents,
    pub(super) held: bool,
}

impl More {
    /// One read from memory: `bytes` are what its pages hold.
    pub(super) fn read(
        target: ColourTarget,
        swap: ComponentSwap,
        id: crate::ResourceId,
        bytes: Vec<u8>,
    ) -> Self {
        Self {
            target,
            swap,
            id,
            memory_then: Contents::kept(bytes),
            held: false,
        }
    }

    fn span(&self) -> u64 {
        self.target.words() as u64 * 4
    }
}

/// Hands `more` to the device through `seed`: each read from memory as its texels, each held there
/// as no words, which the device answers for by holding it still.
pub(super) fn seed(seed: MoreTargetsSeeder, more: &[More]) -> bool {
    let words: Vec<Vec<u32>> = more
        .iter()
        .map(|kept| {
            if kept.held {
                Vec::new()
            } else {
                kept.target.detile_mapped(&kept.memory_then.words(), |w| {
                    loaded(w, kept.target, kept.swap)
                })
            }
        })
        .collect();
    let seeds: Vec<(crate::ResourceId, (u32, u32), &[u32])> = more
        .iter()
        .zip(&words)
        .map(|(kept, words)| {
            (
                kept.id,
                (kept.target.width, kept.target.height),
                words.as_slice(),
            )
        })
        .collect();
    seed(&seeds)
}

/// What memory held under the attachment kept on the device as `id` for exactly `target` in `swap`
/// order, when one is and it is the only write deferred over its pages: the device holds what memory
/// will, and the next segment draws over it there.
pub(super) fn held(
    target: ColourTarget,
    swap: ComponentSwap,
    id: crate::ResourceId,
) -> Option<Contents> {
    let span = target.words() as u64 * 4;
    let list = deferred().lock().ok()?;
    let mut over = list
        .iter()
        .filter(|copy| ranges_overlap((copy.pages.0, copy.pages.1), page_span(target.base, span)));
    let only = over.next()?;
    if over.next().is_some() || only.destination != target.base || only.length != span {
        return None;
    }
    match &only.held {
        Held::Attachment {
            id: kept,
            target: from,
            swap: order,
            memory_then,
        } if *kept == id && *from == target && *order == swap => Some(memory_then.clone()),
        _ => None,
    }
}

/// Leaves `more` on the device, its write-back deferred over its own guarded pages, in place of the
/// one it supersedes. `false` where it cannot wait - no guard, or memory the command processor may
/// not write - and the caller writes it back now.
pub(super) fn defer(more: &More) -> bool {
    let Some(hooks) = lazy_copies().get() else {
        return false;
    };
    let (base, span) = (more.target.base, more.span());
    if !write_lookup()
        .get()
        .is_some_and(|allows| allows(base, span))
    {
        return false;
    }
    let pages = page_span(base, span);
    let Ok(mut list) = deferred().lock() else {
        return false;
    };
    // Guarded another way now: a protection kept would be saved as the one to restore (D720).
    unprotect_overlapping(pages.0, pages.1);
    // What this attachment held before the segment, still on the device, is drawn over and every
    // byte of it written by this one.
    let guard = list
        .iter()
        .position(|copy| copy.destination == base && copy.length == span)
        .map(|index| {
            crate::perf::span(crate::perf::Span::DeferredSuperseded, || ());
            let old = list.remove(index);
            old.release_snapshot(hooks);
            old.pages.2
        });
    let (touched, kept): (Vec<Deferred>, Vec<Deferred>) = std::mem::take(&mut *list)
        .into_iter()
        .partition(|copy| ranges_overlap((copy.pages.0, copy.pages.1), pages));
    *list = kept;
    let carried = !touched.is_empty();
    carry_out_all(&touched, hooks);
    if carried && guard.is_some() && guard_pages(hooks, pages.0, pages.1).is_none() {
        return false;
    }
    let Some(protection) = guard.or_else(|| {
        crate::perf::span(crate::perf::Span::CopyGuard, || {
            guard_pages(hooks, pages.0, pages.1)
        })
    }) else {
        return false;
    };
    list.push(Deferred {
        destination: base,
        length: span,
        pages: (pages.0, pages.1, protection),
        held: Held::Attachment {
            id: more.id,
            target: more.target,
            swap: more.swap,
            memory_then: more.memory_then.clone(),
        },
    });
    true
}

/// Carries out `copy`, an attachment kept on the device as `id`: read off the device and laid out
/// over memory as it was read, as a deferred frame is (D719). Its pages are already given back.
pub(super) fn carry_out(
    copy: &Deferred,
    id: crate::ResourceId,
    (target, swap, memory_then): (ColourTarget, ComponentSwap, &Contents),
) -> bool {
    note_carried_out(target);
    let Some((_, read)) = more_targets_io().get() else {
        return false;
    };
    let Some(frame) = crate::perf::span(crate::perf::Span::CarryOutTake, || read(id)) else {
        tracing::warn!(
            "a colour attachment kept on the device could not be read - {:#x} keeps what it held",
            copy.destination
        );
        return false;
    };
    lay_out_frame(copy, (target, swap, memory_then), &frame)
}

/// Writes back every attachment kept on the device over `[address, address + length)` - what a
/// read of a colour target there does first, so that nothing taken for unchanged since its last
/// read is an attachment's memory the device has drawn over since.
pub(super) fn settle_over(address: u64, length: u64) {
    let over = deferred().lock().is_ok_and(|list| {
        list.iter().any(|copy| {
            matches!(copy.held, Held::Attachment { .. })
                && ranges_overlap((copy.destination, copy.length), (address, length))
        })
    });
    if over {
        let _ = carry_out_overlapping(address, length);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::super::tests::{allow_writes_to, fake, region_of, serial};
    use super::super::{
        ColourTarget, ComponentSwap, GuestCp, MappedRegions, deferred, install_more_targets,
        set_guest_regions,
    };
    use super::{More, defer, held};

    static READ: AtomicU64 = AtomicU64::new(0);

    fn target(base: u64) -> ColourTarget {
        ColourTarget {
            base,
            width: 16,
            height: 8,
            pipe_bank_xor: 0,
            layout: crate::tiling::SurfaceLayout::Rx64Kb,
            place: crate::registers::Place::Whole,
        }
    }

    /// An attachment left on the device is held there for the next segment, superseded by the
    /// next without its pages guarded again, and written back - read off the device once - only
    /// when its memory is touched, after which nothing is held.
    #[test]
    fn an_attachment_stays_on_the_device_until_its_memory_is_touched() {
        let _guard = serial();
        fake::install();
        install_more_targets(
            |_| true,
            |_| {
                READ.fetch_add(1, Ordering::SeqCst);
                Some(fake::frame())
            },
        );
        let span = crate::tiling::surface_words_64kb_rx_bpp4(16, 8) * 4;
        let memory: &'static mut [u8] = Box::leak(vec![0x10u8; span].into_boxed_slice());
        let base = memory.as_ptr() as usize as u64;
        allow_writes_to(base, span as u64);
        set_guest_regions(vec![region_of(memory)]);
        let (target, swap, id) = (
            target(base),
            ComponentSwap::Standard,
            crate::ResourceId(0x00a7),
        );
        let more = || More::read(target, swap, id, vec![0x10; span]);

        let read_before = READ.load(Ordering::SeqCst);
        let guards = fake::GUARDS.load(Ordering::SeqCst);
        assert!(defer(&more()), "left on the device");
        assert!(
            held(target, swap, id).is_some(),
            "held for the next segment"
        );
        assert!(
            held(target, swap, crate::ResourceId(0x00a8)).is_none(),
            "by its own id"
        );
        assert!(defer(&more()), "the next segment's supersedes it");
        assert_eq!(
            fake::GUARDS.load(Ordering::SeqCst),
            guards + 1,
            "its pages guarded once"
        );
        assert_eq!(
            deferred()
                .lock()
                .expect("list")
                .iter()
                .filter(|copy| copy.destination == base)
                .count(),
            1
        );
        assert_eq!(
            READ.load(Ordering::SeqCst),
            read_before,
            "nothing read back"
        );

        // A command-processor read of its memory writes it back first.
        let cp = GuestCp {
            memory: MappedRegions::current(),
            submission: None,
        };
        let bytes = crate::cp::CpMemory::read(&cp, base, 4).expect("readable");
        assert_eq!(
            READ.load(Ordering::SeqCst),
            read_before + 1,
            "read back once"
        );
        assert_ne!(bytes, vec![0x10; 4], "the frame is in memory");
        assert!(
            held(target, swap, id).is_none(),
            "nothing held once written back"
        );
    }

    /// A colour target read over an attachment kept on the device writes the attachment back
    /// first, so the target's memory is not taken for what it held when last read.
    #[test]
    fn reading_a_target_over_a_kept_attachment_writes_it_back() {
        let _guard = serial();
        fake::install();
        install_more_targets(
            |_| true,
            |_| {
                READ.fetch_add(1, Ordering::SeqCst);
                Some(fake::frame())
            },
        );
        let span = crate::tiling::surface_words_64kb_rx_bpp4(16, 8) * 4;
        let memory: &'static mut [u8] = Box::leak(vec![0x20u8; span].into_boxed_slice());
        let base = memory.as_ptr() as usize as u64;
        allow_writes_to(base, span as u64);
        set_guest_regions(vec![region_of(memory)]);
        let (target, swap, id) = (
            target(base),
            ComponentSwap::Standard,
            crate::ResourceId(0x00b7),
        );
        assert!(defer(&More::read(target, swap, id, vec![0x20; span])));
        let read_before = READ.load(Ordering::SeqCst);
        super::settle_over(base, span as u64);
        assert_eq!(READ.load(Ordering::SeqCst), read_before + 1);
        assert!(held(target, swap, id).is_none());
    }
}
