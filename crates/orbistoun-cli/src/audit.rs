//! The symbol database: `harvest` and `audit`.

use crate::common::how_it_was_found;
use anyhow::{Context, Result};

/// Collects every `Symbol.map` beneath a directory.
///
/// Hand-rolled rather than pulling in a directory-walking crate: the whole job is
/// "recurse and match one filename", and a dependency that does it would be more code
/// to audit than the code it replaces.
fn find_symbol_maps(root: &std::path::Path, found: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        // An unreadable directory costs its own symbols, not the run. A harvest that
        // fails wholesale because one path is inaccessible is worse than a partial one
        // that says how many it read.
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            find_symbol_maps(&path, found);
        } else if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(orbistoun_names::harvest::is_version_script)
        {
            // **Asked rather than re-decided.** This tested the file name against
            // `"Symbol.map"` while `is_version_script` - written to fix exactly that, after
            // it cost every `pthread_*` name (D127) - sat unused in the crate below.
            //
            // Two implementations of one rule, one of them fixed. `lib/libthr` calls its
            // file `pthread.map` and `lib/libsys` calls its `Symbol.sys.map`, so this
            // walker silently skipped both and reported success either way (D191).
            found.push(path);
        }
    }
}

/// `harvest` - rebuild the standard-library word list from a FreeBSD source tree.
pub(crate) fn cmd_harvest(
    source: &std::path::Path,
    out: &std::path::Path,
    revision: Option<&str>,
) -> Result<()> {
    anyhow::ensure!(
        source.is_dir(),
        "{} is not a directory - point this at a FreeBSD source checkout",
        source.display()
    );

    let mut maps = Vec::new();
    for library in orbistoun_names::harvest::FREEBSD_LIBRARY_PATHS {
        let path = source.join(library);
        if path.is_dir() {
            find_symbol_maps(&path, &mut maps);
        } else {
            // Named rather than silently skipped: a sparse checkout missing one library
            // yields a smaller list, and the reader should know which.
            eprintln!("note: {} is not present, skipping", path.display());
        }
    }
    anyhow::ensure!(
        !maps.is_empty(),
        "no Symbol.map files found under {} - is this a FreeBSD source tree?",
        source.display()
    );
    maps.sort();

    let mut names = std::collections::BTreeSet::new();
    for map in &maps {
        let text =
            std::fs::read_to_string(map).with_context(|| format!("reading {}", map.display()))?;
        for symbol in orbistoun_names::harvest::parse_symbol_map(&text) {
            names.insert(symbol.name);
        }
    }

    // A revision is a citation; a local path is not. When both exist the revision leads
    // and the path is dropped entirely - nobody re-deriving this has the same directory,
    // and a temporary one actively misleads.
    let described = match revision {
        Some(revision) => revision.to_owned(),
        None => format!("{} (no revision given)", source.display()),
    };
    let names: Vec<String> = names.into_iter().collect();
    let text = orbistoun_names::harvest::render(&names, &described, &orbistoun_nid::today());
    std::fs::write(out, text).with_context(|| format!("writing {}", out.display()))?;

    println!(
        "harvested {} names from {} symbol maps into {}",
        names.len(),
        maps.len(),
        out.display()
    );
    if revision.is_none() {
        // The point of harvesting is citability, and a path with no revision is only
        // half a citation.
        eprintln!("note: pass --revision to record which revision this came from");
    }
    Ok(())
}

