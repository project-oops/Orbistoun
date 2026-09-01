//! Every diagnostic axis, against a real wall.
//!
//! ```text
//! cargo test -p orbistoun-turn --release --test axes -- --ignored --nocapture
//! ```
//!
//! # Why this exists
//!
//! `orbistoun-cli env` lists ten diagnostics. Only one of them - planting a value at an
//! argument - had ever been swept automatically, and doing that exhaustively across a
//! whole title took fifty seconds. The rest are the same price and had never been run
//! systematically at all.
//!
//! Each one asks a different question, so a negative from one says nothing about
//! another:
//!
//! - **Fill** - does this wall depend on memory nobody wrote? Asked of the stack once
//!   before, by hand (D185), and never of the heap or the zero-initialised statics -
//!   which the variable's own description calls *"the last region a poison could not
//!   reach"*.
//! - **Map** - is the faulting address a region the guest expected to exist? Nobody has
//!   asked this at all, and the fault is a write to an address in no mapped region.
//!
//! # Two signals, because one cannot tell a lead from a regression
//!
//! The first run of this reported that poisoning zero-initialised statics *moved the
//! wall*. It had not. The guest reached eight distinct imports instead of twenty-three
//! and died somewhere else entirely - the poison broke it long before it got near what
//! was being asked about. The fault address alone said "moved"; only how far it got says
//! which way. D129 records the same lesson about the progress verdict.
//!
//! # What a result here is, and is not
//!
//! An observation. Principle 3: *"an intervention that moves a wall is not a
//! diagnosis"* - a poisoned region that shifts a fault has not explained it, and a
//! mapped region that lets the guest continue may only have postponed the same mistake.
//! So this prints what changed and stops. Reading it needs a second observation of a
//! different kind, which is a person's job.

use orbistoun_turn::axis::{Change, against_a_wall};
use orbistoun_turn::experiment::Trial;
use orbistoun_turn::trial::{GuestTrial, traces_in};

const TITLE: &str = "../../titles/PPSA02664-app0/eboot.bin";
const BINARY: &str = "../../target/release/orbistoun-cli.exe";

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

    // Not asserted: what the guest does is a fact about the guest. What is asserted is
    // that something was actually run - every axis reporting `NotApplied` means the
    // sweep measured nothing, and reading that as "none of these regions matters" is the
    // failure this whole distinction exists to prevent.
    assert!(
        !results
            .iter()
            .all(|(_, change)| *change == Change::NotApplied),
        "no axis was applied - the sweep measured nothing"
    );
}
