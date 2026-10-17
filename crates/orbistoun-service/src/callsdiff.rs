//! What one run's imports did that another's did not: the imports only one reached, and those
//! whose share of the calls moved. Two runs differing in one setting - a press, an answer - say by
//! this whether the setting changed anything the title asked of the platform.

use std::collections::BTreeMap;

/// The fewest calls an import needs in either run for its share to be compared: below it, one call
/// more or less moves the share by chance.
pub const FEWEST_COMPARED: u64 = 20;

/// How far an import's share of the calls must move, as a ratio either way, to be reported.
pub const SHARE_MOVED: f64 = 2.0;

/// How two runs' import calls differ.
#[derive(Debug, Default, PartialEq)]
pub struct CallsDifference {
    /// Imports only the first run reached, with their calls.
    pub only_first: Vec<(String, u64)>,
    /// Imports only the second run reached, with their calls.
    pub only_second: Vec<(String, u64)>,
    /// Imports both reached whose share of the calls moved by [`SHARE_MOVED`] or more, with their
    /// calls in each run, the largest move first.
    pub moved: Vec<(String, u64, u64)>,
}

impl CallsDifference {
    /// Whether the two runs asked the same of the platform, as far as this measures.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.only_first.is_empty() && self.only_second.is_empty() && self.moved.is_empty()
    }
}

/// How the calls of `second` differ from those of `first`, each an import's label with its calls.
#[must_use]
// A share is a ratio of counts compared against a factor of two: a count past 2^52, where the
// conversion rounds, moves it by nothing that matters.
#[allow(clippy::cast_precision_loss)]
pub fn calls_difference(
    first: &BTreeMap<String, u64>,
    second: &BTreeMap<String, u64>,
) -> CallsDifference {
    let total = |calls: &BTreeMap<String, u64>| calls.values().sum::<u64>().max(1);
    let (first_total, second_total) = (total(first), total(second));
    let mut difference = CallsDifference::default();
    for (label, &calls) in first {
        if !second.contains_key(label) {
            difference.only_first.push((label.clone(), calls));
        }
    }
    for (label, &calls) in second {
        let Some(&before) = first.get(label) else {
            difference.only_second.push((label.clone(), calls));
            continue;
        };
        if before.max(calls) < FEWEST_COMPARED {
            continue;
        }
        let share = |calls: u64, total: u64| (calls as f64 + 1.0) / total as f64;
        let ratio = share(calls, second_total) / share(before, first_total);
        if ratio >= SHARE_MOVED || ratio <= 1.0 / SHARE_MOVED {
            difference.moved.push((label.clone(), before, calls));
        }
    }
    let moved_by = |(_, before, after): &(String, u64, u64)| {
        let ratio = (*after as f64 + 1.0) / (*before as f64 + 1.0);
        ratio.max(1.0 / ratio)
    };
    difference
        .moved
        .sort_by(|a, b| moved_by(b).total_cmp(&moved_by(a)));
    difference
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    fn calls(pairs: &[(&str, u64)]) -> BTreeMap<String, u64> {
        pairs.iter().map(|&(l, c)| (l.to_owned(), c)).collect()
    }

    /// An import only one run reached is named on its side; one whose share of the calls moved
    /// past the ratio is named with both counts; one too rarely called to compare is not. GTA V's
    /// runs with and without presses differed in none of these: the presses changed nothing it
    /// asked of the platform.
    #[test]
    fn imports_reached_and_shares_moved_are_named() {
        let first = calls(&[("a", 1000), ("b", 100), ("c", 5), ("gone", 3)]);
        let second = calls(&[("a", 1000), ("b", 400), ("c", 15), ("new", 1)]);
        let difference = super::calls_difference(&first, &second);
        assert_eq!(difference.only_first, [("gone".to_owned(), 3)]);
        assert_eq!(difference.only_second, [("new".to_owned(), 1)]);
        assert_eq!(difference.moved, [("b".to_owned(), 100, 400)]);
        assert!(super::calls_difference(&first, &first).is_empty());
    }
}