/// Re-derives generated records that the current grammar no longer confirms.
///
/// # The record shape that fights the loop it belongs to
///
/// A generated record is `pattern` plus `index`, and that is what makes checking it a
/// microsecond rather than a full sweep. It is also what makes it fragile: an index is a
/// position in a mixed-radix enumeration over the vocabularies, so **adding one word to a
/// vocabulary renumbers every candidate built from it**.
///
/// Which is a problem, because adding words is the loop. Every confirmed name is split
/// into parts and fed back into the grammar so the next search reaches further (D195) - and
/// each time that happens, some already-recorded name stops being at the index its record
/// names. Nothing was wrong with the name and nothing was wrong with the claim; the
/// coordinates moved underneath it.
///
/// Before this, those names fell off the verified count onto the unaccounted ceiling - a
/// file whose whole rule is that it may only shrink. One learning pass of thirty-seven
/// words moved twenty names onto it. Left alone the ceiling would have grown on every
/// successful search, which is the exact opposite of what it is for (D213).
///
/// **A repair knows the name, so it never searches.** `Pattern::index_of` inverts the
/// mixed-radix encoding that produced it, so new coordinates are arithmetic per name. This
/// used to hand the hashes to the generative sweep and hunt for names it already had - five
/// hours on thirty-three records, unfinished; the same work is now fourteen seconds (D304).
///
/// The date is never touched. A record says when a name was *first* worked out, and that
/// did not change; only where the current grammar produces it did.
fn repair_generated_records(
    file: &mut orbistoun_nid::SymbolDbFile,
    patterns: &[orbistoun_names::Pattern],
    standard: &[String],
) -> usize {
    let stale: Vec<String> = file
        .names
        .iter()
        .filter(|name| {
            file.derivations.get(*name).is_some_and(|d| {
                matches!(d.method, orbistoun_nid::Method::Generated { .. })
                    && !orbistoun_names::solve::verify(
                        name,
                        d,
                        patterns,
                        standard,
                        // This filter has already narrowed to `Generated`, so no affix
                        // rule can be consulted and none is offered.
                        &orbistoun_names::affix::Affixes::default(),
                    )
            })
        })
        .cloned()
        .collect();
    if stale.is_empty() {
        return 0;
    }

    println!();
    println!(
        concat!(
            "{} generated record(s) no longer hold - the grammar moved underneath them. ",
            "Re-deriving:"
        ),
        stale.len()
    );

    // **Derived from the name, not searched for by hash.** This ran `solve_patterns` - the
    // full generative sweep, hashing every candidate across a space measured in trillions,
    // looking for NIDs it already had names for. Five hours on thirty-three names, and it did
    // not finish.
    //
    // A repair knows the name. `Pattern::index_of` inverts the mixed-radix encoding that
    // produced it, so the new coordinates are arithmetic rather than a search (D304).
    //
    // It also removes a hazard the hash route had and had to guard against by hand: a target
    // set holds hashes, the first candidate hashing to one is not necessarily the name being
    // repaired, and rewriting a record from a collision would forge coordinates using the tool
    // meant to prevent forged records. Searching for the name cannot collide.
    let mut repaired = 0;
    for name in &stale {
        let Some(derivation) = orbistoun_names::solve::derive(name, patterns, standard) else {
            continue;
        };
        if let Some(existing) = file.derivations.get_mut(name) {
            existing.method = derivation.method;
            repaired += 1;
        }
    }
    let missed = stale.len() - repaired;
    if missed > 0 {
        // Not an error. A name the grammar genuinely cannot reach any more belongs on the
        // ceiling, which is what the ceiling is for - and saying so beats a silent partial
        // repair that leaves somebody wondering why the count still does not add up.
        println!("  {repaired} re-derived, {missed} the current grammar cannot produce at all");
    } else {
        println!("  {repaired} re-derived");
    }
    repaired
}

