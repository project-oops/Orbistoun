//! What to implement next: `worklist`.

use crate::common::library_or;
use orbistoun_service::Service;

/// Ranks the calls a guest made to the kernel directly, which no import list can show.
///
/// # Why this is a section of its own
///
/// A guest reaching the kernel by number touches no stub, so it contributes nothing to the
/// ranked imports above. That is not an edge case: it is how every open-toolchain payload
/// works - resolve one function to build a gadget, then go straight to the kernel - so a run
/// that stopped dead on an unimplemented call could report nothing of interest and be telling
/// the truth (D401).
///
/// Ranked by **how many runs asked**, not by how often. The recorder is a bitmap and knows only
/// that a number came up; a call count would be a number nobody measured.
/// What the census has gathered about one direct syscall number: how many runs asked, the name
/// where one is known, the first argument, and the set of guests that issued it. The last is what
/// keeps a number attributed to the guest whose trace carried it rather than the title just run.
type KernelCall = (
    usize,
    Option<String>,
    Option<u64>,
    std::collections::BTreeSet<String>,
);

fn report_kernel_calls(kernel: &std::collections::BTreeMap<u64, KernelCall>) {
    let unserved: Vec<_> = kernel
        .iter()
        .filter(|(_, (_, name, _, _))| name.is_none())
        .collect();
    if unserved.is_empty() {
        return;
    }

    let mut ranked = unserved;
    ranked.sort_unstable_by_key(|(number, (runs, _, _, _))| (std::cmp::Reverse(*runs), **number));

    println!(
        "
{} system call(s) asked for directly that nothing here implements",
        ranked.len()
    );
    // **`ASKED BY` names the guest, not the title just run.** This census totals every module's
    // trace, so a number here belongs to whichever guest's trace carried it - reading it as the
    // title the command named is how syscall 601 was twice taken for a retail title's wall when
    // it was a homebrew payload's log write (worklog 727).
    println!(
        "{:>6}  {:>5}  {:<14}  ASKED BY",
        "CALL", "RUNS", "FIRST ARGUMENT"
    );
    for (number, (runs, _, argument, who)) in &ranked {
        // The argument is shown because for a call nobody can name it is most of what there is
        // to go on - a number alone says which entry to write, and the argument starts to say
        // what it is for.
        let argument = argument.map_or_else(|| "-".to_owned(), |a| format!("{a:#x}"));
        let who = who.iter().cloned().collect::<Vec<_>>().join(", ");
        println!("{number:>6}  {runs:>5}  {argument:<14}  {who}");
    }
}

/// What is imported and unimplemented, grouped by where an answer can come from.
///
/// **The counterpart to [`cmd_worklist`], and deliberately not a replacement.** That one totals
/// the call traces, which is a fact about runs that have happened; this reads the import tables,
/// which is a fact about what the guests would need if they got that far. For a published
/// interface the second is enough to act on - the standard is the oracle, so the function can be
/// written and tested without any guest reaching it - and waiting for a run to trip over one is
/// a session spent per function (D472).
pub(crate) fn cmd_worklist_static(service: &Service, top: usize) {
    use orbistoun_hle::origin::{self, Origin};
    use std::collections::{BTreeMap, BTreeSet};

    let implemented: BTreeSet<String> = service
        .declared_symbols()
        .iter()
        .filter(|d| d.implemented)
        .map(|d| d.symbol.clone())
        .collect();
    // **Implemented is not the same as finished.** A function that answers the easy case and
    // gives up counts as present here and is not, so the ones that declare what they do not do
    // are counted separately rather than folded into the same number (D474).
    let knowledge = orbistoun_hle::knowledge::Knowledge::builtin();
    let mut partial: BTreeMap<String, String> = BTreeMap::new();

    let mut modules = 0_usize;
    let mut unnamed: BTreeMap<String, usize> = BTreeMap::new();
    // Distinct by name, because the same function imported by four titles is one job.
    let mut missing: BTreeMap<Origin, BTreeMap<String, BTreeSet<String>>> = BTreeMap::new();
    let mut needed: BTreeMap<Origin, BTreeSet<String>> = BTreeMap::new();
    // Counted flat as well as per library, because a symbol two libraries both name is **one
    // job**, and summing the per-library sets reported more missing than needed - a number that
    // is impossible on its face, which is the kind a report must not print (principle 3).
    let mut missing_names: BTreeMap<Origin, BTreeSet<String>> = BTreeMap::new();

    for entry in std::fs::read_dir(library_or(None))
        .into_iter()
        .flatten()
        .flatten()
    {
        let module = entry.path().join("eboot.bin");
        if !module.is_file() {
            continue;
        }
        let Ok(survey) = service.survey_path(&module) else {
            continue;
        };
        modules += 1;
        for import in &survey.imports {
            let library = import.library.as_deref().unwrap_or("?");
            let Some(symbol) = import.symbol.as_deref() else {
                *unnamed.entry(library.to_owned()).or_default() += 1;
                continue;
            };
            let where_from = origin::of(library, symbol);
            needed
                .entry(where_from)
                .or_default()
                .insert(symbol.to_owned());
            if implemented.contains(symbol)
                && let Some(known) = knowledge.get(symbol)
                && !known.partial.is_empty()
            {
                partial.insert(symbol.to_owned(), known.partial.clone());
            }
            if !implemented.contains(symbol) {
                missing_names
                    .entry(where_from)
                    .or_default()
                    .insert(symbol.to_owned());
                missing
                    .entry(where_from)
                    .or_default()
                    .entry(library.to_owned())
                    .or_default()
                    .insert(symbol.to_owned());
            }
        }
    }

    print_static_gap(modules, &needed, &missing_names, &partial);

    print_static_gap_lists(&missing, &unnamed, top);
}

