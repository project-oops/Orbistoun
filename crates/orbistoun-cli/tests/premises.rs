//! `questions --premises`, and the one way its output can be confidently wrong.
//!
//! # The failure this is here for
//!
//! Grouping and truncation do not commute. `--top` was already applied to the queue before
//! the grouping was added, so the first version of this mode grouped a shortened list and
//! reported that four functions shared a premise fourteen of them share - in the same words,
//! with the same confidence, and with no way for a reader to tell (D538).
//!
//! That is the shape principle 3 forbids: output that reports more than its input supports.
//! So the invariant is checked against the tool's own full-run answer rather than against a
//! number written here, which would drift with the knowledge base and start passing for the
//! wrong reason.

/// Run `questions --premises` with the extra arguments given, as JSON.
///
/// JSON rather than the printed form because the assertion is about the *group*, and
/// scraping a function list back out of prose would be a second thing that can be wrong.
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

/// **`--top` shortens the list of premises, never the premises themselves.**
///
/// # What this asserts
///
/// That the heaviest premise carries the same functions whether or not the run was asked
/// for one group or all of them. It fails the moment grouping is moved back after
/// truncation, which is how this mode shipped its first version.
///
/// # What it cannot assert
///
/// That the grouping is *right* - that two questions belong together, or that the ranking
/// puts the useful premise first. Those are checked where the grouping lives, over data a
/// test can state. This checks only that asking for less does not silently change the
/// answer to a different question.
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

/// **The header counts the whole knowledge base, not the part being shown.**
///
/// The printed summary leads with how many premises there are and how much of the list
/// they carry. Under `--top` that has to stay a statement about everything, with the
/// truncation said out loud, or the one number a reader is most likely to quote becomes
/// whatever was asked for.
///
/// It cannot check the numbers are correct - only that they do not move when the window
/// onto them does.
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