/// Re-runs every static harvest against the module its record names.
///
/// # Why this is the point of splitting `observed` in two
///
/// A static record says a name was read out of one named file, at rest. That is not a
/// claim anybody has to take on trust - the file either contains the string or it does
/// not, and re-reading it settles the question exactly as an array lookup settles a
/// generated one. The only difference is that the file is not in this repository, so CI
/// cannot do it and a person holding the title can (D213).
///
/// The old vocabulary made this impossible to even ask. `observed` covered both "read out
/// of a file" and "worked out by watching a guest run", and there is no single check that
/// applies to both, so neither got one.
///
/// Absent modules are counted and named, never passed. A check that reports success for
/// material it could not read is worse than no check.
fn verify_static_records(file: &orbistoun_nid::SymbolDbFile) -> Result<()> {
    use std::collections::BTreeMap;

    // Grouped by module so each is read and scanned once. A record-at-a-time loop would
    // re-scan a thirty-megabyte executable for every one of the hundred names it carries.
    let mut by_module: BTreeMap<&str, Vec<&String>> = BTreeMap::new();
    for name in &file.names {
        if let Some(orbistoun_nid::Method::Static { from, .. }) =
            file.derivations.get(name).map(|d| &d.method)
        {
            by_module.entry(from.as_str()).or_default().push(name);
        }
    }
    if by_module.is_empty() {
        return Ok(());
    }

    let mut checked = 0_usize;
    let mut unchecked = 0_usize;
    let mut absent: Vec<&str> = Vec::new();
    let mut failed: Vec<(&str, &str)> = Vec::new();

    for (module, names) in &by_module {
        let path = std::path::Path::new(module);
        let Ok(bytes) = std::fs::read(path) else {
            unchecked += names.len();
            absent.push(module);
            continue;
        };
        let candidates: std::collections::HashSet<String> =
            orbistoun_names::strings::candidates(&bytes)
                .into_iter()
                .collect();
        for name in names {
            if candidates.contains(name.as_str()) {
                checked += 1;
            } else {
                failed.push((module, name.as_str()));
            }
        }
    }

    println!();
    println!("{checked} static record(s) re-harvested from the module each one names");
    if unchecked > 0 {
        // `concat!` defeats implicit `{name}` capture, so both arguments go positional.
        println!(
            concat!(
                "  {} not checked - {} module(s) are not here, which is the normal state ",
                "of this repository and of CI:"
            ),
            unchecked,
            absent.len()
        );
        for module in &absent {
            println!("      {module}");
        }
    }
    if failed.is_empty() {
        return Ok(());
    }
    // A record naming a module that does not contain the string is a false provenance
    // claim, which is precisely what this whole mechanism exists to make impossible.
    println!();
    println!(
        "{} record(s) claim a string the named module does not contain:",
        failed.len()
    );
    for (module, name) in &failed {
        println!("  {name}  is not in  {module}");
    }
    anyhow::bail!("a static provenance record does not hold against its own module")
}

/// One line saying what kind of material the database was built out of.
///
/// **The headline the split was for.** The tier listing below answers "what would it take
/// to check this?", which is the audit's own question; this answers the one a person asks
/// first - how much of what we know came from running things, and how much from reading
/// them. Under the old vocabulary the answer was unobtainable, because the value that
/// would have carried it covered both (D213).
///
/// Zero classes are printed too. "0 external" is the most reassuring number in the line
/// and would be the most conspicuous one to omit.
fn print_by_evidence(file: &orbistoun_nid::SymbolDbFile) {
    use orbistoun_nid::Evidence;

    let mut counts = [0_usize; 4];
    for derivation in file.derivations.values() {
        let slot = match derivation.method.evidence() {
            Evidence::Derived => 0,
            Evidence::Static => 1,
            Evidence::Runtime => 2,
            Evidence::External => 3,
        };
        counts[slot] += 1;
    }
    let unrecorded = file.names.len().saturating_sub(file.derivations.len());
    let classes = [
        Evidence::Derived,
        Evidence::Static,
        Evidence::Runtime,
        Evidence::External,
    ];
    let line: Vec<String> = classes
        .iter()
        .zip(counts)
        .map(|(class, n)| format!("{n} {}", class.label()))
        .collect();
    if unrecorded > 0 {
        println!(
            "  by evidence: {} ({unrecorded} with no record)",
            line.join(", ")
        );
    } else {
        println!("  by evidence: {}", line.join(", "));
    }
}

