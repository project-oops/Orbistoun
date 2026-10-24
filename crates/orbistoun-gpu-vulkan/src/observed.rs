//! A guest dispatch with the diagnostics that observe it: captures (`capture`), watched ranges
//! (`watch`) and the latest dispatches kept for a loss (`recent`). None of them changes what the
//! dispatch does; each reads the inputs it was handed, which stay as they were before it ran.

use crate::capture::{Capture, Reason, Wanted};
use crate::compute::{DispatchError, GuestDispatch};
use crate::compute_buffers::BufferIn;
use crate::compute_images::DispatchImages;

/// Runs a guest dispatch ([`crate::compute::run_guest_dispatch`]) as
/// [`crate::dispatch_guest_hashed`] describes, with the run's diagnostics around it.
pub(crate) fn dispatch(
    module: (&[u32], u64),
    memory: &[u32],
    (push, groups): (&[u32], [u32; 3]),
    images: &DispatchImages<'_>,
    buffers: &[BufferIn<'_>],
) -> Result<GuestDispatch, DispatchError> {
    let program = crate::device_lost::noted_compute();
    let ordinal = crate::capture::next_ordinal();
    let wanted = crate::capture::asked(program);
    // Where the command processor placed the bindings, read only for a diagnostic that names it.
    let ranges = || orbistoun_gpu::bound_ranges::noted(module.1);
    let capture = |reason| Capture {
        ranges: ranges().unwrap_or_default(),
        ordinal: Some(ordinal),
        ..Capture::of(
            (program, reason),
            module.0,
            memory,
            (push, groups),
            images,
            buffers,
        )
    };
    match wanted {
        Wanted::Once => crate::capture::write(&capture(Reason::Asked), module.1, None),
        Wanted::Numbered(n) => {
            crate::capture::write(&capture(Reason::Asked), module.1, Some(u64::from(n)));
        }
        Wanted::No | Wanted::Capped => {}
    }
    if crate::watch::any()
        && crate::watch::note_bound(
            (ordinal, program, groups),
            module.0,
            ranges().as_ref(),
            buffers,
        )
        && let Some(n) = crate::capture::next_watched(Reason::Bound)
    {
        crate::capture::write(&capture(Reason::Bound), module.1, Some(u64::from(n)));
    }
    let lost_before = crate::device_lost::device_lost();
    // A loss ends the run once this is dropped, after the dispatch that met it is written out.
    let held = crate::device_lost::hold_ending();
    let dispatched =
        crate::compute::run_guest_dispatch(module, memory, (push, groups), images, buffers);
    match &dispatched {
        Ok(done) => {
            if crate::watch::any()
                && crate::watch::note(
                    (ordinal, program, groups),
                    ranges().as_ref(),
                    (memory, buffers),
                    done,
                )
                && let Some(n) = crate::capture::next_watched(Reason::Watched)
            {
                crate::capture::write(&capture(Reason::Watched), module.1, Some(u64::from(n)));
            }
            if crate::recent::kept() {
                crate::recent::keep(&capture(Reason::Recent), module.1);
            }
        }
        Err(error) => {
            let lost_after = crate::device_lost::device_lost() || crate::capture::is_loss(error);
            if crate::capture::written_for_loss(lost_before, lost_after, wanted.written()) {
                crate::capture::write(&capture(Reason::DeviceLost), module.1, None);
            }
            if crate::capture::written_for_loss(lost_before, lost_after, false) {
                crate::recent::write_on_loss();
            }
        }
    }
    drop(held);
    dispatched
}
