//! Running a guest: `run` and `handoff`.

use crate::common::previous_trace;
use crate::compat::record_compat;
use crate::progress::report_progress;
use anyhow::{Context, Result};

/// `run` - execute a guest, in a worker process.
///
/// Both shims go through the worker uniformly (D033): the CLI gets no in-process fast
/// path just because a CLI crash is cheap. One execution path means the GUI's protocol
/// is exercised on every CLI run rather than only when someone opens the GUI.
/// Validates a `--profile` name and, if good, sets the env the spawned worker reads.
///
/// Validated here, before the worker is spawned, so an unknown name fails fast with the
/// alternatives rather than the worker silently falling back to the configured machine (D409).
fn set_profile_for_run(profile: Option<&str>) -> Result<()> {
    if let Some(name) = profile {
        if orbistoun_shell::profiles::machine(name).is_none() {
            anyhow::bail!(
                "no console profile named {name} - known profiles: {}",
                orbistoun_shell::profiles::names().join(", ")
            );
        }
        // SAFETY: single-threaded here, and the spawned worker inherits this at startup.
        unsafe { std::env::set_var("ORBISTOUN_MACHINE_PROFILE", name) };
    }
    Ok(())
}

pub(crate) fn cmd_run(
    path: &std::path::Path,
    limit: u64,
    calls: u64,
    profile: Option<&str>,
    (symbols_db, input): (Option<&std::path::Path>, Option<&std::path::Path>),
    staged: bool,
) -> Result<()> {
    set_profile_for_run(profile)?;
    let mut worker =
        orbistoun_worker::WorkerHandle::spawn_self().context("spawning a worker process")?;

    // Read before the run, because the run overwrites it. The comparison is the whole
    // point of keeping traces at all.
    let before = previous_trace(path);
    // And its identity, so that "the run overwrites it" can be *checked* rather than
    // assumed. A worker that dies without writing leaves this file untouched (D470).
    let before_stamp = trace_stamp(path);

    let events = worker
        .request(&orbistoun_proto::Request::Run {
            path: path.to_path_buf(),
            symbols_db: symbols_db.map(std::path::Path::to_path_buf),
            // Zero is the explicit way to ask for no limit, rather than a magic
            // sentinel: an unlimited run is a deliberate choice, not a default.
            limit_seconds: (limit > 0).then_some(limit),
            call_budget: (calls > 0).then_some(calls),
            // Absolute, because the worker resolves nothing against this process's directory.
            input_script: input.map(std::path::absolute).transpose()?,
            capture_input: None,
            staged,
        })
        .context("driving the worker")?;

    for event in &events {
        match event {
            orbistoun_proto::Event::Reached { phase } => println!("reached {phase:?}"),
            // The terminal event repeats the furthest phase; the progress lines above
            // already said it, so only the outcome is new here.
            orbistoun_proto::Event::Terminated { outcome, .. } => match outcome {
                orbistoun_proto::Outcome::Halted { reason } => println!("halted {reason}"),
                other => println!("outcome {other:?}"),
            },
            orbistoun_proto::Event::Failed { error } => println!("failed {error}"),
            other => println!("event {other:?}"),
        }
    }

    worker.shutdown().context("shutting the worker down")?;

    // After the worker has exited, so the trace it wrote is complete - *if* it wrote one.
    //
    // **Checked, not assumed.** A worker that dies before it can record anything leaves the
    // previous run's file in place, and reading it back presents an old measurement as this
    // one - which compares equal to itself and reads as `same - nothing moved`, the most
    // convincing possible way to say nothing happened. Six reports came out of one file that
    // way (D470).
    if orbistoun_report::trace::wrote_a_trace(before_stamp, trace_stamp(path)) {
        if let Some(after) = previous_trace(path) {
            report_progress(before.as_ref(), &after);
            record_compat(path, &after);
        }
    } else {
        println!();
        println!("this run recorded no trace, so there is nothing of its own to report.");
        println!("  the worker ended without writing one - a fault it could not report, or a");
        println!("  crash inside orbistoun itself. A stored trace from an earlier run exists and");
        println!("  is deliberately not shown: presenting it here would read as this run's");
        println!("  measurement, and it is not one.");
    }
    Ok(())
}

/// What one run said about one field.
enum Used {
    /// The guest faulted on the poisoned value, so it used the field.
    Yes {
        /// What it did with it - read, wrote, or called.
        kind: String,
        /// Where in the guest it did so.
        site: String,
    },
    /// The run ended somewhere else, so it never reached this field.
    No {
        /// What did happen, for the reader who wants to know the run was not simply broken.
        instead: String,
    },
}