/// Names this project worked out but this repository cannot re-derive on its own, grouped
/// by what somebody else would need in order to arrive at them.
///
/// **This is the half of the audit that used to be a shrug.** One bucket labelled
/// "documented, not verified" held a string read deterministically out of a file next to a
/// conclusion a person drew from a trace, and it described both as unverifiable. Neither
/// is: one needs the module, one needs a run of it, and saying which is both truer and a
/// stronger claim than declining to classify them (D213).
fn print_by_tier(entries: &[(&String, &orbistoun_nid::Derivation)]) {
    use orbistoun_nid::Reproducible;

    // Tier order is the enum's own order, cheapest to check first, so the listing cannot
    // disagree with the type about which claim is stronger.
    for tier in [
        Reproducible::FromModule,
        Reproducible::FromRun,
        Reproducible::FromHardware,
    ] {
        let in_tier: Vec<_> = entries
            .iter()
            .filter(|(_, d)| d.method.reproducible() == tier)
            .collect();
        if in_tier.is_empty() {
            continue;
        }
        println!();
        println!(
            "{} reproducible {} - ours, but not from this repository alone:",
            in_tier.len(),
            tier.label()
        );
        for (name, d) in in_tier {
            println!("  {name}  [{}]  {}", d.on, how_it_was_found(&d.method));
            if let Some(note) = &d.note {
                println!("      {note}");
            }
        }
    }
}

/// `audit` - re-derive every name in a database from this repository's own inputs.
pub(crate) fn cmd_audit(
    database: &std::path::Path,
    grammar: Option<&std::path::Path>,
    ceiling: Option<&std::path::Path>,
    deep: bool,
    verify_harvest: bool,
    repair: bool,
) -> Result<()> {
    let text = std::fs::read_to_string(database)
        .with_context(|| format!("reading {}", database.display()))?;
    let mut file = orbistoun_nid::SymbolDbFile::from_json(&text).context("parsing the database")?;

    let grammar = match grammar {
        Some(path) => {
            let text = std::fs::read_to_string(path)
                .with_context(|| format!("reading {}", path.display()))?;
            orbistoun_names::Grammar::parse(&text)?
        }
        None => orbistoun_names::Grammar::builtin()?,
    };
    let patterns = grammar.patterns()?;
    let standard = orbistoun_names::standard_names();
    // The rules an affixed record is rechecked against. Shipped data, like the grammar, so
    // a record that verified when it was written still verifies unless somebody removed the
    // rule that made it - which is exactly the staleness this audit exists to surface.
    let affixes = orbistoun_names::affix::Affixes::builtin()?;

    // Before anything is classified, so a repaired record is reported as what it now is
    // rather than as a failure that was quietly fixed on the way past.
    if repair {
        // No thread count: a repair is arithmetic per name now rather than a sweep, so there
        // is nothing to spread across cores (D304). `--threads` still governs `--deep`, which
        // genuinely searches because it has no pattern to invert against.
        if repair_generated_records(&mut file, &patterns, &standard) > 0 {
            let text =
                serde_json::to_string_pretty(&file).context("serialising the symbol database")?;
            std::fs::write(database, text)
                .with_context(|| format!("writing {}", database.display()))?;
        }
    }
    let file = file;

    let mut verified = 0_usize;
    // Everything that is ours but needs something this repository does not hold, kept in
    // tier order so the report reads from cheapest to check to most expensive. Sorting
    // into one "documented, not verified" bucket was the thing that let a static harvest
    // and a runtime observation look like the same claim (D213).
    let mut elsewhere: Vec<(&String, &orbistoun_nid::Derivation)> = Vec::new();
    let mut imported: Vec<(&String, &orbistoun_nid::Derivation)> = Vec::new();
    let mut unaccounted: Vec<&String> = Vec::new();

    for name in &file.names {
        match file.derivations.get(name) {
            Some(d) if orbistoun_names::solve::verify(name, d, &patterns, &standard, &affixes) => {
                verified += 1;
            }
            // Recorded, but not from material this repository holds. Split by whether
            // this project worked it out or took it from elsewhere - the whole question
            // this file exists to answer.
            Some(d) if !d.method.is_mechanically_checkable() => {
                if d.method.is_our_own_work() {
                    elsewhere.push((name, d));
                } else {
                    imported.push((name, d));
                }
            }
            // A record that claims to be checkable and is not, or no record at all.
            // `--deep` gives the second kind a chance before giving up on it.
            _ if deep && orbistoun_names::solve::derive(name, &patterns, &standard).is_some() => {
                verified += 1;
            }
            _ => unaccounted.push(name),
        }
    }

    let total = file.names.len();
    println!("{verified} of {total} names re-derived from this repository");
    print_by_evidence(&file);
    if !deep && !unaccounted.is_empty() {
        println!("  (pass --deep to search the whole space for names with no record)");
    }

    if !elsewhere.is_empty() {
        print_by_tier(&elsewhere);
    }

    // Before the unaccounted list, because a false static record is a different and worse
    // finding than an unaccounted name, and it should not be read after two hundred lines
    // of known ceiling.
    if verify_harvest {
        verify_static_records(&file)?;
    }

    if !imported.is_empty() {
        // Listed loudly and separately. Not an error - taking a name from a public
        // database is lawful and sometimes sensible - but it is the one category that
        // changes the answer to "did you derive all of this yourselves?", so it must
        // never be quiet.
        println!();
        println!("{} came from outside this project:", imported.len());
        for (name, d) in &imported {
            let source = match &d.method {
                orbistoun_nid::Method::Supplied { source } => source.as_str(),
                _ => "",
            };
            println!("  {name}  [{}]  {source}", d.on);
        }
    }

    if unaccounted.is_empty() {
        println!(
            "
every name is accounted for"
        );
        // **The ceiling is still checked.** Returning here skipped it whenever nothing was
        // unaccounted, which is exactly the state in which the ceiling has gone stale - so
        // the half of its rule that says "an entry that stopped applying must leave" was
        // unenforceable in the only case that could trigger it. A guard that passes because
        // it stopped looking is worse than no guard (D199).
        if let Some(path) = ceiling {
            return against_ceiling(path, &unaccounted);
        }
        return Ok(());
    }
    let plural = if unaccounted.len() == 1 {
        "name"
    } else {
        "names"
    };
    println!(
        "
{} {plural} this repository cannot account for:",
        unaccounted.len()
    );
    for name in &unaccounted {
        println!("  {name}");
    }

    if let Some(path) = ceiling {
        return against_ceiling(path, &unaccounted);
    }

    // A non-zero status so this can gate a commit. An unaccounted name is not
    // necessarily wrong - a vocabulary shrinks, a name is added by hand - but it is
    // always something a person should have decided about deliberately, and recorded.
    anyhow::bail!("{} {plural} unaccounted for", unaccounted.len())
}