/// The counted half of the static gap report: how many of each kind, and what is unfinished.
///
/// Split from [`cmd_worklist_static`] because the two halves have nothing to say to each
/// other - one gathers, one prints - and together they were past the length this workspace
/// lints for.
fn print_static_gap(
    modules: usize,
    needed: &std::collections::BTreeMap<
        orbistoun_hle::origin::Origin,
        std::collections::BTreeSet<String>,
    >,
    missing_names: &std::collections::BTreeMap<
        orbistoun_hle::origin::Origin,
        std::collections::BTreeSet<String>,
    >,
    partial: &std::collections::BTreeMap<String, String>,
) {
    use orbistoun_hle::origin::Origin;
    use std::collections::BTreeSet;

    println!("{modules} modules, by where an answer has to come from");
    println!();
    for (where_from, label) in [
        (
            Origin::Documented,
            "documented - write in bulk, no guest needed",
        ),
        (
            Origin::TitleOwn,
            "the title's own modules - load them, do not implement",
        ),
        (
            Origin::Vendor,
            "vendor - the guest is the only oracle, so one at a time",
        ),
    ] {
        let have = needed.get(&where_from).map_or(0, BTreeSet::len);
        let gap = missing_names.get(&where_from).map_or(0, BTreeSet::len);
        println!("  {have:>5} needed, {gap:>5} missing   {label}");
    }

    println!();
    if partial.is_empty() {
        println!("nothing is declared partial - which is not the same as everything being");
        println!("complete, only that no incompleteness has been written down.");
    } else {
        println!(
            "{} implemented but declared partial - present, and not finished:",
            partial.len()
        );
        for (name, what) in partial {
            println!("  {name}");
            println!("      {what}");
        }
    }
}

/// The listed half: the documented gap by library, and what has no name at all.
fn print_static_gap_lists(
    missing: &std::collections::BTreeMap<
        orbistoun_hle::origin::Origin,
        std::collections::BTreeMap<String, std::collections::BTreeSet<String>>,
    >,
    unnamed: &std::collections::BTreeMap<String, usize>,
    top: usize,
) {
    use orbistoun_hle::origin::Origin;

    if let Some(by_lib) = missing.get(&Origin::Documented) {
        println!();
        println!("the documented gap, by library - this is the batch list");
        for (library, names) in by_lib {
            let mut sorted: Vec<&String> = names.iter().collect();
            sorted.sort();
            println!("  {library} ({} missing)", sorted.len());
            for name in sorted.iter().take(top) {
                println!("    {name}");
            }
            if sorted.len() > top {
                // Said out loud rather than truncated silently: a list that stops without
                // saying so reads as a list that ended (principle 3).
                println!(
                    "    ... and {} more (--top to see further)",
                    sorted.len() - top
                );
            }
        }
    }
    if !unnamed.is_empty() {
        println!();
        let total: usize = unnamed.values().sum();
        println!("{total} imports have no name yet, so nothing can be said about them:");
        for (library, count) in unnamed {
            println!("  {count:>5}  {library}");
        }
    }
}

/// Guests this collection builds, one name per line - see the file's own header for why it is a
/// list and not a rule.
const OUR_GUESTS: &str = include_str!("../data/our-guests.txt");

/// Whether a trace came from a guest we wrote.
///
/// Matched on **path components**, not as a substring: a module is
/// `titles/PPSA99980/eboot.bin` or `.../oops-apps/home/dist/eboot.bin`, and a bare `contains`
/// would let a short name like `dist` match an unrelated path that happens to contain it.
///
/// A guest missing from the list reads as third-party, which is the safe direction - it can
/// understate how much of a ranking is our own noise and never overstate it.
fn is_our_guest(module: &str) -> bool {
    let ours: Vec<&str> = OUR_GUESTS
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    module
        .split(['/', std::path::MAIN_SEPARATOR])
        .any(|part| ours.contains(&part))
}

