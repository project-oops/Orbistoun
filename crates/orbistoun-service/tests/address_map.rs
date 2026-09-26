//! The address map in `docs/ADDRESS_MAP.md` is checked against the source it describes (D513).
//!
//! A base is a named constant with a value, so "the document lists every base the source declares"
//! compares two machine-readable sets. A base chosen without that list can land on the mapping
//! arena. The map records starts, not lengths, so this cannot check that a span fits: two regions
//! four gibibytes apart pass here and would still collide if one grew past that.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// How close two bases may be before this refuses them: four gibibytes, the spacing of the `0x5E2*`
/// family and the tightest packing the tree uses.
const CLOSEST: u64 = 4 * 1024 * 1024 * 1024;

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/<crate>/ is two below the repository root")
        .to_path_buf()
}

/// Every `const *_BASE: u64` the tree declares at the top level of a source file.
///
/// Top level only: a constant inside `mod tests` is a test's own scratch address. A production base
/// that is nested should be lifted out rather than the rule loosened.
fn declared() -> BTreeMap<String, u64> {
    let mut found = BTreeMap::new();
    let crates = repository().join("crates");
    let mut stack = vec![crates];
    while let Some(directory) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // Only `src`: an integration test's constants are that test's business.
                if path.file_name().is_some_and(|n| n == "tests") {
                    continue;
                }
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let Ok(text) = std::fs::read_to_string(&path) else {
                    continue;
                };
                for line in text.lines() {
                    if let Some((name, value)) = base_declaration(line) {
                        found.insert(name, value);
                    }
                }
            }
        }
    }
    assert!(
        found.len() > 10,
        concat!(
            "the walk found {} bases, which means it stopped working rather than that the tree ",
            "emptied - a gate that silently matches nothing passes for the wrong reason"
        ),
        found.len()
    );
    found
}

/// `NAME` and its value, for a top-level `const NAME_BASE: u64 = 0x...;` line.
fn base_declaration(line: &str) -> Option<(String, u64)> {
    let rest = line
        .strip_prefix("const ")
        .or_else(|| line.strip_prefix("pub const "))
        .or_else(|| line.strip_prefix("pub(crate) const "))
        .or_else(|| line.strip_prefix("pub(super) const "))?;
    let (name, rest) = rest.split_once(": u64 = ")?;
    if !name.ends_with("BASE") {
        return None;
    }
    let digits = rest.strip_suffix(';')?.trim().strip_prefix("0x")?;
    u64::from_str_radix(&digits.replace('_', ""), 16)
        .ok()
        .map(|value| (name.to_owned(), value))
}

/// Every base the document lists, by constant name.
fn documented() -> BTreeMap<String, u64> {
    let text = std::fs::read_to_string(repository().join("docs/ADDRESS_MAP.md"))
        .expect("docs/ADDRESS_MAP.md");
    let mut found = BTreeMap::new();
    for line in text.lines() {
        let mut cells = line.split('|').map(str::trim);
        let (Some(""), Some(base), Some(name)) = (cells.next(), cells.next(), cells.next()) else {
            continue;
        };
        let (Some(base), Some(name)) = (base.strip_prefix("`0x"), name.strip_prefix('`')) else {
            continue;
        };
        let Some(base) = base.strip_suffix('`') else {
            continue;
        };
        let Ok(value) = u64::from_str_radix(&base.replace('_', ""), 16) else {
            continue;
        };
        found.insert(name.trim_end_matches('`').to_owned(), value);
    }
    found
}

/// The document names every base the source declares, at the value the source declares.
#[test]
fn the_map_describes_the_tree_it_claims_to() {
    let declared = declared();
    let documented = documented();

    let missing: Vec<_> = declared
        .iter()
        .filter(|(name, _)| !documented.contains_key(*name))
        .map(|(name, value)| format!("{name} = {value:#x}"))
        .collect();
    assert!(
        missing.is_empty(),
        concat!(
            "declared in the tree and absent from docs/ADDRESS_MAP.md: {:?} - a base ",
            "nobody can find is a base somebody else will collide with"
        ),
        missing
    );

    let phantom: Vec<_> = documented
        .keys()
        .filter(|name| !declared.contains_key(*name))
        .collect();
    assert!(
        phantom.is_empty(),
        concat!(
            "in docs/ADDRESS_MAP.md and not in the tree: {:?} - a map naming a region ",
            "that no longer exists is worse than no map"
        ),
        phantom
    );

    for (name, value) in &declared {
        assert_eq!(
            documented.get(name),
            Some(value),
            "{name} is {value:#x} in the tree and something else in the map"
        );
    }
}

/// No two regions start close enough together to be in doubt, such as a fixed heap placed at
/// `MAPPING_BASE` exactly.
#[test]
fn no_two_bases_are_within_four_gibibytes() {
    let declared = declared();
    let mut by_address: Vec<_> = declared.iter().map(|(n, v)| (*v, n.clone())).collect();
    by_address.sort_unstable();
    for pair in by_address.windows(2) {
        let [(low, low_name), (high, high_name)] = pair else {
            continue;
        };
        assert!(
            high.saturating_sub(*low) >= CLOSEST,
            concat!(
                "{} at {:#x} and {} at {:#x} are {} bytes apart, closer ",
                "than the {:#x} this tree spaces regions by - one of them is about to be ",
                "inside the other"
            ),
            low_name,
            low,
            high_name,
            high,
            high - low,
            CLOSEST
        );
    }
}