/// Judges the unaccounted set against a written-down ceiling.
///
/// Two failure directions, both load-bearing. A name unaccounted and **unlisted** is the
/// thing worth stopping for - somebody added a name nobody can explain. A name listed that
/// is **no longer** unaccounted has to leave the file, or the ceiling stops describing
/// anything and becomes a list nobody prunes (D208).
fn against_ceiling(path: &std::path::Path, unaccounted: &[&String]) -> Result<()> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading the ceiling at {}", path.display()))?;
    let listed: std::collections::BTreeSet<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    let now: std::collections::BTreeSet<&str> = unaccounted.iter().map(|n| n.as_str()).collect();

    let added: Vec<&&str> = now.difference(&listed).collect();
    let retired: Vec<&&str> = listed.difference(&now).collect();

    println!();
    if !added.is_empty() {
        println!("{} of those are NOT on the ceiling:", added.len());
        for name in &added {
            println!("  {name}");
        }
        // **Try `--repair` before reaching for the ceiling.** Most arrivals here are not
        // names the grammar cannot spell; they are names whose recorded index a learned
        // word renumbered, and the ceiling reached 202 entries before anybody checked
        // which (D213). Adding one without trying the repair records a fact that is not
        // true, in a file that is supposed to be evidence.
        println!("  most of these are records the grammar moved underneath, not names it");
        println!("  cannot produce. Try `audit --repair` first; it re-derives them in one");
        println!("  sweep. What survives that is a real gap - add it to");
        println!("  {} with a reason.", path.display());
    }
    if !retired.is_empty() {
        println!(
            "{} on the ceiling are now accounted for and must be removed from {}:",
            retired.len(),
            path.display()
        );
        for name in &retired {
            println!("  {name}");
        }
    }
    if added.is_empty() && retired.is_empty() {
        println!(
            "unaccounted set matches the ceiling exactly ({} names, and it may only shrink)",
            listed.len()
        );
        return Ok(());
    }
    anyhow::bail!("the unaccounted set does not match {}", path.display())
}
