//! Running a guest: `run` and `handoff`.

use crate::common::previous_trace;
use crate::compat::record_compat;
use crate::progress::report_progress;
use anyhow::{Context, Result};

/// Validates a `--profile` name and sets the environment the spawned worker reads.
///
/// Validated before the worker is spawned, so an unknown name fails with the alternatives instead
/// of the worker falling back to the configured machine.
fn set_profile_for_run(profile: Option<&str>) -> Result<()> {
    if let Some(name) = profile {
        if orbistoun_shell::profiles::machine(name).is_none() {
            anyhow::bail!(
                "no console profile named {name} - known profiles: {}",
                orbistoun_shell::profiles::names().join(", ")
            );
        }
        // SAFETY: single-threaded here; the spawned worker inherits the variable at startup.
        unsafe { std::env::set_var("ORBISTOUN_MACHINE_PROFILE", name) };
    }
    Ok(())
}

/// `link` - link a title in a worker process and store its plan, entering nothing (D724).
pub(crate) fn cmd_link(
    path: &std::path::Path,
    symbols_db: Option<&std::path::Path>,
    relink: bool,
) -> Result<()> {
    let mut worker =
        orbistoun_worker::WorkerHandle::spawn_self().context("spawning a worker process")?;
    let events = worker
        .request(&orbistoun_proto::Request::Link {
            path: std::path::absolute(path)?,
            symbols_db: symbols_db.map(std::path::Path::to_path_buf),
            relink,
        })
        .context("driving the worker")?;
    worker.shutdown().context("shutting the worker down")?;
    for event in events {
        match event {
            orbistoun_proto::Event::Linked(summary) => {
                for line in describe_link(&summary, relink) {
                    println!("{line}");
                }
                return Ok(());
            }
            orbistoun_proto::Event::Failed { error } => anyhow::bail!("linking failed: {error}"),
            _ => {}
        }
    }
    anyhow::bail!("the worker ended without reporting a link")
}

/// What a link decided, one line per fact, then what differed.
fn describe_link(summary: &orbistoun_proto::LinkSummary, relink: bool) -> Vec<String> {
    let stored = if summary.stored.is_empty() {
        "not stored: no title library"
    } else {
        summary.stored.as_str()
    };
    let mut lines = vec![
        format!("modules  {}", summary.modules),
        format!("writes   {}", summary.writes),
        format!("syscalls {}", summary.syscalls),
        format!("digest   {}", summary.digest),
        format!("stored   {stored}"),
    ];
    if !summary.differs.is_empty() {
        lines.push(if relink {
            "replaced a stored plan that differed:".to_owned()
        } else {
            "differs from the stored plan under the same key - a loader defect:".to_owned()
        });
        lines.extend(summary.differs.iter().map(|d| format!("  {d}")));
    }
    lines
}

/// `run` - execute a guest in a worker process.
///
/// The CLI has no in-process path: every shim runs guests through the worker (D033).
pub(crate) fn cmd_run(
    path: &std::path::Path,
    limit: u64,
    calls: u64,
    profile: Option<&str>,
    (symbols_db, input): (Option<&std::path::Path>, Option<&std::path::Path>),
    (staged, relink): (bool, bool),
) -> Result<()> {
    set_profile_for_run(profile)?;
    let mut worker =
        orbistoun_worker::WorkerHandle::spawn_self().context("spawning a worker process")?;

    // Read before the run, which overwrites it.
    let before = previous_trace(path);
    // Its identity too, so whether this run wrote a new trace is checked (D470).
    let before_stamp = trace_stamp(path);

    let events = worker
        .request(&orbistoun_proto::Request::Run {
            path: path.to_path_buf(),
            symbols_db: symbols_db.map(std::path::Path::to_path_buf),
            // Zero asks for no limit explicitly.
            limit_seconds: (limit > 0).then_some(limit),
            call_budget: (calls > 0).then_some(calls),
            // Absolute, because the worker resolves nothing against this process's directory.
            input_script: input.map(std::path::absolute).transpose()?,
            capture_input: None,
            staged,
            relink,
        })
        .context("driving the worker")?;

    for event in &events {
        match event {
            orbistoun_proto::Event::Reached { phase } => println!("reached {phase:?}"),
            // The terminal event repeats the furthest phase, which the progress lines already
            // printed.
            orbistoun_proto::Event::Terminated { outcome, .. } => match outcome {
                orbistoun_proto::Outcome::Halted { reason } => println!("halted {reason}"),
                other => println!("outcome {other:?}"),
            },
            orbistoun_proto::Event::Failed { error } => println!("failed {error}"),
            other => println!("event {other:?}"),
        }
    }

    worker.shutdown().context("shutting the worker down")?;

    // After the worker has exited, so its trace is complete. A worker that dies before recording
    // leaves the previous run's file in place, which would compare equal to itself as "nothing
    // moved" (D470).
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
        /// What did happen, so the run is seen to be sound.
        instead: String,
    },
}

/// `handoff` - which fields of the handoff structure a runtime uses.
///
/// One run per field. A fault on the poisoned address means the field was used; anything else means
/// the run never reached it.
pub(crate) fn cmd_handoff(path: &std::path::Path, fields: u64, limit: u64) -> Result<()> {
    println!("asking {} which handoff fields it uses", path.display());
    println!("  one run per field, poisoned with an address nothing maps\n");

    let mut used = Vec::new();
    for field in 0..fields {
        let verdict = ask_about_field(path, field, limit)?;
        match &verdict {
            Used::Yes { kind, site } => {
                // The kind carries its own preposition ("read of", "instruction fetch from"), as in
                // the fault report.
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
    // The entry argument is selected as the handoff structure, not whatever the configuration names
    // (D399).
    //
    // SAFETY: single-threaded here; the child reads both variables at startup, and both are removed
    // below so a later run is not under them.
    unsafe { std::env::set_var(orbistoun_env::ENTRY_ARGUMENT.name, "handoff") };
    // SAFETY: as above.
    unsafe { std::env::set_var(orbistoun_env::HANDOFF_POISON.name, field.to_string()) };
    let outcome = run_quietly(path, limit);
    // SAFETY: as above.
    unsafe { std::env::remove_var(orbistoun_env::HANDOFF_POISON.name) };
    // SAFETY: as above.
    unsafe { std::env::remove_var(orbistoun_env::ENTRY_ARGUMENT.name) };
    outcome?;

    // The worker's own constants, so this matches the exact address the worker planted.
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
            relink: false,
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
    /// Every distinct first argument across the run, so the line can say when `arg0` was not the
    /// only one.
    pub(crate) firsts: std::collections::BTreeSet<u64>,
}

#[cfg(test)]
mod link_tests {
    /// A link prints its counts and digest, and a mismatch is called a loader defect.
    #[test]
    fn a_link_is_described_in_lines() {
        let summary = orbistoun_proto::LinkSummary {
            digest: "0123456789abcdef".to_owned(),
            modules: 3,
            writes: 564_184,
            syscalls: 1,
            stored: "mismatch".to_owned(),
            differs: vec!["libc::malloc at 0x10: stored 0x1, now 0x2".to_owned()],
        };
        let lines = super::describe_link(&summary, false);
        assert_eq!(lines[0], "modules  3");
        assert_eq!(lines[1], "writes   564184");
        assert_eq!(lines[2], "syscalls 1");
        assert_eq!(lines[3], "digest   0123456789abcdef");
        assert_eq!(lines[4], "stored   mismatch");
        assert!(lines[5].contains("loader defect"));
        assert_eq!(lines[6], "  libc::malloc at 0x10: stored 0x1, now 0x2");
    }
}
