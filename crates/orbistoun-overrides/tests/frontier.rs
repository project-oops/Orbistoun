//! The frontier, held against the records in the tree.
//!
//! A unit test written with an ordering asserts the ordering chosen, so it cannot see that the
//! ordering is wrong. The rendered table is committed instead, and any change to ordering or
//! rendering shows up as a diff to review (D184). It changes whenever a better run is recorded.
//! Regenerate with:
//!
//! ```text
//! UPDATE_FRONTIER=1 cargo test -p orbistoun-overrides --test frontier
//! ```
//!
//! Reads `compat/`, which is tracked and holds no guest material, so this runs anywhere.

use orbistoun_overrides::{OverrideFile, Status, frontier, render_frontier};

/// Where the records live, relative to this crate.
fn compat_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../compat")
}

/// Where the rendered frontier is kept.
fn golden() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/compat-frontier.txt")
}

/// Every record in the tree, parsed.
fn records() -> Vec<(String, Status)> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(compat_dir()) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "toml") {
            continue;
        }
        let Some(title) = path.file_stem().map(|n| n.to_string_lossy().into_owned()) else {
            continue;
        };
        let text = std::fs::read_to_string(&path).expect("reading a record");
        let file = OverrideFile::from_toml(&text)
            .unwrap_or_else(|e| panic!("{} does not parse: {e}", path.display()));
        if let Some(status) = file.status {
            out.push((title, status));
        }
    }
    out
}

/// The rendered frontier matches the committed golden file.
#[test]
fn the_frontier_matches_what_is_committed() {
    let rendered = render_frontier(&frontier(records()));

    if std::env::var_os("UPDATE_FRONTIER").is_some() {
        std::fs::write(golden(), &rendered).expect("writing the golden frontier");
        return;
    }

    let expected = std::fs::read_to_string(golden()).unwrap_or_default();
    assert_eq!(
        rendered.replace("\r\n", "\n"),
        expected.replace("\r\n", "\n"),
        concat!(
            "\nthe frontier changed. If a run genuinely improved, regenerate it:\n  ",
            "UPDATE_FRONTIER=1 cargo test -p orbistoun-overrides --test frontier\n",
            "and read the diff - that diff is the whole point of this test.\n"
        )
    );
}

/// Every record in the tree parses and carries a measurement.
#[test]
fn every_record_in_the_tree_parses_and_carries_a_measurement() {
    // A record that stopped parsing would vanish from the frontier rather than fail. Counted by
    // slot, because a record may carry only `[experiment]` and no `[status]`; a record with
    // neither slot is still wrong (D312).
    let mut files = 0;
    let mut measured = 0;
    for entry in std::fs::read_dir(compat_dir())
        .into_iter()
        .flatten()
        .flatten()
    {
        let path = entry.path();
        if path.extension().is_none_or(|x| x != "toml") {
            continue;
        }
        files += 1;
        let text = std::fs::read_to_string(&path).expect("reading a record");
        let file = OverrideFile::from_toml(&text)
            .unwrap_or_else(|e| panic!("{} does not parse: {e}", path.display()));
        assert!(
            file.status.is_some() || file.experiment.is_some(),
            "{} carries no measurement at all",
            path.display()
        );
        measured += 1;
    }
    assert_eq!(measured, files, "every .toml is readable and measured");
}

/// The order is total, so identical measurements never swap places between runs.
#[test]
fn the_order_is_total_so_diffs_mean_something() {
    // Two titles measured identically must not swap places between runs, or the golden file churns
    // and its diffs stop being read.
    let once = render_frontier(&frontier(records()));
    let mut reversed = records();
    reversed.reverse();
    assert_eq!(once, render_frontier(&frontier(reversed)));
}
