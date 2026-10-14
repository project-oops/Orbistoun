//! A title's mods' cheats, written into the running title at its flips (D836).
//!
//! Armed once the executable is linked, with its base; each guest flip then writes every cheat due
//! at the address its chain reaches. A chain that does not reach yet - a structure the title has
//! not made - is tried again at the next flip.

use std::sync::Mutex;

/// A cheat armed for this run: the mod it came from, the executable's base, and whether a cheat
/// written once has been.
struct Armed {
    from: String,
    base: u64,
    cheat: orbistoun_overrides::mods::Cheat,
    written: bool,
}

/// The cheats armed for this run.
static ARMED: Mutex<Vec<Armed>> = Mutex::new(Vec::new());

/// The executable's segments - first address, end, and whether it may be written - which the
/// guest's mapping table does not hold: it holds what the guest mapped, not the image.
static IMAGE: Mutex<Vec<(u64, u64, bool)>> = Mutex::new(Vec::new());

/// Whether `[at, at + length)` lies inside one of the executable's segments, a writable one where
/// `write` asks.
fn in_image(at: u64, length: u64, write: bool) -> bool {
    let end = at.saturating_add(length);
    IMAGE.lock().is_ok_and(|segments| {
        segments
            .iter()
            .any(|&(first, last, writable)| first <= at && end <= last && (writable || !write))
    })
}

/// Arms `cheats` from the mod `from` against the executable at `image`.
pub(crate) fn arm(
    from: &str,
    image: &orbistoun_loader::Image,
    cheats: Vec<orbistoun_overrides::mods::Cheat>,
) {
    if let Ok(mut segments) = IMAGE.lock()
        && segments.is_empty()
    {
        segments.extend(image.segments().iter().map(|segment| {
            (
                segment.address,
                segment.address.saturating_add(segment.memsz()),
                segment.protection().write,
            )
        }));
    }
    let base = image.base();
    let mut armed = ARMED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    armed.extend(cheats.into_iter().map(|cheat| Armed {
        from: from.to_owned(),
        base,
        cheat,
        written: false,
    }));
}

/// Writes every cheat due at this flip.
pub(crate) fn at_flip() {
    let Ok(mut armed) = ARMED.try_lock() else {
        return;
    };
    for one in armed.iter_mut() {
        if one.written && one.cheat.every == orbistoun_overrides::mods::CheatTiming::Once {
            continue;
        }
        // A chain is the user's, and may point anywhere: each step is asked of the guest's own
        // mappings before it is touched.
        let read_u64 = |at: u64| {
            if !in_image(at, 8, false) && !orbistoun_kernel::is_guest_readable(at, 8) {
                return None;
            }
            // SAFETY: eight bytes inside the executable's segments or the guest's readable
            // mappings, read unaligned.
            unsafe { orbistoun_mem::guest::read_u64(at) }
        };
        let Some(at) = one.cheat.address(one.base, read_u64) else {
            continue;
        };
        let length = one.cheat.value.len() as u64;
        if !in_image(at, length, true) && !orbistoun_kernel::is_guest_writable(at, length) {
            continue;
        }
        // SAFETY: as many bytes as the value, inside a writable segment of the executable or a
        // mapping the guest may write.
        let landed = unsafe { orbistoun_mem::guest::write_bytes(at, &one.cheat.value) };
        if landed && !one.written {
            tracing::info!(
                "the mod {}'s cheat{} first wrote at {at:#x}",
                one.from,
                one.cheat
                    .description
                    .as_deref()
                    .map_or_else(String::new, |what| format!(" ({what})"))
            );
        }
        one.written |= landed;
    }
}
