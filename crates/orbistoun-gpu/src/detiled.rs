//! Which tiled layouts a run's textures were detiled from, and what each cost: the measurement a
//! faster detile is aimed by. A texture read afresh - not kept from an earlier read - is counted
//! under its layout with its bytes and the time its read and detile took.

use std::collections::BTreeMap;
use std::sync::Mutex;

/// One layout's fresh reads: how many, their bytes, and their nanoseconds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Detiled {
    /// Base levels read afresh.
    pub reads: u64,
    /// Their bytes, as tiled in guest memory.
    pub bytes: u64,
    /// The time their reads and detiles took.
    pub nanos: u64,
}

/// Every layout's fresh reads so far, by the layout's name.
static BY_LAYOUT: Mutex<BTreeMap<String, Detiled>> = Mutex::new(BTreeMap::new());

/// Counts one base level read afresh from `layout`: `bytes` of it, in `took`.
pub fn note(layout: &str, bytes: u64, took: std::time::Duration) {
    let Ok(mut by_layout) = BY_LAYOUT.lock() else {
        return;
    };
    let row = by_layout.entry(layout.to_owned()).or_default();
    row.reads += 1;
    row.bytes += bytes;
    row.nanos += u64::try_from(took.as_nanos()).unwrap_or(u64::MAX);
}

/// Every layout's fresh reads so far.
#[must_use]
pub fn by_layout() -> Vec<(String, Detiled)> {
    BY_LAYOUT.lock().map_or_else(
        |_| Vec::new(),
        |by_layout| {
            by_layout
                .iter()
                .map(|(layout, row)| (layout.clone(), *row))
                .collect()
        },
    )
}

/// The run's line: each layout's reads, megabytes and milliseconds, the costliest first. `None`
/// when nothing was detiled.
#[must_use]
pub fn describe(rows: &[(String, Detiled)]) -> Option<String> {
    if rows.is_empty() {
        return None;
    }
    let mut rows = rows.to_vec();
    rows.sort_by(|a, b| b.1.nanos.cmp(&a.1.nanos).then_with(|| a.0.cmp(&b.0)));
    let parts: Vec<String> = rows
        .iter()
        .map(|(layout, row)| {
            format!(
                "{layout} {} read(s), {} MB, {} ms",
                row.reads,
                row.bytes / 1_000_000,
                row.nanos / 1_000_000
            )
        })
        .collect();
    Some(format!(
        "textures detiled afresh, by layout, costliest first: {}",
        parts.join("; ")
    ))
}

#[cfg(test)]
mod tests {
    use super::Detiled;

    /// Layouts are listed costliest first, with their reads, megabytes and milliseconds; a run that
    /// detiled nothing says nothing.
    #[test]
    fn layouts_are_listed_costliest_first() {
        let row = |reads, bytes, nanos| Detiled {
            reads,
            bytes,
            nanos,
        };
        let line = super::describe(&[
            ("S64Kb".to_owned(), row(10, 40_000_000, 50_000_000)),
            (
                "Rx64KbBpp1".to_owned(),
                row(800, 1_600_000_000, 7_200_000_000),
            ),
        ]);
        assert_eq!(
            line.as_deref(),
            Some(concat!(
                "textures detiled afresh, by layout, costliest first: ",
                "Rx64KbBpp1 800 read(s), 1600 MB, 7200 ms; S64Kb 10 read(s), 40 MB, 50 ms"
            ))
        );
        assert_eq!(super::describe(&[]), None);
    }
}
