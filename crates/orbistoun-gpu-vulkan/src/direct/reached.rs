//! A device-addressed dispatch's capture, written after it runs from the chunks its lookups found
//! (D980), so a replay holds every node and pointer the dispatch reached rather than the pages its
//! user data names alone (D971).
//!
//! The capture keeps each chunk as it was before the dispatch. A chunk the dispatch only read is
//! the same after it. A chunk it stored into is read before the copy-back, from its home pages,
//! which a copied chunk's stores do not reach (D974). Where the dispatch stored into a chunk that
//! was not copied, its bytes from before are gone, so the capture waits: every chunk the dispatch
//! reached is copied for the next dispatch of the program, and that one is written instead.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::Mutex;

use super::GUEST_PAGE;
use crate::capture::{Capture, CapturedGuest, Wanted};

/// The most dispatches of one program a capture waits through for one whose stores all land in
/// copies; past them it is written with what those chunks held after, and says so.
pub(super) const MOST_ATTEMPTS: u32 = 4;

/// A capture of the dispatch running on this thread, asked for and held until it has run.
struct Armed {
    program: Option<u64>,
    wanted: Wanted,
    attempt: u32,
    held: Option<Held>,
}

/// A capture held until its dispatch has run, and its module's hash.
pub(super) struct Held {
    pub(super) capture: Capture<'static>,
    pub(super) module_hash: u64,
}

thread_local! {
    static ARMED: RefCell<Option<Armed>> = const { RefCell::new(None) };
}

/// A capture waiting for a later dispatch of its program: the program, what was asked, and how many
/// dispatches it has waited.
static DEFERRED: Mutex<Option<(u64, Wanted, u32)>> = Mutex::new(None);

/// Before a device-addressed dispatch of `program`: whether it is captured, a capture waiting for
/// it first, else as `ORBISTOUN_CAPTURE_DISPATCH` asks. Either way the dispatch's diagnostics read
/// the answer from here ([`armed`]) rather than asking again.
pub(super) fn begin(program: Option<u64>) -> bool {
    let waiting = program.and_then(|program| {
        let mut deferred = DEFERRED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        deferred
            .filter(|&(waits_for, _, _)| waits_for == program)
            .and_then(|_| deferred.take())
    });
    let (wanted, attempt) = waiting.map_or_else(
        || (crate::capture::asked(program), 0),
        |(_, wanted, attempt)| (wanted, attempt),
    );
    ARMED.with(|armed| {
        *armed.borrow_mut() = Some(Armed {
            program,
            wanted,
            attempt,
            held: None,
        });
    });
    wanted.written()
}

/// What the device-addressed dispatch running on this thread was asked, where one is ([`begin`]).
pub(crate) fn armed() -> Option<Wanted> {
    ARMED.with(|armed| armed.borrow().as_ref().map(|armed| armed.wanted))
}

/// Holds `capture`, of the module hashed `module_hash`, until its dispatch has run.
pub(crate) fn hold(capture: Capture<'static>, module_hash: u64) {
    ARMED.with(|armed| {
        if let Some(armed) = armed.borrow_mut().as_mut() {
            armed.held = Some(Held {
                capture,
                module_hash,
            });
        }
    });
}

