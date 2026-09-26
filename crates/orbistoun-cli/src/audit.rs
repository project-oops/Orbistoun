//! The symbol database: `harvest` and `audit`.

use crate::common::how_it_was_found;
use anyhow::{Context, Result};

/// Collects every version script (`.map`) beneath a directory.
///
/// Hand-rolled: recursing and matching one file kind is less code than a directory-walking
/// dependency.
fn find_symbol_maps(root: &std::path::Path, found: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        // An unreadable directory costs its own symbols, not the run.
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
            // The rule is asked of `is_version_script` rather than re-decided here: `lib/libthr`
            // names its script `pthread.map` and `lib/libsys` names its `Symbol.sys.map` (D126).
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
            // Named rather than silently skipped: a sparse checkout missing one library yields a
            // smaller list, and the reader knows which.
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

    // A revision is a citation; a local path is not. When both exist the revision is recorded and
    // the path is dropped, since nobody re-deriving this has the same directory.
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
        // A path with no revision is only half a citation.
        eprintln!("note: pass --revision to record which revision this came from");
    }
    Ok(())
}

/// Re-derives generated records that the current grammar no longer confirms.
///
/// A generated record is `pattern` plus `index`, a position in a mixed-radix enumeration over the
/// vocabularies, so adding a word renumbers every candidate built from that vocabulary. Confirmed
/// names widen the grammar (D195), so recorded coordinates move under names that are still right;
/// without repair they would fall onto the unaccounted ceiling, which may only shrink.
/// `Pattern::index_of` inverts the encoding, so a repair is arithmetic per name, not a search. The
/// date is never touched: it records when a name was first worked out.
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
                        // This filter has already narrowed to `Generated`, so no affix rule
                        // applies.
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

    // Derived from the name with `Pattern::index_of`, not searched for by hash. A hash search could
    // also land on a colliding candidate first and forge coordinates; inverting the known name
    // cannot collide.
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
        // Not an error: a name the grammar can no longer reach belongs on the ceiling, and the
        // count says how many.
        println!("  {repaired} re-derived, {missed} the current grammar cannot produce at all");
    } else {
        println!("  {repaired} re-derived");
    }
    repaired
}

/// Re-runs every static harvest against the module its record names.
///
/// A static record says a name was read out of one named file, which re-reading the file settles
/// exactly. The file is not in this repository, so CI cannot run this and a person holding the
/// title can (D213). Absent modules are counted and named, never passed.
fn verify_static_records(file: &orbistoun_nid::SymbolDbFile) -> Result<()> {
    use std::collections::BTreeMap;

    // Grouped by module so each file is read and scanned once, not once per name it carries.
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
    // A record naming a module that does not contain the string is a false provenance claim.
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
/// How much of what is known came from running things and how much from reading them (D213). Zero
/// classes are printed too, so "0 external" is asserted rather than implied.
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

/// Names this project worked out but this repository cannot re-derive on its own, grouped by what
/// somebody else would need in order to arrive at them.
///
/// A string read from a module needs the module; a conclusion drawn from a trace needs a run
/// (D213).
fn print_by_tier(entries: &[(&String, &orbistoun_nid::Derivation)]) {
    use orbistoun_nid::Reproducible;

    // Tier order is the enum's own order, cheapest to check first, so the listing cannot disagree
    // with the type about which claim is stronger.
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
    // The rules an affixed record is rechecked against. Shipped data, like the grammar, so a record
    // stops verifying only when the rule that made it is removed.
    let affixes = orbistoun_names::affix::Affixes::builtin()?;

    // Before anything is classified, so a repaired record is reported as what it is now.
    if repair {
        // No thread count: a repair is arithmetic per name. `--threads` governs `--deep`, which
        // searches because it has no pattern to invert.
        if repair_generated_records(&mut file, &patterns, &standard) > 0 {
            let text =
                serde_json::to_string_pretty(&file).context("serialising the symbol database")?;
            std::fs::write(database, text)
                .with_context(|| format!("writing {}", database.display()))?;
        }
    }
    let file = file;

    let mut verified = 0_usize;
    // Everything that is ours but needs something this repository does not hold, in tier order from
    // cheapest to check to most expensive (D213).
    let mut elsewhere: Vec<(&String, &orbistoun_nid::Derivation)> = Vec::new();
    let mut imported: Vec<(&String, &orbistoun_nid::Derivation)> = Vec::new();
    let mut unaccounted: Vec<&String> = Vec::new();

    for name in &file.names {
        match file.derivations.get(name) {
            Some(d) if orbistoun_names::solve::verify(name, d, &patterns, &standard, &affixes) => {
                verified += 1;
            }
            // Recorded, but not from material this repository holds; split by whether this project
            // worked it out or took it from elsewhere.
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

    // Before the unaccounted list: a false static record is a worse finding than an unaccounted
    // name.
    if verify_harvest {
        verify_static_records(&file)?;
    }

    if !imported.is_empty() {
        // Listed separately and loudly. Taking a name from a public database is lawful, but it
        // changes the answer to "was all of this derived here?".
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
        // The ceiling is checked even when nothing is unaccounted, the one state in which an entry
        // that stopped applying must leave it (D213).
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

    // A non-zero status so this can gate a commit. An unaccounted name is not necessarily wrong,
    // but a person decides about it and records the decision.
    anyhow::bail!("{} {plural} unaccounted for", unaccounted.len())
}

/// Judges the unaccounted set against a written-down ceiling.
///
/// Two failure directions: a name unaccounted and unlisted is unexplained, and a listed name that
/// is no longer unaccounted must leave the file so the ceiling keeps describing something.
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
        // Most arrivals here are records a learned word renumbered, not names the grammar cannot
        // spell, so `--repair` comes before adding to the ceiling.
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
