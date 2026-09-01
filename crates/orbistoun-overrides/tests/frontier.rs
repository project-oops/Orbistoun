//! The frontier, held against the records actually in the tree.
//!
//! # Why a golden file and not more unit tests
//!
//! Four ranking and rendering bugs landed in one day, and **every one of them had passing
//! unit tests**. That is not a gap in coverage: a test written by whoever chose the
//! ordering asserts the ordering they chose, so it cannot see that the ordering is wrong.
//! The tests were faithful to a mistaken belief.
//!
//! What caught two of the four was rendering the real records and reading the result - a
//! spin on four imports sitting at the top of the frontier is obvious in a table and
//! invisible in a `assert!(a.beats(&b))`. This makes that mechanical: the table is
//! committed, and any change to ordering or rendering shows up as a diff to review rather
//! than as a surprise three weeks later (D184).
//!
//! **It is expected to change.** Recording a better run rewrites it, and that is the point -
//! the diff says what moved. Regenerate with:
//!
//! ```text
//! UPDATE_FRONTIER=1 cargo test -p orbistoun-overrides --test frontier
//! ```
//!
//! Reads `compat/`, which is tracked and holds no guest material, so this runs anywhere -
//! `titles/` is never tracked and a test that needed it could not run in CI at all.

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
        "\nthe frontier changed. If a run genuinely improved, regenerate it:\n  \
         UPDATE_FRONTIER=1 cargo test -p orbistoun-overrides --test frontier\n\
         and read the diff - that diff is the whole point of this test.\n"
    );
}

#[test]
fn every_record_in_the_tree_parses_and_carries_a_measurement() {
    // A record that stopped parsing would silently vanish from the frontier rather than
    // fail, and a table quietly missing a title reads exactly like a title nobody ran.
    //
    // **Counted by slot rather than by `[status]`.** This compared the number of files with
    // the number of *honest* records, which was the same number until a module was first run
    // under a measured policy: its result went to `[experiment]`, it had no `[status]` at
    // all, and a test about *parsing* failed for a reason that had nothing to do with
    // parsing (D312, D332).
    //
    // A record with neither slot is still wrong - that is a file nobody measured.
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

#[test]
fn the_order_is_total_so_diffs_mean_something() {
    // Two titles measured identically - which the abort-at-53 pair really are - must not
    // swap places between runs. Otherwise the golden file churns and its diffs stop being
    // read, which quietly disables the check above.
    let once = render_frontier(&frontier(records()));
    let mut reversed = records();
    reversed.reverse();
    assert_eq!(once, render_frontier(&frontier(reversed)));
}
