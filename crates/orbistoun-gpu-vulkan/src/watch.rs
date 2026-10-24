//! Which guest dispatches change, or bind, a range of guest memory: `ORBISTOUN_WATCH_GPU`, as the
//! device sees it.
//!
//! The words a dispatch leaves in its window and buffers are compared with those it was handed,
//! at the guest addresses the command processor noted for them
//! ([`orbistoun_gpu::bound_ranges`]), aliases through the buffer they share. A buffer handed over
//! unread counts the words it stored. Stores through an image are not counted here; the command
//! processor's own comparison ([`orbistoun_gpu::memory_watch`]) sees every change however it was
//! made. A range watched `:bound` also names each dispatch that binds it. Each dispatch that
//! changes or binds a watched range is captured (`capture`), from the inputs it was handed, so a
//! replay makes the same change.

use std::ops::Range;

use orbistoun_gpu::bound_ranges::BoundRanges;
use orbistoun_gpu::memory_watch::Watched;

use crate::compute::GuestDispatch;
use crate::compute_buffers::{BufferIn, BufferOut};

/// A binding a dispatch changed words through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Binding {
    /// The window at binding 1.
    Window,
    /// A buffer, by slot.
    Buffer(usize),
}

/// The indices of the words of a binding at `base` of `bytes` that hold any byte of `watched`.
fn words_in(base: u64, bytes: u64, (at, length): (u64, u64)) -> Range<usize> {
    let start = at.max(base);
    let end = (at + length).min(base + bytes);
    if end <= start {
        return 0..0;
    }
    ((start - base) / 4) as usize..(end - base).div_ceil(4) as usize
}

/// One word a dispatch changed: its guest address, what it held before where the host read it,
/// and what the dispatch left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Change {
    pub(crate) address: u64,
    pub(crate) before: Option<u32>,
    pub(crate) after: u32,
}

/// The words among `words` - indices from `base` - a buffer handed over as `before` was changed
/// in, as `after` answers it. An unread buffer's before is not known: every word it stored counts.
fn buffer_changes(
    base: u64,
    before: BufferIn<'_>,
    after: &BufferOut,
    words: Range<usize>,
) -> Vec<Change> {
    let change = |i: usize, before, after| Change {
        address: base.wrapping_add(i as u64 * 4),
        before,
        after,
    };
    match before {
        BufferIn::Alias(..) => Vec::new(),
        BufferIn::Words(before) => words
            .filter_map(|i| {
                let left = *after.words.get(i)?;
                (before.get(i) != Some(&left)).then(|| change(i, before.get(i).copied(), left))
            })
            .collect(),
        BufferIn::Unread(_) => {
            if let Some((stored, values)) = &after.stored {
                stored
                    .iter()
                    .zip(values)
                    .filter(|&(&i, _)| words.contains(&(i as usize)))
                    .map(|(&i, &value)| change(i as usize, None, value))
                    .collect()
            } else if let Some(value) = after.whole {
                words.map(|i| change(i, None, value)).collect()
            } else {
                words
                    .filter(|&i| after.written.get(i).is_some_and(|&w| w != 0))
                    .map(|i| change(i, None, after.words.get(i).copied().unwrap_or(0)))
                    .collect()
            }
        }
    }
}

