//! Sweeping every import the guest called, not just the one that looked guilty.
//!
//! ```text
//! cargo test -p orbistoun-turn --release --test tail -- --ignored --nocapture
//! ```
//!
//! # Why exhaustive, rather than asking something which to try
//!
//! The single-target sweep on this wall took **1.77 seconds** for thirteen boots - the
//! guest reaches the fault almost immediately, so a run costs about a tenth of a second
//! rather than the twenty this was budgeted at.
//!
//! That changes the shape of the problem entirely. `docs/BACKLOG.md` frames automated
//! semantics search around query cost - *"the value of any prior is entirely in reducing
//! the number of queries"* - and at a tenth of a second a query is nearly free. Every
//! import the guest called can be swept in the time it takes to describe one.
//!
//! So this asks nothing and guesses nothing. It plants two sentinels in all six argument
//! slots of **every distinct import in the trace** and reports which, if any, the fault
//! address follows. A model-driven proposer is worth building only if this comes back
//! with nothing.
//!
//! # What a negative result would mean
//!
//! That the base the guest expected filled does not arrive through any argument of any
//! call it made - which would rule out the out-parameter explanation for this wall
//! entirely, rather than narrowing it. That is a stronger statement than anything
//! reached by hand so far, and it is worth having either way.

use std::collections::BTreeSet;

use orbistoun_turn::experiment::{Finding, Trial, investigate};
use orbistoun_turn::trial::{GuestTrial, traces_in};

const TITLE: &str = "../../titles/PPSA02664-app0/eboot.bin";
const BINARY: &str = "../../target/release/orbistoun-cli.exe";

/// Every distinct import the guest called, most-used first.
///
/// From `calls`, which is the ranked list of everything reached - not `tail`, which is
/// only the last few. A wall reached through a call made early and used once is exactly
/// the case a tail would miss.
fn imports(trace: &serde_json::Value) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for call in trace["calls"].as_array().into_iter().flatten() {
        let Some(label) = call["label"].as_str() else {
            continue;
        };
        // **The library prefix is stripped, and it has to be.** `ORBISTOUN_WRITE` is
        // `<import>:<slot>:<value>`, split on `:` - so a label like
        // `libkernel::scePthreadMutexInit` produces five fields instead of three and no
        // target is ever resolved. The worker matches on the bare symbol anyway.
        //
        // Found by running this: passing whole labels made all 23 imports report
        // `NeverPlanted`, which is the only reason it was not read as "nothing moved".
        let symbol = label.rsplit("::").next().unwrap_or(label);
        if seen.insert(symbol.to_owned()) {
            out.push(symbol.to_owned());
        }
    }
    out
}

#[test]
#[ignore = "boots a commercial title several hundred times; opt-in via --ignored"]
fn sweep_every_import_the_guest_called() {
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

    // One baseline, to find out what the guest actually calls.
    let baseline = trial.run(None).expect("a baseline run");
    let trace = trial.trace().expect("a trace");
    let targets = imports(&trace);

    let started = std::time::Instant::now();
    eprintln!(
        "TAIL  title={TITLE}\n baseline fault={:?}  reached={}  imports={}",
        baseline.fault.map(|f| format!("{f:#x}")),
        trace["reached"].as_str().unwrap_or("?"),
        targets.len()
    );

    let mut interesting = Vec::new();
    for target in &targets {
        let (finding, _) = match investigate(&mut trial, target) {
            Ok(result) => result,
            Err(error) => {
                eprintln!("  {target:<44} FAILED - {error}");
                continue;
            }
        };
        let line = match &finding {
            Finding::OutParameter {
                slot,
                offset,
                answer,
            } => {
                interesting.push((target.clone(), finding.clone()));
                // The condition is part of the finding: without it the line claims the guest
                // reads the slot unconditionally, which for the wall this was built for is
                // false (D286).
                let needs = answer.map_or(String::new(), |a| format!(", when it answers {a:#x}"));
                format!("*** arg{slot}, fault follows it by {offset:#x}{needs}")
            }
            Finding::Dereferenced { slot } => concat!(
                "    arg{} is a pointer the guest follows - it dereferenced the sentinel ",
                "itself, which is the sweep breaking it rather than a finding"
            )
            .replace("{}", &slot.to_string()),
            Finding::Derailed {
                slot,
                touched,
                reached,
                was,
            } => format!(
                "    arg{slot} derailed the run - {}, reaching {reached} of {was}",
                if *touched {
                    "the fault is still an address it asked for"
                } else {
                    "the fault is no longer an address it asked for"
                }
            ),
            Finding::Moved { slot } => {
                interesting.push((target.clone(), finding.clone()));
                format!("**  arg{slot} moved the fault, inconsistently")
            }
            Finding::Unmoved {
                tested,
                not_addresses,
            } => format!("    planted {tested:?}, not addresses {not_addresses:?}"),
            Finding::Escaped { slot, reached, was } => {
                format!("    arg{slot} broke the loop: reached {reached} against {was}")
            }
            Finding::NeverPlanted => "    nothing planted - not measured".to_owned(),
        };
        eprintln!("  {target:<44} {line}");
    }

    eprintln!(
        "\nTAIL RESULT  {} imports swept in {:.1}s",
        targets.len(),
        started.elapsed().as_secs_f64()
    );
    if interesting.is_empty() {
        eprintln!(concat!(
            "  Nothing moved the fault. The base this guest expects is not reached ",
            "through any argument of any call it makes - which rules the out-parameter ",
            "explanation out for this wall rather than narrowing it."
        ));
    } else {
        for (target, finding) in &interesting {
            eprintln!("  {target}: {finding:?}");
        }
    }

    // Not asserted: which import, or none, is a fact about the guest. What is asserted is
    // that the sweep had something to sweep - an empty target list means the trace was
    // not read, and reporting that as "nothing moved" would be a conclusion drawn from a
    // measurement that never happened.
    assert!(
        !targets.is_empty(),
        "the trace listed no imports, so nothing was swept"
    );
}
