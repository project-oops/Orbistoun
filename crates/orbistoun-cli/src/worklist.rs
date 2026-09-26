//! What to implement next: `worklist`.

use crate::common::library_or;
use orbistoun_service::Service;

/// What the census has gathered about one direct syscall number: how many runs asked, the name
/// where one is known, the first argument, and the guests whose traces carried it.
type KernelCall = (
    usize,
    Option<String>,
    Option<u64>,
    std::collections::BTreeSet<String>,
);

/// Ranks the calls a guest made to the kernel directly, which no import list can show.
///
/// A guest reaching the kernel by number touches no stub, so it contributes nothing to the ranked
/// imports; open-toolchain payloads work this way (D401). Ranked by how many runs asked, because
/// the recorder is a bitmap and has no call count.
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
    // `ASKED BY` names the guest whose trace carried the number. The census totals every module's
    // trace, so a number need not belong to the title just run.
    println!(
        "{:>6}  {:>5}  {:<14}  ASKED BY",
        "CALL", "RUNS", "FIRST ARGUMENT"
    );
    for (number, (runs, _, argument, who)) in &ranked {
        // For a call nobody can name, the argument is most of what there is to go on.
        let argument = argument.map_or_else(|| "-".to_owned(), |a| format!("{a:#x}"));
        let who = who.iter().cloned().collect::<Vec<_>>().join(", ");
        println!("{number:>6}  {runs:>5}  {argument:<14}  {who}");
    }
}

/// What is imported and unimplemented, grouped by where an answer can come from.
///
/// The counterpart to [`cmd_worklist`], not a replacement: that one totals call traces from runs
/// that happened; this reads the import tables. For a published interface the standard is the
/// oracle, so a function can be written and tested before any guest reaches it (D472).
pub(crate) fn cmd_worklist_static(service: &Service, top: usize) {
    use orbistoun_hle::origin::{self, Origin};
    use std::collections::{BTreeMap, BTreeSet};

    let implemented: BTreeSet<String> = service
        .declared_symbols()
        .iter()
        .filter(|d| d.implemented)
        .map(|d| d.symbol.clone())
        .collect();
    // Implemented is not finished: functions that declare what they do not do are counted
    // separately (D474).
    let knowledge = orbistoun_hle::knowledge::Knowledge::builtin();
    let mut partial: BTreeMap<String, String> = BTreeMap::new();

    let mut modules = 0_usize;
    let mut unnamed: BTreeMap<String, usize> = BTreeMap::new();
    // Distinct by name: the same function imported by several titles is one job.
    let mut missing: BTreeMap<Origin, BTreeMap<String, BTreeSet<String>>> = BTreeMap::new();
    let mut needed: BTreeMap<Origin, BTreeSet<String>> = BTreeMap::new();
    // Counted flat as well as per library: a symbol two libraries both name is one job, and summing
    // the per-library sets would over-count.
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
/// Split from [`cmd_worklist_static`]: one half gathers, the other prints.
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
                // Said out loud: a list truncated silently reads as a list that ended.
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

/// Guests this collection builds, one name per line; the file's header says why it is a list and
/// not a rule.
const OUR_GUESTS: &str = include_str!("../data/our-guests.txt");

/// Whether a trace came from a guest we wrote.
///
/// Matched on path components, not as a substring, so a short name like `dist` does not match an
/// unrelated path containing it. A guest missing from the list reads as third-party, which can
/// understate our share of a ranking but never overstate it.
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
/// The directory the eboot sits in: the title id for a retail title (`PPSA02664-app0`) and the
/// payload name for one of ours (`dist`). The census attributes each direct syscall with it,
/// because the aggregate census totals every module's trace.
fn guest_label(module: &str) -> String {
    let parts: Vec<&str> = module
        .split(['/', std::path::MAIN_SEPARATOR])
        .filter(|p| !p.is_empty())
        .collect();
    match parts.as_slice() {
        // The eboot's parent directory; the file itself is always `eboot.bin`.
        [.., parent, _file] => (*parent).to_owned(),
        [only] => (*only).to_owned(),
        _ => module.to_owned(),
    }
}

/// One ranked import table, printed with the share each row is of its own group.
///
/// Shares are within the group: a share of a corpus mixing our probes with retail titles depends on
/// which guests happened to run.
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

/// `worklist` - rank what to implement next, across every run so far.
pub(crate) fn cmd_worklist(top: usize) {
    let paths = orbistoun_paths::Paths::resolve();
    let dir = paths.traces_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        println!("no traces yet at {}", dir.display());
        println!("run a guest first:  ./bin/orbistoun crunch");
        return;
    };

    // Totalled by label, not by index: a stub index is per-module, so summing by index is wrong as
    // soon as a second module is involved.
    let mut totals: std::collections::BTreeMap<String, (u64, usize)> =
        std::collections::BTreeMap::new();
    // A second table for our own guests: probes are built to exercise everything, so they out-call
    // a title that stops early and would dominate a combined ranking.
    let mut ours: std::collections::BTreeMap<String, (u64, usize)> =
        std::collections::BTreeMap::new();
    let mut our_runs = 0;
    // Kept apart from the imports and ranked by how many runs asked, because the recorder is a
    // bitmap and has no call volume.
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

    // This ranks what is missing by call volume, which is not the same as what is blocking: a title
    // can answer nearly every call with a real implementation and still fault in its own code.
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

    // Separated rather than excluded: what probes reach is worth seeing, but they are built to call
    // everything and cannot share a ranking with titles.
    println!("\nGUESTS WE WROTE - probes, built to exercise everything");
    if ours.is_empty() {
        println!("  (none traced)");
    } else {
        print_ranked(&ours, top);
    }

    report_kernel_calls(&kernel);

    // Counted across both tables: an unnamed hash is a gap in the symbol database whichever guest
    // called it.
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

    /// Our own guests are told apart by name, including one with a retail-shaped title id.
    ///
    /// `PPSA99980` is obSCEne's homebrew under a retail-shaped id, so no rule over the shape of a
    /// name works; the list is curated.
    #[test]
    fn our_own_guests_are_recognised_including_the_one_shaped_like_a_title() {
        for module in [
            "titles/obscene/eboot.bin",
            "titles/obscene-payload/eboot.bin",
            "titles/PPSA99980/eboot.bin",
            // An absolute path, as a trace records one, written generically.
            "/build/oops-apps/home/dist/eboot.bin",
        ] {
            assert!(super::is_our_guest(module), "{module} is ours");
        }
    }

    /// A title is not ours, and a near-miss is not either.
    ///
    /// A directory whose name merely ends in one of ours is not swept up, which is what
    /// path-component matching buys over `contains`.
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

    /// A direct syscall is labelled by the guest that issued it, not the title just run.
    ///
    /// The probe path resolves to the payload name (`dist`), never to the title the command named.
    #[test]
    fn a_guest_is_labelled_by_its_own_eboot_directory() {
        // A payload path reads as `dist`, not as any title.
        assert_eq!(
            super::guest_label("/build/oops-apps/home/dist/eboot.bin"),
            "dist"
        );
        // A retail title reads as its own id, from the same rule.
        assert_eq!(
            super::guest_label("titles/PPSA02664-app0/eboot.bin"),
            "PPSA02664-app0"
        );
        // Windows separators, as a trace on that platform records them.
        assert_eq!(
            super::guest_label("C:\\x\\oops-apps\\home\\dist\\eboot.bin"),
            "dist"
        );
    }
}
