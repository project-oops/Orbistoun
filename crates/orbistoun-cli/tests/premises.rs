//! `questions --premises`: grouping happens before `--top` truncates (D538).
//!
//! Grouping a truncated list reports a fact about `--top` as a fact about the knowledge base. The
//! invariant is checked against the tool's own full-run answer, not a fixed number, so it does not
//! drift with the knowledge base.

/// Runs `questions --premises` with the extra arguments given, as JSON.
///
/// JSON because the assertion is about the group, and parsing it back out of prose adds a second
/// thing that can be wrong.
fn premises(extra: &[&str]) -> serde_json::Value {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_orbistoun-cli"))
        .arg("questions")
        .arg("--premises")
        .arg("--json")
        .args(extra)
        .output()
        .expect("running questions --premises");
    assert!(
        out.status.success(),
        "questions --premises failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("the premise list is JSON")
}

/// `--top` shortens the list of premises, never the premises themselves.
///
/// The heaviest premise carries the same functions whether one group or all of them was asked for.
/// Whether the grouping itself is right is tested where the grouping lives.
#[test]
fn asking_for_fewer_premises_does_not_shrink_the_ones_shown() {
    let all = premises(&[]);
    let one = premises(&["--top", "1"]);

    let all = all.as_array().expect("an array of premises");
    let one = one.as_array().expect("an array of premises");
    assert!(
        !all.is_empty(),
        "there are open questions, so there is at least one premise"
    );
    assert_eq!(one.len(), 1, "--top 1 shows one premise");
    assert_eq!(
        one[0], all[0],
        concat!(
            "the heaviest premise must be reported identically whether or not the run was ",
            "truncated - a shorter function list here means the queue was cut before it was ",
            "grouped, and the count printed beside it would be a fact about --top rather than ",
            "about the knowledge base"
        )
    );
}

/// The header counts the whole knowledge base, not the part being shown.
///
/// Under `--top` the summary still describes every premise and states the truncation. This checks
/// that the totals do not move with the window, not that they are correct.
#[test]
fn the_totals_do_not_move_when_the_window_does() {
    let text = |extra: &[&str]| {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_orbistoun-cli"))
            .arg("questions")
            .arg("--premises")
            .args(extra)
            .output()
            .expect("running questions --premises");
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .next()
            .unwrap_or_default()
            .to_owned()
    };
    let full = text(&[]);
    let cut = text(&["--top", "3"]);
    assert!(
        full.contains(" premises behind "),
        "the header says what it counted, got {full:?}"
    );
    assert_eq!(
        full, cut,
        "the header is about the knowledge base, so --top must not change it"
    );
}