/// After the dispatch, with the mappings' lock still held and before any copy goes back to its home
/// pages: the capture held for it, where one is, written with every chunk it reached - `touched`,
/// the chunks its lookups found, and those of `written`, the ranges its stores marked - each
/// `chunk_bytes` long. `copied` says whether a chunk is copied (D974).
///
/// Where it stored into a chunk that is not copied and that no page named by its user data holds,
/// and `may_wait`, the capture waits for the program's next dispatch instead, at most
/// [`MOST_ATTEMPTS`] in all: the chunks it reached are answered, for the caller to copy for that
/// dispatch. This thread's capture is disarmed either way.
pub(super) fn after(
    (written, touched, chunk_bytes): (&[(u64, u64)], &[u64], u64),
    copied: impl Fn(u64) -> bool,
    may_wait: bool,
) -> Option<Vec<u64>> {
    let armed = ARMED.with(|armed| armed.borrow_mut().take())?;
    let held = armed.held?;
    let step = usize::try_from(chunk_bytes).unwrap_or(usize::MAX).max(1);
    let stored: Vec<u64> = written
        .iter()
        .flat_map(|&(base, len)| (base..base.saturating_add(len)).step_by(step))
        .collect();
    let mut chunks: Vec<u64> = touched.iter().chain(&stored).copied().collect();
    chunks.sort_unstable();
    chunks.dedup();
    let named = |chunk: u64| {
        held.capture
            .guest
            .iter()
            .any(|range| (range.at..range.at + range.words.len() as u64 * 4).contains(&chunk))
    };
    let lost: Vec<u64> = stored
        .into_iter()
        .filter(|&chunk| !copied(chunk) && !named(chunk))
        .collect();
    if let (false, true, Some(program)) = (lost.is_empty(), may_wait, armed.program)
        && armed.attempt + 1 < MOST_ATTEMPTS
    {
        tracing::info!(
            "the dispatch of the program at {program:#x} stored into {} guest chunk(s) not copied, whose bytes from before it are gone; its capture waits for the next dispatch, with every chunk it reached copied (D980)",
            lost.len()
        );
        *DEFERRED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some((program, armed.wanted, armed.attempt + 1));
        return Some(chunks);
    }
    if !lost.is_empty() {
        tracing::warn!(
            "a device-addressed capture holds {} guest chunk(s) the dispatch stored into as it left them, not as they were before it (D980)",
            lost.len()
        );
    }
    let mut capture = held.capture;
    capture.guest = reached_pages(&chunks, chunk_bytes, &capture.guest);
    capture.guest_before = capture.guest_before && lost.is_empty();
    let numbered = match armed.wanted {
        Wanted::Numbered(n) => Some(u64::from(n)),
        _ => None,
    };
    crate::capture::write(&capture, held.module_hash, numbered);
    None
}

/// The guest pages holding `chunks` - each `chunk_bytes` from its first byte - read from their home
/// pages now, with the ranges of `named` laid over them, in order and those that touch as one
/// range. A page's bytes no chunk or named range holds read zero, which no lookup reached.
///
/// Every chunk must lie in a guest range the device maps, which holds it committed and readable
/// while the mappings' lock is held: the caller holds it.
pub(super) fn reached_pages(
    chunks: &[u64],
    chunk_bytes: u64,
    named: &[CapturedGuest<'_>],
) -> Vec<CapturedGuest<'static>> {
    let page_words = (GUEST_PAGE / 4) as usize;
    let mut pages: BTreeMap<u64, Vec<u32>> = BTreeMap::new();
    for &chunk in chunks {
        let page = chunk & !(GUEST_PAGE - 1);
        let words = pages.entry(page).or_insert_with(|| vec![0; page_words]);
        let mut bytes = vec![0_u8; chunk_bytes as usize];
        // SAFETY: the chunk lies in a guest range the device maps, committed and readable, and the
        // mappings' lock the caller holds keeps it so for the read.
        if unsafe { orbistoun_mem::guest::read_into(chunk, &mut bytes) } {
            let from = ((chunk - page) / 4) as usize;
            for (word, value) in words[from..].iter_mut().zip(bytes.chunks_exact(4)) {
                *word = u32::from_le_bytes([value[0], value[1], value[2], value[3]]);
            }
        }
    }
    for range in named {
        for (index, page_words_named) in range.words.chunks(page_words).enumerate() {
            let page = range.at + index as u64 * GUEST_PAGE;
            let words = pages.entry(page).or_insert_with(|| vec![0; page_words]);
            words[..page_words_named.len()].copy_from_slice(page_words_named);
        }
    }
    let mut ranges: Vec<CapturedGuest<'static>> = Vec::new();
    for (page, words) in pages {
        match ranges.last_mut() {
            Some(last) if last.at + last.words.len() as u64 * 4 == page => {
                last.words.to_mut().extend_from_slice(&words);
            }
            _ => ranges.push(CapturedGuest {
                at: page,
                words: words.into(),
            }),
        }
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::{GUEST_PAGE, reached_pages};
    use crate::capture::CapturedGuest;

    /// Named ranges lie over the pages they share with the chunks, and pages that touch are one
    /// range; with no chunk the named ranges come back as they were.
    #[test]
    fn named_ranges_lie_over_the_pages_the_chunks_reached() {
        let page_words = (GUEST_PAGE / 4) as usize;
        let named = [CapturedGuest {
            at: 0x10 * GUEST_PAGE,
            words: vec![7; 2 * page_words].into(),
        }];
        let pages = reached_pages(&[], 0x1000, &named);
        assert_eq!(pages, named.to_vec());
    }
}
