//! `ORBISTOUN_WATCH_GPU` at the command processor: which packet changed a watched range.
//!
//! The range's bytes are compared after each packet the command processor carries out, through
//! the memory it carries packets out with, so a fill or copy deferred over the range is carried
//! out first and the packet that issued it is the one named. Whatever reached the range - a
//! dispatch's write-back through any binding, alias or atomic, a draw's stores, `DMA_DATA`,
//! `WRITE_DATA`, `RELEASE_MEM` or `EVENT_WRITE` data - the packet after which it changed is
//! named. A change found as a stream starts was made outside the command processor: by the guest
//! CPU, or by work carried out apart from it. The range is compared in place; it is read only when
//! it changed.

use std::sync::{Mutex, OnceLock};

use crate::cp::CpMemory;

/// One range `ORBISTOUN_WATCH_GPU` names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Watched {
    /// Its first byte.
    pub at: u64,
    /// Its length in bytes.
    pub bytes: u64,
    /// Whether `:bound` asks for every dispatch that binds it, changed or not.
    pub bound: bool,
}

/// The ranges a value of `ORBISTOUN_WATCH_GPU` names: `<addr>+<len>`, comma-separated, in
/// hexadecimal with `0x` or decimal, each followed by `:bound` where every dispatch binding it is
/// wanted too.
///
/// # Errors
///
/// The entry that is not a range.
pub fn parse(value: Option<&str>) -> Result<Vec<Watched>, String> {
    let number = |text: &str| {
        text.strip_prefix("0x").map_or_else(
            || text.parse().ok(),
            |hex| u64::from_str_radix(hex, 16).ok(),
        )
    };
    value
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            let (range, bound) = entry
                .strip_suffix(":bound")
                .map_or((entry, false), |range| (range, true));
            range
                .split_once('+')
                .and_then(|(at, bytes)| Some((number(at)?, number(bytes)?)))
                .filter(|&(_, bytes)| bytes > 0)
                .map(|(at, bytes)| Watched { at, bytes, bound })
                .ok_or_else(|| entry.to_owned())
        })
        .collect()
}

/// The run's watched ranges, read from the environment once. A value that names no range is said
/// once and watches nothing.
pub fn watched() -> &'static [Watched] {
    static WATCHED: OnceLock<Vec<Watched>> = OnceLock::new();
    WATCHED.get_or_init(|| {
        parse(orbistoun_env::WATCH_GPU.get().as_deref()).unwrap_or_else(|entry| {
            tracing::warn!(
                "{} names {entry:?}, which is not <addr>+<len>[:bound]; nothing is watched",
                orbistoun_env::WATCH_GPU.name
            );
            Vec::new()
        })
    })
}

/// Each watched range's bytes when last compared; `None` before the first.
fn seen() -> &'static Mutex<Vec<Option<Vec<u8>>>> {
    static SEEN: Mutex<Vec<Option<Vec<u8>>>> = Mutex::new(Vec::new());
    &SEEN
}

/// A watched range that changed: the range, and its bytes before - `None` where it is read for the
/// first time - and after.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Changed {
    watched: Watched,
    before: Option<Vec<u8>>,
    after: Vec<u8>,
}

/// The watched ranges whose bytes changed since they were last compared, as `memory` holds them;
/// a range not readable now is passed over.
pub(crate) fn changes(memory: &dyn CpMemory) -> Vec<Changed> {
    let watched = watched();
    if watched.is_empty() {
        return Vec::new();
    }
    let mut seen = seen()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    seen.resize(watched.len(), None);
    let mut changed = Vec::new();
    for (range, seen) in watched.iter().zip(seen.iter_mut()) {
        if seen
            .as_ref()
            .is_some_and(|before| memory.holds(range.at, before))
        {
            continue;
        }
        let Some(after) = usize::try_from(range.bytes)
            .ok()
            .and_then(|bytes| memory.read(range.at, bytes))
        else {
            continue;
        };
        if seen.as_ref() == Some(&after) {
            continue;
        }
        changed.push(Changed {
            watched: *range,
            before: seen.replace(after.clone()),
            after,
        });
    }
    changed
}

/// How many changed words a line names, the lowest first.
const NAMED: usize = 8;

