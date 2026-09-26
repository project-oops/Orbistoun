//! Every diagnostic axis, against a real wall.
//!
//! ```text
//! cargo test -p orbistoun-turn --release --test axes -- --ignored --nocapture
//! ```
//!
//! Each diagnostic asks a different question, so a negative from one says nothing about another: a
//! fill asks whether the wall depends on memory nobody wrote (stack, heap or zero-initialised
//! statics), and a reservation whether the faulting address is a region the guest expected. Reach
//! is read beside the fault address, because a poison that breaks the guest earlier also moves the
//! fault (D129). A result is an observation, printed and not concluded.

use orbistoun_turn::axis::{Change, against_a_wall};
use orbistoun_turn::experiment::Trial;
use orbistoun_turn::trial::{GuestTrial, traces_in};

const TITLE: &str = "../../titles/PPSA02664-app0/eboot.bin";
const BINARY: &str = "../../target/release/orbistoun-cli.exe";

/// Every diagnostic axis runs against a real wall, and at least one is applied.
#[test]
#[ignore = "boots a commercial title once per axis; opt-in via --ignored"]
fn sweep_every_diagnostic_axis() {
    for (what, path) in [("the release binary", BINARY), ("the title", TITLE)] {
        assert!(
            std::path::Path::new(path).exists(),
            "{what} is missing at {path} - build with `cargo build --release -p orbistoun-cli`"
        );
    }

    let data = tempfile::tempdir().expect("temp dir");
    let traces = traces_in(data.path());
    std::fs::create_dir_all(&traces).expect("traces dir");
    let mut trial = GuestTrial::new(BINARY, TITLE, &traces)
        .with_env(orbistoun_env::DATA_DIR.name, data.path().to_string_lossy());

    let baseline = trial.run(None).expect("a baseline run");
    eprintln!(
        "AXES  title={TITLE}\n baseline fault={:?}",
        baseline.fault.map(|f| format!("{f:#x}"))
    );

    let axes = against_a_wall(baseline.fault).expect("axes");
    let started = std::time::Instant::now();
    let results = trial.probe(&axes).expect("the probe runs");

    let mut notable = Vec::new();
    for (axis, change) in &results {
        let verdict = match change {
            Change::Nothing => "    unchanged".to_owned(),
            Change::MovedTo { address } => {
                notable.push((axis.clone(), change.clone()));
                format!("**  the fault moved to {address:#x}")
            }
            Change::NoLongerFaulted => {
                notable.push((axis.clone(), change.clone()));
                "*** it stopped faulting there".to_owned()
            }
            Change::BrokeEarlier {
                address,
                reached,
                was,
            } => format!("    broke earlier - {address:#x}, reaching {reached} of {was}"),
            Change::NotApplied => "    not applied - nothing measured".to_owned(),
        };
        eprintln!("  {:<58} {verdict}", axis.question());
    }

    eprintln!(
        "\nAXES RESULT  {} axes in {:.1}s",
        results.len(),
        started.elapsed().as_secs_f64()
    );
    if notable.is_empty() {
        eprintln!("  Nothing changed the fault. None of these regions is what it is missing.");
    } else {
        for (axis, change) in &notable {
            eprintln!("  {} -> {change:?}", axis.question());
        }
        eprintln!(concat!(
            "\n  An intervention that moves a wall is not a diagnosis. Each of these ",
            "needs a second observation, of a different kind, saying what the guest did ",
            "with it."
        ));
    }

    // Not asserted: what the guest does is a fact about the guest. What is asserted is that
    // something ran, since every axis reporting `NotApplied` means the sweep measured nothing.
    assert!(
        !results
            .iter()
            .all(|(_, change)| *change == Change::NotApplied),
        "no axis was applied - the sweep measured nothing"
    );
}