/// `handoff` - which fields of the handoff structure a runtime uses.
///
/// One run per field. A fault **on the poisoned address** means the field was used; anything
/// else means the run never reached it, which is as much of an answer.
pub(crate) fn cmd_handoff(path: &std::path::Path, fields: u64, limit: u64) -> Result<()> {
    println!("asking {} which handoff fields it uses", path.display());
    println!("  one run per field, poisoned with an address nothing maps\n");

    let mut used = Vec::new();
    for field in 0..fields {
        let verdict = ask_about_field(path, field, limit)?;
        match &verdict {
            Used::Yes { kind, site } => {
                // The kind carries its own preposition - "read of", "instruction fetch
                // from" - so the value goes straight after it, as the fault report does.
                println!("  field {field:2}  USED         {kind} the field's own value, at {site}");
                used.push(field);
            }
            Used::No { instead } => println!("  field {field:2}  not reached  ({instead})"),
        }
    }

    println!();
    if used.is_empty() {
        println!("no field was reached - the run stops before the runtime reads its argument");
        return Ok(());
    }
    let names: Vec<String> = used.iter().map(u64::to_string).collect();
    println!("fields used: {}", names.join(", "));
    println!(
        "everything else was never reached, which is a fact about this run rather than about the structure"
    );
    Ok(())
}

/// Runs the guest once with one field poisoned, and reads what happened.
fn ask_about_field(path: &std::path::Path, field: u64, limit: u64) -> Result<Used> {
    // **The handoff argument, selected rather than assumed.** This asked which field of the
    // handoff structure a guest used while the run it measured was handed whatever the
    // configuration named - which for a bare payload is not the handoff at all. It poisoned
    // fields of a block the guest never received, and answered "no field was reached" about a
    // structure that was never handed over (D399).
    //
    // SAFETY: single-threaded here, and the child process reads it at startup. Both variables
    // are removed again below so a later run is not silently still under them.
    unsafe { std::env::set_var(orbistoun_env::ENTRY_ARGUMENT.name, "handoff") };
    // SAFETY: as above.
    unsafe { std::env::set_var(orbistoun_env::HANDOFF_POISON.name, field.to_string()) };
    let outcome = run_quietly(path, limit);
    // SAFETY: as above.
    unsafe { std::env::remove_var(orbistoun_env::HANDOFF_POISON.name) };
    // SAFETY: as above.
    unsafe { std::env::remove_var(orbistoun_env::ENTRY_ARGUMENT.name) };
    outcome?;

    // **The worker's own constants, not a copy of them.** This has to recognise the exact
    // address the other side planted, and two numbers that must agree are two numbers that
    // can drift - the same reason the harvested constants moved to one crate (D385).
    let poisoned = orbistoun_worker::POISON_BASE + field * orbistoun_worker::POISON_STRIDE;
    let Some(trace) = previous_trace(path) else {
        return Ok(Used::No {
            instead: "the run left no trace".to_owned(),
        });
    };
    let Some(fault) = trace.fault.as_ref() else {
        return Ok(Used::No {
            instead: "the run did not fault".to_owned(),
        });
    };
    if fault.address == poisoned {
        return Ok(Used::Yes {
            kind: fault.kind.clone(),
            site: match (&fault.region, fault.offset) {
                (Some(region), Some(offset)) => format!("{region}+{offset:#x}"),
                _ => format!("{:#x}", fault.instruction_pointer),
            },
        });
    }
    Ok(Used::No {
        instead: format!("{} {:#x}", fault.kind, fault.address),
    })
}

/// One run, with the guest's own output kept out of the way.
fn run_quietly(path: &std::path::Path, limit: u64) -> Result<()> {
    let mut worker =
        orbistoun_worker::WorkerHandle::spawn_self().context("spawning a worker process")?;
    let _ = worker
        .request(&orbistoun_proto::Request::Run {
            path: path.to_path_buf(),
            symbols_db: None,
            limit_seconds: (limit > 0).then_some(limit),
            call_budget: None,
            input_script: None,
            capture_input: None,
            staged: false,
        })
        .context("driving the worker")?;
    worker.shutdown().context("shutting the worker down")
}

/// The identity of the trace file on disk, for telling this run's trace from an older one.
fn trace_stamp(module: &std::path::Path) -> Option<orbistoun_report::trace::Stamp> {
    let paths = orbistoun_paths::Paths::resolve();
    orbistoun_report::trace::stamp_of(&paths.traces_dir(), module)
}

/// One line of the tail: a run of consecutive calls sharing a label and an answer.
pub(crate) struct Run<'a> {
    pub(crate) label: &'a str,
    /// The first call's first argument, which is what the line shows.
    pub(crate) arg0: u64,
    pub(crate) returned: Option<u64>,
    pub(crate) from: u64,
    pub(crate) count: u64,
    /// Every distinct first argument across the run, so the line can say when `arg0` was
    /// not the only one (D574).
    pub(crate) firsts: std::collections::BTreeSet<u64>,
}
