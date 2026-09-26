//! Does the base come back as the answer to any call the guest makes?
//!
//! ```text
//! cargo test -p orbistoun-turn --release --test returns -- --ignored --nocapture
//! ```
//!
//! With every argument slot of every import swept, the remaining channel is a function handing the
//! base back as its answer rather than filling one in. Forcing a return on an implemented function
//! changes what it does, not only what it says, so the run is a different program.
//!
//! A positive result is an offset that agrees across two sentinels, not a fault that moved. The
//! wall is a write to `0xfffe0` with `rdx = 0x100000`, and the caller lays an arena header at
//! `region_end - 0x20`, so a function that should have answered the base puts the fault at
//! `sentinel + 0xfffe0`.

use orbistoun_turn::axis::Axis;
use orbistoun_turn::experiment::{Agreement, Outcome, RETURN_SENTINELS, agreement};
use orbistoun_turn::trial::{GuestTrial, traces_in};
use std::collections::BTreeSet;

const TITLE: &str = "../../titles/PPSA02664-app0/eboot.bin";
const BINARY: &str = "../../target/release/orbistoun-cli.exe";

/// The offset the wall's own arithmetic would produce.
///
/// `region_end - 0x20` where `region_end` is `base + 0x100000`.
const WALL_OFFSET: i64 = 0xfffe0;

/// Some import's forced answer is checked against the wall's arithmetic, and at least one run is
/// applied.
#[test]
#[ignore = "boots a commercial title twice per import; opt-in via --ignored"]
fn does_any_call_answer_with_the_base() {
    for (what, path) in [("the release binary", BINARY), ("the title", TITLE)] {
        assert!(
            std::path::Path::new(path).exists(),
            "{what} is missing at {path} - build with `cargo build --release -p orbistoun-cli`"
        );
    }

    let data = tempfile::tempdir().expect("temp dir");
    let traces = traces_in(data.path());
    std::fs::create_dir_all(&traces).expect("traces dir");
    let trial = GuestTrial::new(BINARY, TITLE, &traces)
        .with_env(orbistoun_env::DATA_DIR.name, data.path().to_string_lossy());

    let baseline = trial.spawn(&[]).expect("a baseline run");
    let trace = read_trace(&traces);
    let targets = imports(&trace);
    eprintln!(
        "RETURNS  title={TITLE}\n baseline fault={:?}  imports={}  looking for +{WALL_OFFSET:#x}",
        baseline.fault.map(|f| format!("{f:#x}")),
        targets.len()
    );

    let started = std::time::Instant::now();
    let mut carries_the_base = Vec::new();
    let mut flows = Vec::new();
    for target in &targets {
        let runs: Vec<(u64, Outcome)> = RETURN_SENTINELS
            .iter()
            .map(|value| {
                (
                    *value,
                    trial
                        .spawn(&[Axis::Return {
                            target: target.clone(),
                            value: *value,
                        }])
                        .expect("the run is made"),
                )
            })
            .collect();

        let verdict = agreement(&baseline, &runs);
        if let Agreement::Offset(offset) = verdict {
            if offset == WALL_OFFSET {
                carries_the_base.push(target.clone());
            } else {
                flows.push((target.clone(), offset));
            }
        }
        let line = say(&verdict, &baseline);
        eprintln!("  {target:<40} {line}");
    }

    eprintln!(
        "\nRETURNS RESULT  {} imports in {:.1}s",
        targets.len(),
        started.elapsed().as_secs_f64()
    );
    if carries_the_base.is_empty() {
        eprintln!(concat!(
            "  No call answers with the base. Every channel into this wall that can be ",
            "varied from outside is now exhausted - arguments, unwritten memory in every ",
            "region, a reservation at the faulting address, and now return values. What ",
            "remains is not a call at all."
        ));
        if !flows.is_empty() {
            eprintln!(concat!(
                "\n  These answers do reach an address, at the wrong distance. Each is a ",
                "real relationship and none of them is this wall's arithmetic:"
            ));
            for (target, offset) in &flows {
                eprintln!("    {target} -> answer{offset:+#x}");
            }
        }
    } else {
        for target in &carries_the_base {
            eprintln!("  *** {target} answers what should have been the region base");
        }
        eprintln!(concat!(
            "\n  An intervention that moves a wall is not a diagnosis. This needs a second ",
            "observation of a different kind - what the guest does with the region once it ",
            "has one - before it is more than a coincidence at one offset."
        ));
    }

    // Not asserted: which import, or whether any, is a fact about the guest. What is asserted is
    // that something ran, since every import reporting `NotApplied` means nothing was measured.
    assert!(!targets.is_empty(), "the run reached no imports at all");
}

/// Every import the guest called, by bare symbol.
///
/// From `calls`, the ranked list of everything reached. The library prefix is stripped because
/// `ORBISTOUN_RETURN` is `<import>:<value>` split on `:`, so a qualified label resolves nothing.
fn imports(trace: &serde_json::Value) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for call in trace["calls"].as_array().into_iter().flatten() {
        let Some(label) = call["label"].as_str() else {
            continue;
        };
        let symbol = label.rsplit("::").next().unwrap_or(label);
        if seen.insert(symbol.to_owned()) {
            out.push(symbol.to_owned());
        }
    }
    out
}

/// The trace the baseline run wrote.
fn read_trace(traces: &std::path::Path) -> serde_json::Value {
    let newest = std::fs::read_dir(traces)
        .expect("traces dir")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|e| e == "json"))
        .max_by_key(|path| {
            std::fs::metadata(path)
                .and_then(|meta| meta.modified())
                .ok()
        })
        .expect("a trace was written");
    let text = std::fs::read_to_string(&newest).expect("reading the trace");
    serde_json::from_str(&text).expect("parsing the trace")
}

/// One verdict, as a line a person reads.
///
/// Lifted out of the sweep so the loop stays short; the wording is the finding.
fn say(verdict: &Agreement, baseline: &Outcome) -> String {
    match *verdict {
        Agreement::Offset(offset) if offset == WALL_OFFSET => {
            format!("*** the answer becomes the base - fault at answer+{offset:#x}")
        }
        Agreement::Offset(offset) => {
            let sign = if offset < 0 { "-" } else { "+" };
            format!(
                "**  the answer reaches an address - fault at answer{sign}{:#x}",
                offset.unsigned_abs()
            )
        }
        Agreement::Dereferenced => {
            "    the answer is dereferenced as given, and nothing computed from it".to_owned()
        }
        Agreement::Derailed { reached } => {
            format!(
                "    forcing it derails the run, reaching {reached} of {}",
                baseline.reached
            )
        }
        Agreement::Unchanged => {
            "    indifferent - it faults in exactly the same place whatever it is told".to_owned()
        }
        Agreement::Inconsistent => "    the answer changes the run, but not an address".to_owned(),
        // Not a negative: nothing was measured.
        Agreement::NotApplied => "    no call was answered - nothing was measured".to_owned(),
    }
}