/// A short, recognisable name for the guest a trace came from.
///
/// The directory the eboot sits in, which is the title id for a retail title (`PPSA02664-app0`)
/// and the payload name for one of ours (`dist`). Used to say **which guest** asked for a direct
/// syscall in the census, so a number there is read against the guest that issued it rather than
/// against whichever title the `run` command happened to name - the aggregate census totals every
/// module's trace, and a syscall in it need not belong to the title just run (worklog 727).
fn guest_label(module: &str) -> String {
    let parts: Vec<&str> = module
        .split(['/', std::path::MAIN_SEPARATOR])
        .filter(|p| !p.is_empty())
        .collect();
    match parts.as_slice() {
        // The eboot's parent directory is the recognisable name; the file itself is always
        // `eboot.bin`, which names nothing.
        [.., parent, _file] => (*parent).to_owned(),
        [only] => (*only).to_owned(),
        _ => module.to_owned(),
    }
}

/// One ranked import table, printed with the share each row is of its own group.
///
/// **Shares are within the group, deliberately.** A row's percentage of a corpus that mixes our
/// probes with retail titles is a number about which guests happened to run, which is the thing
/// splitting the table exists to stop reporting.
fn print_ranked(totals: &std::collections::BTreeMap<String, (u64, usize)>, top: usize) {
    let mut ranked: Vec<(&String, u64, usize)> =
        totals.iter().map(|(l, (c, m))| (l, *c, *m)).collect();
    ranked.sort_unstable_by_key(|(label, calls, _)| (std::cmp::Reverse(*calls), (*label).clone()));
    let grand: u64 = ranked.iter().map(|(_, c, _)| *c).sum();

    println!("{:>14}  {:>5}  {:>7}  IMPORT", "CALLS", "SHARE", "MODULES");
    for (label, calls, in_modules) in ranked.iter().take(top) {
        let tenths = calls.saturating_mul(1000).checked_div(grand).unwrap_or(0);
        let share = format!("{}.{}", tenths / 10, tenths % 10);
        println!("{calls:>14}  {share:>4}%  {in_modules:>7}  {label}");
    }
    if ranked.len() > top {
        println!(
            "\n... and {} more (--top to see further)",
            ranked.len() - top
        );
    }
}

pub(crate) fn cmd_worklist(top: usize) {
    let paths = orbistoun_paths::Paths::resolve();
    let dir = paths.traces_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        println!("no traces yet at {}", dir.display());
        println!("run a guest first:  ./bin/orbistoun crunch");
        return;
    };

    // Totalled by label rather than by index. A stub index is per-module - index 260 is
    // a different function in every title - so summing by index would produce confident
    // nonsense the moment a second module was involved.
    let mut totals: std::collections::BTreeMap<String, (u64, usize)> =
        std::collections::BTreeMap::new();
    // **A second table, because one answers a question nobody asked.** Our own probes are built
    // to exercise everything, so they out-call a title that dies early and dominate a combined
    // ranking - which is how `sceKernelDlsym` came to head this report at 27% of all calls while
    // every retail title calls it exactly once (worklog 542).
    let mut ours: std::collections::BTreeMap<String, (u64, usize)> =
        std::collections::BTreeMap::new();
    let mut our_runs = 0;
    // Kept apart from the imports above, and counted differently on purpose. The recorder is a
    // bitmap: it knows a number was asked for, not how many times. Ranking these by call volume
    // would mean inventing the volume, so they rank by **how many runs wanted them** - which is
    // a fact, and is the right question anyway for something that blocks a payload outright.
    let mut kernel: std::collections::BTreeMap<u64, KernelCall> = std::collections::BTreeMap::new();
    let mut modules = 0;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(trace) = serde_json::from_str::<orbistoun_report::trace::CallTrace>(&text) else {
            eprintln!("note: {} is not a call trace, skipping", path.display());
            continue;
        };
        modules += 1;
        let mine = is_our_guest(&trace.module);
        if mine {
            our_runs += 1;
        }
        let into = if mine { &mut ours } else { &mut totals };
        for call in &trace.calls {
            let entry = into.entry(call.label.clone()).or_insert((0, 0));
            entry.0 = entry.0.saturating_add(call.calls);
            entry.1 += 1;
        }
        for asked in &trace.syscalls {
            let seen = kernel
                .entry(asked.number)
                .or_insert_with(|| (0, None, None, std::collections::BTreeSet::new()));
            seen.0 += 1;
            if seen.1.is_none() {
                seen.1.clone_from(&asked.name);
            }
            if seen.2.is_none() {
                seen.2 = asked.first_argument;
            }
            seen.3.insert(guest_label(&trace.module));
        }
    }

    if totals.is_empty() && ours.is_empty() && kernel.is_empty() {
        println!("no imports recorded across {modules} traces");
        return;
    }

    let retail_runs = modules - our_runs;
    println!(
        "{} distinct imports across {retail_runs} title run(s); {} across {our_runs} run(s) of guests we wrote\n",
        totals.len(),
        ours.len()
    );

    // **What this ranks, said outright.** Call volume answers *what is missing*, and the question
    // a reader brings is *what is blocking* - which is a different question with a different
    // answer. On 2026-09-14 the top four candidates this report offered were each tested with
    // `ORBISTOUN_RETURN` and **none of them was a wall**: PPSA21564 answers 500,258 of its 500,260
    // calls with a real implementation and still faults in its own code (worklog 562).
    println!("Ranked by how often a guest called it - which is what is MISSING, not what is");
    println!("BLOCKING. Before implementing one, answer it without implementing it:");
    println!("  ORBISTOUN_RETURN=<function>:0x0 ./bin/orbistoun run <title>");
    println!(
        "A wall moves. Four of four candidates tested this way moved nothing (worklog 562).\n"
    );

    println!("TITLES WE DID NOT WRITE");
    if totals.is_empty() {
        println!("  (none - no retail run has been traced)");
    } else {
        print_ranked(&totals, top);
    }

    // Separated rather than excluded: our probes exercise the platform deliberately and what they
    // reach is worth seeing - it just cannot share a ranking with titles, because they are built
    // to call everything and a title is not (worklog 542).
    println!("\nGUESTS WE WROTE - probes, built to exercise everything");
    if ours.is_empty() {
        println!("  (none traced)");
    } else {
        print_ranked(&ours, top);
    }

    report_kernel_calls(&kernel);

    // Counted across both tables: an unnamed hash is a gap in the symbol database, and which
    // kind of guest happened to call it says nothing about that.
    let all_labels = totals.keys().chain(ours.keys());
    let named: Vec<&String> = all_labels.collect();
    let unnamed = named.iter().filter(|l| l.contains("::0x")).count();
    println!(
        "
{} of {} still have no name - extend crates/orbistoun-names/data/vendor.toml",
        unnamed,
        named.len()
    );
}