/// The words of `after` that differ from `before`, each its byte offset, what it held and what it
/// holds, the lowest first; every word where there is no before.
fn word_changes(before: Option<&[u8]>, after: &[u8]) -> Vec<(usize, Option<u32>, u32)> {
    let word = |bytes: &[u8], at: usize| {
        let mut held = [0; 4];
        let end = (at + 4).min(bytes.len());
        held[..end - at].copy_from_slice(&bytes[at..end]);
        u32::from_le_bytes(held)
    };
    (0..after.len())
        .step_by(4)
        .filter_map(|at| {
            let now = word(after, at);
            let then = before.map(|before| word(before, at));
            (then != Some(now)).then_some((at, then, now))
        })
        .collect()
}

/// The line naming `changed`: what changed it, how many words, and the first of them.
fn line(changed: &Changed, by: &str) -> String {
    let words = word_changes(changed.before.as_deref(), &changed.after);
    let named: Vec<String> = words
        .iter()
        .take(NAMED)
        .map(|(at, then, now)| {
            let then = then.map_or_else(|| "?".to_owned(), |word| format!("{word:#x}"));
            format!("+{at:#x} {then}->{now:#x}")
        })
        .collect();
    let range = format!("{:#x}+{:#x}", changed.watched.at, changed.watched.bytes);
    if changed.before.is_none() {
        format!("{range} first read {by}: {}", named.join(" "))
    } else {
        format!(
            "{range} changed {by}: {} words, first by offset {}",
            words.len(),
            named.join(" ")
        )
    }
}

/// Says each of `changes` was made by `by`.
pub(crate) fn say(changes: &[Changed], by: &str) {
    for changed in changes {
        tracing::info!("{}", line(changed, by));
    }
}

#[cfg(test)]
mod tests {
    use super::{Changed, Watched, line, parse, word_changes};

    /// Ranges are `<addr>+<len>`, comma-separated, each optionally `:bound`; anything else is
    /// named, and an empty range is not a range.
    #[test]
    fn a_watch_names_ranges_and_their_mode() {
        assert_eq!(
            parse(Some("0x1000+0x18, 8192+16:bound")),
            Ok(vec![
                Watched {
                    at: 0x1000,
                    bytes: 0x18,
                    bound: false
                },
                Watched {
                    at: 8192,
                    bytes: 16,
                    bound: true
                },
            ])
        );
        assert_eq!(parse(None), Ok(Vec::new()));
        assert_eq!(parse(Some("0x1000")), Err("0x1000".to_owned()));
        assert_eq!(parse(Some("0x1000+0")), Err("0x1000+0".to_owned()));
        assert_eq!(parse(Some("0x10+4:all")), Err("0x10+4:all".to_owned()));
    }

    /// Only the words that differ are named, a partial last word included; a range read for the
    /// first time names every word with no before.
    #[test]
    fn the_changed_words_are_the_ones_that_differ() {
        let before = [0, 0, 0x80, 0x7f, 1, 0, 0, 0, 9];
        let after = [0, 0, 0x80, 0xff, 1, 0, 0, 0, 7];
        assert_eq!(
            word_changes(Some(&before), &after),
            [(0, Some(0x7f80_0000), 0xff80_0000), (8, Some(9), 7)]
        );
        assert_eq!(
            word_changes(None, &after[..8]),
            [(0, None, 0xff80_0000), (4, None, 1)]
        );
    }

    /// A line names the range, what changed it and the words, or says the range was first read.
    #[test]
    fn a_line_names_the_range_and_its_changer() {
        let watched = Watched {
            at: 0x7400_9d05_d740,
            bytes: 8,
            bound: false,
        };
        let changed = Changed {
            watched,
            before: Some(vec![0; 8]),
            after: vec![0, 0, 0x80, 0x7f, 0, 0, 0, 0],
        };
        assert_eq!(
            line(&changed, "after WRITE_DATA at byte 0x40"),
            "0x74009d05d740+0x8 changed after WRITE_DATA at byte 0x40: 1 words, first by offset +0x0 0x0->0x7f800000"
        );
        let first = Changed {
            before: None,
            ..changed
        };
        assert_eq!(
            line(&first, "as a stream starts"),
            "0x74009d05d740+0x8 first read as a stream starts: +0x0 ?->0x7f800000 +0x4 ?->0x0"
        );
    }
}
