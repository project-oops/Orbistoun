//! One turn of the loop, with nobody reading the findings.
//!
//! ```text
//! cargo test -p orbistoun-turn --release --test turn -- --ignored --nocapture
//! ```
//!
//! This runs a real title, takes the findings the run produced and does everything reading them
//! would lead to, without a person. Writing code is not automated: every step it declines says why,
//! and "a function needs implementing" is the commonest.

use orbistoun_report::diagnose::findings;
use orbistoun_report::trace::CallTrace;
use orbistoun_turn::trial::{GuestTrial, traces_in};
use orbistoun_turn::turn::{Step, plan, turn};

const TITLE: &str = "../../titles/PPSA02664-app0/eboot.bin";
const BINARY: &str = "../../target/release/orbistoun-cli.exe";

/// One turn runs a real title's plan through `turn::turn` and produces work from its findings.
#[test]
#[ignore = "boots a commercial title many times; opt-in via --ignored"]
fn one_turn_with_nobody_reading_the_findings() {
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

    // The run itself; everything after this is what a person would have done with it.
    let baseline = trial.spawn(&[]).expect("a baseline run");
    let trace = newest_trace(&traces);
    let found = findings(&trace);

    eprintln!(
        "TURN  {} findings, fault={:?}",
        found.len(),
        baseline.fault.map(|f| format!("{f:#x}"))
    );
    for finding in found.iter().take(5) {
        eprintln!(
            "  {:?} {} - {}",
            finding.gap,
            finding.subject.as_deref().unwrap_or("-"),
            finding.what
        );
    }

    let plan = plan(&found, baseline.fault);
    let automatic = plan.iter().filter(|step| step.is_automatic()).count();
    eprintln!(
        "\nPLAN  {} steps, {automatic} of them this can take on its own",
        plan.len()
    );

    let started = std::time::Instant::now();
    // Runs the plan through `turn::turn`, the function a real caller uses, rather than a second
    // copy of the dispatcher (D289).
    let taken = turn(&mut trial, &plan).expect("the plan runs");
    for result in &taken {
        eprintln!("  {}", result.say());
    }
    let taken_count = taken.iter().filter(|t| t.was_taken()).count();

    eprintln!(
        concat!(
            "\nTURN RESULT  took {0} of {1} steps in {2:.1}s, and stopped at the ones ",
            "that are a person's"
        ),
        taken_count,
        plan.len(),
        started.elapsed().as_secs_f64()
    );

    // Asserted: a run with findings produced work rather than an empty plan. Where the guest got to
    // is a fact about the guest.
    assert!(!found.is_empty(), "the run produced no findings at all");
    assert!(!plan.is_empty(), "findings produced no plan");
    assert!(
        plan.iter().any(Step::is_automatic),
        "every step was declined - nothing was turned"
    );
}

/// The trace the run just wrote.
fn newest_trace(traces: &std::path::Path) -> CallTrace {
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