#[cfg(test)]
mod tests {

    /// **Our own guests are told apart by name, and the hard case is the one that looks retail.**
    ///
    /// `PPSA99980` is obSCEne's homebrew registered under a retail-shaped title id. Any rule that
    /// reads the shape of a name gets it wrong, which is why the list is curated - and why this
    /// asserts the awkward case rather than the obvious ones.
    #[test]
    fn our_own_guests_are_recognised_including_the_one_shaped_like_a_title() {
        for module in [
            "titles/obscene/eboot.bin",
            "titles/obscene-payload/eboot.bin",
            "titles/PPSA99980/eboot.bin",
            // An absolute path, because a trace records one - written generically, since a real
            // machine path must never be committed.
            "/build/oops-apps/home/dist/eboot.bin",
        ] {
            assert!(super::is_our_guest(module), "{module} is ours");
        }
    }

    /// **A title is not ours, and a near-miss is not either.**
    ///
    /// The second case is what path-component matching buys over `contains`: a directory whose name
    /// merely ends in one of ours must not be swept up, or the split silently understates the
    /// retail table - the exact failure the split exists to correct.
    #[test]
    fn titles_we_did_not_write_are_not_claimed() {
        for module in [
            "titles/PPSA25872-app0/eboot.bin",
            "titles/PPSA02664-app0/eboot.bin",
            "titles/not-obscene/eboot.bin",
            "titles/distributed-thing/eboot.bin",
        ] {
            assert!(!super::is_our_guest(module), "{module} is not ours");
        }
    }

    /// **A direct syscall is labelled by the guest that issued it, not the title just run.**
    ///
    /// This is the guard on the census misattribution worklog 727 records: syscall 601 was a
    /// homebrew payload's log write, read twice as a retail title's wall because the aggregate
    /// census does not say whose trace a number came from. `guest_label` is what now says it, so
    /// the case that matters is the *probe* path resolving to the payload name (`dist`) - never to
    /// the title the command happened to name.
    #[test]
    fn a_guest_is_labelled_by_its_own_eboot_directory() {
        // The payload that actually issues syscall 601 - it must read as `dist`, not as any title.
        assert_eq!(
            super::guest_label("/build/oops-apps/home/dist/eboot.bin"),
            "dist"
        );
        // A retail title reads as its own id, from the same rule.
        assert_eq!(
            super::guest_label("titles/PPSA02664-app0/eboot.bin"),
            "PPSA02664-app0"
        );
        // Windows separators, as a real trace on this platform records them.
        assert_eq!(
            super::guest_label("C:\\x\\oops-apps\\home\\dist\\eboot.bin"),
            "dist"
        );
    }
}