/// The words a dispatch changed in `watched`, by binding, leaving out bindings it changed none in.
/// An alias's words are its holder's from the alias's offset, so they are compared there; a word
/// two bindings reach is named under each.
pub(crate) fn changes_in(
    watched: (u64, u64),
    ranges: &BoundRanges,
    (memory, buffers): (&[u32], &[BufferIn<'_>]),
    after: &GuestDispatch,
) -> Vec<(Binding, Vec<Change>)> {
    let mut changed = Vec::new();
    if let Some((base, bytes)) = ranges.window {
        let words = words_in(base, bytes, watched)
            .filter_map(|i| {
                let left = *after.memory.get(i)?;
                (memory.get(i) != Some(&left)).then(|| Change {
                    address: base + i as u64 * 4,
                    before: memory.get(i).copied(),
                    after: left,
                })
            })
            .collect();
        changed.push((Binding::Window, words));
    }
    for (slot, ((&(base, bytes), &before), out)) in ranges
        .buffers
        .iter()
        .zip(buffers)
        .zip(&after.buffers)
        .enumerate()
    {
        let words = words_in(base, bytes, watched);
        let words = match before {
            BufferIn::Alias(holder, offset) => {
                let Some(&held) = buffers.get(holder) else {
                    continue;
                };
                let shift = (offset / 4) as usize;
                buffer_changes(
                    base.wrapping_sub(offset),
                    held,
                    out,
                    words.start + shift..words.end + shift,
                )
            }
            _ => buffer_changes(base, before, out, words),
        };
        changed.push((Binding::Buffer(slot), words));
    }
    changed.retain(|(_, words)| !words.is_empty());
    changed
}

/// How a slot is bound, as a line names it.
fn bound_as(buffer: BufferIn<'_>) -> String {
    match buffer {
        BufferIn::Words(words) => format!("words, {} read", words.len()),
        BufferIn::Unread(words) => format!("unread, {words} words the device starts at zero"),
        BufferIn::Alias(holder, offset) => {
            format!("alias of buffer {holder} from byte {offset:#x}")
        }
    }
}

/// The bindings of a dispatch that overlap `watched`, each with how it is bound.
pub(crate) fn bindings_over(
    watched: (u64, u64),
    ranges: &BoundRanges,
    buffers: &[BufferIn<'_>],
) -> Vec<(Binding, String)> {
    let overlaps = |(base, bytes): (u64, u64)| !words_in(base, bytes, watched).is_empty();
    let mut bound = Vec::new();
    if ranges.window.is_some_and(overlaps) {
        bound.push((Binding::Window, "the window".to_owned()));
    }
    for (slot, (&range, &buffer)) in ranges.buffers.iter().zip(buffers).enumerate() {
        if overlaps(range) {
            bound.push((Binding::Buffer(slot), bound_as(buffer)));
        }
    }
    bound
}

/// The guest's name for a dispatch's program.
fn named(program: Option<u64>) -> String {
    program.map_or_else(
        || "a module".to_owned(),
        |p| format!("the program at {p:#x}"),
    )
}

/// Says which ranges watched `:bound` the dispatch `ordinal` of `program` over `groups`, running
/// `module`, binds, through which slots, how, and whether its atomics reach them; answers whether
/// it binds any.
pub(crate) fn note_bound(
    (ordinal, program, groups): (u64, Option<u64>, [u32; 3]),
    module: &[u32],
    ranges: Option<&BoundRanges>,
    buffers: &[BufferIn<'_>],
) -> bool {
    let Some(ranges) = ranges else {
        return false;
    };
    let mut any = false;
    let mut targets = None;
    for watched in watched().iter().filter(|watched| watched.bound) {
        let bound = bindings_over((watched.at, watched.bytes), ranges, buffers);
        if bound.is_empty() {
            continue;
        }
        any = true;
        let targets = targets.get_or_insert_with(|| crate::atomics::atomic_targets(module));
        let through: Vec<String> = bound
            .iter()
            .map(|(binding, how)| {
                let (name, slot) = match binding {
                    Binding::Window => ("window".to_owned(), None),
                    Binding::Buffer(slot) => (format!("buffer {slot}"), Some(*slot)),
                };
                let atomics = crate::atomics::changes_atomically(targets, slot).said();
                format!("{name} ({how}; {atomics})")
            })
            .collect();
        tracing::info!(
            "the run's dispatch {ordinal} of {} over {groups:?} groups binds {:#x}+{:#x}: {}",
            named(program),
            watched.at,
            watched.bytes,
            through.join(", ")
        );
    }
    any
}

/// How many changed words a line names, the lowest first.
const NAMED_CHANGES: usize = 8;

/// The first changes as a line names them: each word's offset into the watched range starting at
/// `start`, what it held where known, and what it holds.
fn first_changes(start: u64, changed: &[(Binding, Vec<Change>)]) -> String {
    let all = distinct(changed);
    let named: Vec<String> = all
        .iter()
        .take(NAMED_CHANGES)
        .map(|change| {
            let before = change
                .before
                .map_or_else(|| "?".to_owned(), |word| format!("{word:#x}"));
            format!(
                "+{:#x} {before}->{:#x}",
                change.address.saturating_sub(start),
                change.after
            )
        })
        .collect();
    named.join(" ")
}

/// Every changed word once, by address, the lowest first.
fn distinct(changed: &[(Binding, Vec<Change>)]) -> Vec<&Change> {
    let mut all: Vec<&Change> = changed.iter().flat_map(|(_, words)| words).collect();
    all.sort_by_key(|change| change.address);
    all.dedup_by_key(|change| change.address);
    all
}

/// The run's watched ranges.
fn watched() -> &'static [Watched] {
    orbistoun_gpu::memory_watch::watched()
}

/// Says which of the watched ranges the dispatch `ordinal` of `program` over `groups` changed
/// words in, through which bindings, and its first changes; answers whether it changed any.
pub(crate) fn note(
    (ordinal, program, groups): (u64, Option<u64>, [u32; 3]),
    ranges: Option<&BoundRanges>,
    before: (&[u32], &[BufferIn<'_>]),
    after: &GuestDispatch,
) -> bool {
    let Some(ranges) = ranges else {
        return false;
    };
    let mut any = false;
    for watched in watched() {
        let watched = (watched.at, watched.bytes);
        let changed = changes_in(watched, ranges, before, after);
        if changed.is_empty() {
            continue;
        }
        any = true;
        let total = distinct(&changed).len();
        let through: Vec<String> = changed
            .iter()
            .map(|(binding, words)| match binding {
                Binding::Window => format!("window {}", words.len()),
                Binding::Buffer(slot) => format!("buffer {slot} {}", words.len()),
            })
            .collect();
        let named = named(program);
        let held_back = if after.escaped || after.loop_cut.is_some() {
            "; an access escaped or a loop was cut, so the dispatch is refused and none of it written back"
        } else {
            ""
        };
        tracing::info!(
            "the run's dispatch {ordinal} of {named} over {groups:?} groups changed {total} words in {:#x}+{:#x}: {}; first, by offset: {}{held_back}",
            watched.0,
            watched.1,
            through.join(", "),
            first_changes(watched.0, &changed)
        );
    }
    any
}

/// Whether any range is watched.
pub(crate) fn any() -> bool {
    !watched().is_empty()
}

#[cfg(test)]
mod tests {
    use super::{Binding, Change, bindings_over, changes_in, first_changes, words_in};
    use crate::compute::GuestDispatch;
    use crate::compute_buffers::{BufferIn, BufferOut};
    use orbistoun_gpu::bound_ranges::BoundRanges;

    fn out(words: Vec<u32>) -> BufferOut {
        BufferOut {
            words,
            written: Vec::new(),
            stored: None,
            whole: None,
        }
    }

    /// A store through an alias is found where the alias lies: the holder's words from the
    /// alias's offset, compared with what the holder was handed.
    #[test]
    fn a_change_through_an_alias_is_found_where_the_alias_lies() {
        let ranges = BoundRanges {
            window: None,
            buffers: vec![(0x9000, 64), (0x7400, 24)],
        };
        let holder: Vec<u32> = (0..16).collect();
        let buffers = [BufferIn::Words(&holder), BufferIn::Alias(0, 0x20)];
        let mut left = holder.clone();
        left[9] = 0xff80_0000;
        let after = GuestDispatch {
            memory: Vec::new(),
            escaped: false,
            loop_cut: None,
            stored: Vec::new(),
            buffers: vec![out(left.clone()), out(left)],
        };
        assert_eq!(
            changes_in((0x7400, 24), &ranges, (&[], &buffers), &after),
            [(
                Binding::Buffer(1),
                vec![Change {
                    address: 0x7404,
                    before: Some(9),
                    after: 0xff80_0000
                }]
            )]
        );
    }

    /// Every binding that overlaps a range is named with how it is bound, changed or not.
    #[test]
    fn every_binding_over_a_range_is_named() {
        let ranges = BoundRanges {
            window: Some((0x7000, 0x1000)),
            buffers: vec![(0x7400, 24), (0x9000, 64), (0x7410, 4)],
        };
        let read = [0; 6];
        let buffers = [
            BufferIn::Words(&read),
            BufferIn::Unread(16),
            BufferIn::Alias(0, 0x10),
        ];
        assert_eq!(
            bindings_over((0x7400, 24), &ranges, &buffers),
            [
                (Binding::Window, "the window".to_owned()),
                (Binding::Buffer(0), "words, 6 read".to_owned()),
                (
                    Binding::Buffer(2),
                    "alias of buffer 0 from byte 0x10".to_owned()
                ),
            ]
        );
        assert_eq!(bindings_over((0x8000, 4), &ranges, &buffers), []);
    }

    /// The words of a binding a range holds any byte of, partial words at either end included.
    #[test]
    fn a_range_covers_the_words_it_touches() {
        assert_eq!(words_in(0x1000, 0x100, (0x1000, 0x100)), 0..64);
        assert_eq!(words_in(0x1000, 0x100, (0x1006, 3)), 1..3);
        assert_eq!(words_in(0x1000, 0x100, (0xff0, 0x18)), 0..2);
        assert_eq!(words_in(0x1000, 0x100, (0x10fc, 0x40)), 63..64);
        assert_eq!(words_in(0x1000, 0x100, (0x2000, 4)), 0..0);
    }

    /// A dispatch's changes are found only inside the range: the window's and a read buffer's by
    /// comparing words, an unread buffer's by its stores in each of the forms it answers them, and
    /// an alias's through the buffer it shares.
    #[test]
    fn only_changes_inside_the_range_are_counted() {
        let ranges = BoundRanges {
            window: Some((0x1000, 16)),
            buffers: vec![
                (0x2000, 16),
                (0x3000, 16),
                (0x4000, 16),
                (0x5000, 16),
                (0x2000, 16),
            ],
        };
        let before = [1, 2, 3, 4];
        let read = [5, 6, 7, 8];
        let buffers = [
            BufferIn::Words(&read),
            BufferIn::Unread(4),
            BufferIn::Unread(4),
            BufferIn::Unread(4),
            BufferIn::Alias(0, 0),
        ];
        let after = GuestDispatch {
            memory: vec![1, 9, 9, 4],
            escaped: false,
            loop_cut: None,
            stored: Vec::new(),
            buffers: vec![
                out(vec![5, 0, 7, 0]),
                BufferOut {
                    written: vec![0, 1, 1, 0],
                    ..out(vec![0, 3, 0, 0])
                },
                BufferOut {
                    stored: Some((vec![3], vec![7])),
                    ..out(Vec::new())
                },
                BufferOut {
                    whole: Some(0),
                    ..out(Vec::new())
                },
                out(vec![5, 0, 7, 0]),
            ],
        };
        let changed = |watched| changes_in(watched, &ranges, (&before, &buffers), &after);
        let word = |address, before, after| Change {
            address,
            before,
            after,
        };
        assert_eq!(
            changed((0x1000, 8)),
            [(Binding::Window, vec![word(0x1004, Some(2), 9)])]
        );
        // The alias at slot 4 lies over buffer 0 from its start, so it names the same words.
        let through_buffer_0 = vec![word(0x2004, Some(6), 0), word(0x200c, Some(8), 0)];
        assert_eq!(
            changed((0x2000, 16)),
            [
                (Binding::Buffer(0), through_buffer_0.clone()),
                (Binding::Buffer(4), through_buffer_0),
            ]
        );
        assert_eq!(
            changed((0x3008, 8)),
            [(Binding::Buffer(1), vec![word(0x3008, None, 0)])]
        );
        assert_eq!(changed((0x4000, 8)), []);
        assert_eq!(
            changed((0x400c, 4)),
            [(Binding::Buffer(2), vec![word(0x400c, None, 7)])]
        );
        assert_eq!(
            changed((0x5004, 4)),
            [(Binding::Buffer(3), vec![word(0x5004, None, 0)])]
        );
        assert_eq!(changed((0x8000, 4)), []);
    }

    /// A line names the lowest changed words first, by offset into the range, with what each held
    /// where known, and no more than its few.
    #[test]
    fn the_first_changes_are_named_by_offset() {
        let word = |address, before, after| Change {
            address,
            before,
            after,
        };
        let changed = [
            (
                Binding::Buffer(2),
                vec![word(0x7408, Some(1), 0x109), word(0x740c, None, 3)],
            ),
            (Binding::Window, vec![word(0x7400, Some(0), 0x108)]),
        ];
        assert_eq!(
            first_changes(0x7400, &changed),
            "+0x0 0x0->0x108 +0x8 0x1->0x109 +0xc ?->0x3"
        );
        let many = [(
            Binding::Window,
            (0..20).map(|i| word(0x7400 + i * 4, None, 0)).collect(),
        )];
        assert_eq!(
            first_changes(0x7400, &many).split(' ').count(),
            super::NAMED_CHANGES * 2
        );
    }
}
