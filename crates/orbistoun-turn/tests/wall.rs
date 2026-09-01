//! Sweeping a real wall with a real guest.
//!
//! ```text
//! cargo test -p orbistoun-turn --release --test wall -- --ignored --nocapture
//! ```
//!
//! Opt-in, because it boots a commercial title thirteen times. Needs a release build of
//! `orbistoun-cli` and a title on disk; both are checked for and the test says which is
//! missing rather than failing obscurely.
//!
//! # The target
//!
//! `libkernel::0x6abac2f3dc6f8cee`, called immediately before the fault at
//! `image+0xafc959` in PPSA02664 and PPSA03416. `docs/PROJECT_STATUS.md` records what is
//! known: `arg1 = 0x100000`, the same megabyte as `rdx`; `arg3 = 0x40000` alignment; and
//! `0x100000 - 0x20 = 0xfffe0` is the faulting address exactly. The guest indexes
//! `base + size - 0x20` from a base of **zero**, and that zero is neither the stub's
//! return nor unwritten memory.
//!
//! So the question is which argument holds the base it expected filled - and that is
//! what a sweep answers, by planting a sentinel in each and seeing which one the fault
//! follows.
//!
//! # Its own data directory
//!
//! Traces go to a temporary root rather than the machine's, so thirteen runs do not
//! overwrite whatever was there and the newest-trace rule cannot pick up an unrelated
//! run that happened to overlap.

use orbistoun_turn::experiment::{Finding, investigate, sweep};
use orbistoun_turn::trial::{GuestTrial, traces_in};

/// The import called immediately before the fault.
///
/// **By name, and it used to be by hash.** A target matches against an import's label, so
/// `0x6abac2f3dc6f8cee` addressed this function for as long as nothing had named it - and
/// stopped the moment the string harvester did, because the label became
/// `libkernel::sceKernelReserveVirtualRange` and no longer contains the hash. The sweep then
/// planted nothing twenty-four times and said so (D287).
const TARGET: &str = "sceKernelReserveVirtualRange";

/// The title whose wall this is.
const TITLE: &str = "../../titles/PPSA02664-app0/eboot.bin";

/// A release build, because thirteen debug boots is not a loop anybody waits for.
const BINARY: &str = "../../target/release/orbistoun-cli.exe";

#[test]
#[ignore = "boots a commercial title thirteen times; opt-in via --ignored"]
fn sweep_the_live_wall() {
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

    eprintln!("WALL  target={TARGET}\n title={TITLE}");
    let (finding, outcomes) = investigate(&mut trial, TARGET).expect("the sweep runs");

    // Zipped with the sweep that produced them rather than derived from the index: the
    // arithmetic assumed two runs per slot, and a second dimension made it four - so it
    // reported slots 8 to 11 on a six-argument call (D286).
    for (experiment, outcome) in sweep(TARGET).iter().zip(outcomes.iter()) {
        eprintln!(
            "  arg{} sentinel {:#x}{}: fault={:?} planted={} refused={}",
            experiment.slot,
            experiment.value,
            experiment
                .answer
                .map_or(String::new(), |a| format!(" answering {a:#x}")),
            outcome.fault.map(|f| format!("{f:#x}")),
            outcome.planted,
            outcome.refused
        );
    }

    eprintln!("\nWALL RESULT  {finding:?}");
    match &finding {
        Finding::OutParameter {
            slot,
            offset,
            answer,
        } => eprintln!(
            "  *** arg{slot} is the out-parameter. The guest faults at arg{slot} {} {:#x}{}.",
            if *offset < 0 { "-" } else { "+" },
            offset.unsigned_abs(),
            // Stated, because a finding that needed the call forced to succeed is a different
            // and weaker claim than one that held whatever it answered (D286).
            answer.map_or(String::new(), |a| format!(
                ", but only when the call answers {a:#x}"
            ))
        ),
        Finding::Dereferenced { slot } => eprintln!(
            concat!(
                "  arg{0} is followed as a pointer, and nothing more. The fault landed ",
                "on the sentinel itself rather than an offset from it, which is what ",
                "overwriting any live pointer looks like - the sweep breaking the ",
                "program, not finding what was missing."
            ),
            slot
        ),
        Finding::Derailed {
            slot,
            touched,
            reached,
            was,
        } => eprintln!(
            concat!(
                "  arg{0} derailed the run rather than moving an address - it reached ",
                "{1} distinct imports against {2}, and the fault {3} an address it ",
                "asked for. Nothing here is a statement about where the base comes from."
            ),
            slot,
            reached,
            was,
            if *touched { "is still" } else { "is no longer" }
        ),
        Finding::Moved { slot } => eprintln!(
            concat!(
                "  arg{0} changed the fault, but not by a consistent offset - something ",
                "downstream moved rather than the address being computed from it."
            ),
            slot
        ),
        Finding::Unmoved {
            tested,
            not_addresses,
        } => {
            eprintln!("  the base it expects is not reached through any argument of this call.");
            eprintln!("    planted, fault unchanged : {tested:?}");
            eprintln!("    not writable addresses   : {not_addresses:?}");
        }
        Finding::Escaped { slot, reached, was } => eprintln!(
            "  *** planting arg{slot} broke the loop: reached {reached} against {was}, nothing faulted."
        ),
        Finding::NeverPlanted => eprintln!(concat!(
            "  no write ever landed, so nothing was measured. Check the target matches ",
            "an import the guest actually calls."
        )),
    }

    // **Deliberately not asserted.** Which slot it is, or whether it is any of them, is a
    // fact about the guest rather than about this code - a test that failed on it would
    // be reporting the wrong thing, and `Unmoved` is a real and useful result.
    //
    // What is asserted is that the experiment happened: a sweep where nothing planted has
    // measured nothing, and reporting that as "not this function" is the failure the
    // whole `NeverPlanted` distinction exists to prevent.
    assert!(
        !matches!(finding, Finding::NeverPlanted),
        "no write reached {TARGET} - the sweep measured nothing"
    );
}
