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
    guest: &[u64],
) -> Result<GuestDispatch, DispatchError> {
    let program = crate::device_lost::noted_compute();
    let ordinal = crate::capture::next_ordinal();
    // A device-addressed dispatch was asked before its table was placed, so it records what it
    // touches, and its capture is held until it has run (D980).
    let armed = crate::direct::reached::armed();
    let wanted = armed.unwrap_or_else(|| crate::capture::asked(program));
    // The guest pages a device-addressed dispatch's user data names (D971), as they are before it
    // runs where a capture may be written of it; a capture written only after it ran, of a dispatch
    // that lost the device, keeps them as it left them, and says so.
    let before = (!guest.is_empty() && (wanted.written() || crate::recent::kept()))
        .then(|| crate::capture::guest_pages(guest));
    let reached = || {
        before.clone().map_or_else(
            || (crate::capture::guest_pages(guest), false),
            |pages| (pages, true),
        )
    };
    // Where the command processor placed the bindings, read only for a diagnostic that names it.
    let ranges = || orbistoun_gpu::bound_ranges::noted(module.1);
    let capture = |reason| {
        let (guest, guest_before) = reached();
        Capture {
            guest,
            guest_before,
            ranges: ranges().unwrap_or_default(),
            ordinal: Some(ordinal),
            source: orbistoun_gpu::translation_source::noted(module.1),
            ..Capture::of(
                (program, reason),
                module.0,
                memory,
                (push, groups),
                images,
                buffers,
            )
        }
    };
    match wanted {
        _ if armed.is_some() && wanted.written() => {
            let asked = owned(&capture(Reason::Asked), module.0);
            crate::direct::reached::hold(asked, module.1);
        }
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

/// An owned copy of `capture`, its module `module`, to keep past the dispatch it borrows from.
fn owned(capture: &Capture<'_>, module: &[u32]) -> Capture<'static> {
    Capture {
        module: std::borrow::Cow::Owned(module.to_vec()),
        ..capture.owned_without_module()
    }
}
