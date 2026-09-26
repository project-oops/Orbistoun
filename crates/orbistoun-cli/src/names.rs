//! Recovering import names: the `names` command and its searches.

use crate::common::{how_it_was_found, library_or, percent, previous_trace};
use crate::{Cli, WordSource, suffix_for};
use anyhow::{Context, Result};
use orbistoun_service::Service;

/// `names` - search generated names for ones that hash to a module's unnamed imports.
pub(crate) struct NameSearch<'a> {
    /// Module to name the imports of, or a directory of them.
    pub(crate) path: &'a std::path::Path,
    /// Threads to search with; zero means one per core.
    pub(crate) threads: usize,
    /// Grammar to use instead of the built-in vocabulary.
    pub(crate) grammar: Option<&'a std::path::Path>,
    /// Extra candidate names to try verbatim.
    pub(crate) words: Option<&'a std::path::Path>,
    /// Where those names came from, for the provenance record.
    pub(crate) words_from: WordSource,
    /// Where to write the names found.
    pub(crate) out: Option<&'a std::path::Path>,
    /// Where to write the hashes still unnamed.
    pub(crate) wanted: Option<&'a std::path::Path>,
    /// Also harvest candidates out of what a previous run captured from guest memory.
    pub(crate) from_trace: bool,
    /// Probe reports whose kernel export tables contribute hashes to look for.
    pub(crate) from_report: &'a [std::path::PathBuf],
}

/// Merges the names found into a database, never losing what was already there.
///
/// **Accumulates rather than overwrites.** A database is built from many modules over
/// many runs, and each run only ever sees the imports of the module it was given -
/// so writing the run's own findings would silently discard every name learned from
/// anything else. Names are cheap to keep and expensive to rediscover (D074).
///
/// An existing derivation always wins over a new one for the same name. The record
/// should say when a name was **first** worked out; rewriting it on every sweep would
/// turn a history into a timestamp of the last time somebody ran the tool.
fn write_symbol_db(
    path: &std::path::Path,
    suffix: &[u8],
    found: &[orbistoun_names::solve::Solved],
) -> Result<()> {
    // The suffix actually hashed with, not whatever the user typed. Writing the raw
    // argument recorded an empty string whenever the shipped default was used, and a
    // database with no suffix cannot be loaded at all - so every trace fell back to
    // printing hashes, which looked like the search having failed.
    let suffix_hex = orbistoun_nid::encode_hex(suffix);
    let mut file = match std::fs::read_to_string(path) {
        Ok(text) => orbistoun_nid::SymbolDbFile::from_json(&text)
            .with_context(|| format!("parsing the existing {}", path.display()))?,
        // Absent is the ordinary first run. Any other error is a real problem and must
        // not be mistaken for it, or a permissions fault silently starts from nothing.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => orbistoun_nid::SymbolDbFile {
            suffix_hex: suffix_hex.clone(),
            names: Vec::new(),
            derivations: std::collections::BTreeMap::new(),
        },
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };

    let before = file.names.len();
    let mut known: std::collections::BTreeSet<String> = file.names.iter().cloned().collect();
    for solved in found {
        if known.insert(solved.name.clone()) {
            // Written alongside the name, by the code that found it. This is what makes
            // "we generated these" checkable rather than merely stated (D073).
            file.derivations
                .insert(solved.name.clone(), solved.derivation.clone());
        }
    }
    // Sorted, so a diff shows what was learned rather than how the search was ordered.
    file.names = known.into_iter().collect();

    // Names only in `names`. NIDs are derived, never stored - a file carrying both could
    // hold a pair that does not hash to itself, and that would surface as a mystery
    // unresolved import much later (docs/SYMBOLS.md).
    let text = serde_json::to_string_pretty(&file).context("serialising the symbol database")?;
    std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))?;
    println!(
        "{}: {} names ({} new, {before} already known)",
        path.display(),
        file.names.len(),
        file.names.len() - before
    );
    Ok(())
}

/// Writes the platform exports nothing can name, accumulating across runs.
///
/// # Why this is a second file and not more of `wanted.txt`
///
/// `wanted.txt` means *imports orbistoun cannot name*, and a kernel export nothing has ever
/// imported is not one - folding them together would change what that file means and move the
/// number every report quotes (D605). They are a work list of their own, on the same terms.
///
/// # Why it is written down at all
///
/// The report directory is **overwritten**. A batch of captures lives for about an hour, and the
/// 215 hashes it says the platform exports and this project cannot name exist, today, only while
/// those files are on disk. That is the same disappearance D618 found in the measurement table,
/// one file along: what a run learned is lost because the thing it read was replaced.
///
/// Accumulated, never truncated, and a hash leaves only when something names it - the rule
/// `write_wanted` follows for the same reason (D074, D619).
fn write_exported(
    service: &Service,
    path: &std::path::Path,
    exported: &[orbistoun_nid::Nid],
    found: &[orbistoun_names::solve::Solved],
) -> Result<()> {
    use std::fmt::Write as _;

    const HEADER: &[&str] = &[
        "# Platform export hashes orbistoun cannot yet name - the other work list.",
        "#",
        "# What a console's own kernel export table offers, whether or not any title has ever",
        "# imported it. A collision search cannot reach these from a corpus of guests, which is",
        "# the whole reason they are worth keeping (D245, D605).",
        "#",
        "# Separate from wanted.txt, which means *imports* nothing can name. Merging them would",
        "# change what that file counts.",
        "#",
        "# Generated - never hand-edited. Refresh with:",
        "#     ORBISTOUN_PROBE_REPORTS=<a directory of reports> ./bin/orbistoun names",
    ];

    let carried: std::collections::BTreeSet<u64> = match std::fs::read_to_string(path) {
        Ok(text) => text
            .lines()
            .map(str::trim)
            .filter_map(|l| l.strip_prefix("0x"))
            .filter_map(|l| u64::from_str_radix(l, 16).ok())
            .collect(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => std::collections::BTreeSet::new(),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    // The same three rules `wanted_now` applies, and the same function, so the two lists cannot
    // drift about what "still unnamed" means.
    let still = wanted_now(carried, exported, found, |nid| service.is_named(nid));

    let mut text = String::new();
    for line in HEADER {
        let _ = writeln!(text, "{line}");
    }
    for nid in &still {
        let _ = writeln!(text, "{}", orbistoun_nid::Nid::from_raw(*nid));
    }
    std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))?;
    println!(
        "{}: {} platform export(s) still unnamed",
        path.display(),
        still.len()
    );
    Ok(())
}

/// Writes the hashes still unnamed, as the work list for the next round.
///
/// Persisted rather than left in a terminal. Without this every run rediscovers the
/// same list and forgets it again, and the list is precisely what the next round of
/// vocabulary work is aimed at.
fn write_wanted(
    service: &Service,
    path: &std::path::Path,
    unnamed: &[orbistoun_nid::Nid],
    found: &[orbistoun_names::solve::Solved],
) -> Result<()> {
    use std::fmt::Write as _;

    // One line per entry rather than a continued literal: a backslash-continued string
    // carries the source indentation into the file, which is how the last version of
    // this header ended up indented in the output.
    const HEADER: &[&str] = &[
        "# Import hashes orbistoun cannot yet name - the work list.",
        "#",
        "# These are system library functions, common to everything built for the",
        "# platform. A hash is derived from a function name and carries nothing of any",
        "# title. Accumulated across every module ever searched; entries disappear as",
        "# the vocabulary grows to explain them.",
        "#",
        "# Generated - never hand-edited. Refresh with: ./bin/orbistoun names",
    ];

    // Accumulated across runs like the database is, and for the same reason: each run
    // sees one module, and the work list is the union of what every module has wanted.
    let carried: std::collections::BTreeSet<u64> = match std::fs::read_to_string(path) {
        Ok(text) => text
            .lines()
            .map(str::trim)
            .filter_map(|l| l.strip_prefix("0x"))
            .filter_map(|l| u64::from_str_radix(l, 16).ok())
            .collect(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => std::collections::BTreeSet::new(),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let wanted = wanted_now(carried, unnamed, found, |nid| service.is_named(nid));

    let mut text = String::new();
    for line in HEADER {
        let _ = writeln!(text, "{line}");
    }
    for nid in &wanted {
        let _ = writeln!(text, "{}", orbistoun_nid::Nid::from_raw(*nid));
    }
    std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))?;
    println!("{}: {} hashes still unnamed", path.display(), wanted.len());
    Ok(())
}

/// What the work list holds after a run, given what can be named.
///
/// Pure, and separated from the file for the reason principle 8 gives: the rule is worth
/// pinning and the writing is not. The bug this shape exposes was invisible while the two
/// were one function, because reproducing it needed a corpus, a database and a search.
///
/// **The rule is "cannot be named now", not "was never solved".** Those read alike and
/// are not the same set. A hash named by any earlier run is excluded from every later
/// search - being named is exactly what excludes it - so it can never be solved again,
/// so a rule keyed on this run's results can never remove it. It stays forever. Measured
/// on the committed file: 116 of 3829 entries were hashes the database could already
/// name, against a header promising they "disappear as the vocabulary grows".
fn wanted_now(
    mut carried: std::collections::BTreeSet<u64>,
    unnamed: &[orbistoun_nid::Nid],
    found: &[orbistoun_names::solve::Solved],
    is_named: impl Fn(orbistoun_nid::Nid) -> bool,
) -> std::collections::BTreeSet<u64> {
    carried.extend(unnamed.iter().map(|nid| nid.as_raw()));
    carried.retain(|raw| !is_named(orbistoun_nid::Nid::from_raw(*raw)));
    // This run's own results too. They are not in that database yet - it is written from
    // `found` after this, and the service answering `is_named` was built before either.
    for solved in found {
        carried.remove(&solved.nid.as_raw());
    }
    carried
}

/// Searches the published word list, then any list the caller supplied.
///
/// Kept apart deliberately. They are different kinds of claim - one is fixed by
/// published standards, the other is whatever a caller handed over - and recording them
/// under one derivation is what made supplied names look as though they had come from
/// this repository's own list (D119).
/// Searches the module's own bytes for the names of its own imports.
///
/// # The strongest source there is, and the cheapest
///
/// Naming is generate-and-test against a one-way hash, so everything turns on whether the
/// true name is in the candidate set. A title's diagnostics, assertions and symbol tables
/// leave literal function names in its data - so for a large class of imports the answer is
/// lying inside the file already being parsed, and no guess is involved at all.
///
/// `sceKernelCreateSema` is why this exists. It blocked two titles, the generator could not
/// spell it - the vocabulary held `Semaphore`, so `sceKernelCreateSemaphore` was generated
/// and tested and the real name was never in the set - and the string was sitting in a
/// *third* title's bytes (D193).
///
/// Run before the generator because it costs one pass over a file that is already in
/// memory, and everything it resolves is removed from a search that is millions of times
/// more expensive.
/// Adds confirmed words to the grammar file, and says how many were new.
///
/// Writes the shipped grammar in place. That file is data and tracked, so a run genuinely
/// changes what the next one can reach - which is the loop compounding rather than
/// restarting (D195).
fn learn_vocabulary(words: &[String]) -> Result<usize> {
    let path = std::path::Path::new("crates/orbistoun-names/data/vendor.toml");
    if !path.exists() {
        // Running from an installed binary rather than a checkout. The names are still
        // confirmed and written; only the grammar cannot grow, and saying nothing is right.
        return Ok(0);
    }
    let before =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    // The injected vocabulary too, which is not in the file and which this would otherwise
    // re-learn word by word (D258).
    let injected = orbistoun_names::posix_vocabulary();
    let after = match orbistoun_names::strings::learn_words(&before, words, &injected) {
        orbistoun_names::strings::Learned::Nothing => return Ok(0),
        orbistoun_names::strings::Learned::Grammar(after) => after,
        // **Printed, not returned as zero.** A refusal that looked like "nothing new" would
        // be the harvest silently doing nothing, which is a worse failure than the one the
        // ceiling exists to prevent - and indistinguishable from a clean run (principle 3).
        orbistoun_names::strings::Learned::Refused(refusal) => {
            println!("  {}", refusal.say());
            return Ok(0);
        }
    };
    // Parsed before it is trusted. A grammar this cannot read is one the next run cannot
    // either, and writing it would break the search rather than widen it.
    let now = orbistoun_names::Grammar::parse(&after)
        .map(|g| g.vocabulary.get("learned").map_or(0, Vec::len))
        .context("the widened grammar no longer parses - not written")?;
    let was = orbistoun_names::Grammar::parse(&before)
        .ok()
        .and_then(|g| g.vocabulary.get("learned").map(Vec::len))
        .unwrap_or(0);
    let added = now.saturating_sub(was);

    // **What it now costs, said where it grows.** A refusal is loud (D330) and an acceptance
    // was silent, so a vocabulary could go from 177 words to thousands with nothing reporting
    // it - which is D320 exactly, at a price nobody is told about. The ceiling stops the
    // unaffordable case; this is what makes the affordable one visible.
    let before_cost = orbistoun_names::strings::round_cost(&before, was);
    let after_cost = orbistoun_names::strings::round_cost(&after, now);
    println!(
        "  learned {was} -> {now} words; a vocabulary round goes {before_cost} -> {after_cost} candidates"
    );
    if before_cost > 0 && after_cost / before_cost >= 2 {
        // A doubling is where somebody should look, not where anything is wrong. Said once,
        // with the multiple, rather than as a threshold nobody can see the position of.
        println!(
            "    that is {}x - worth reading before the next sweep",
            after_cost / before_cost
        );
    }

    std::fs::write(path, after).with_context(|| format!("writing {}", path.display()))?;
    Ok(added)
}

/// Whether a file name is a guest module worth searching.
///
/// **One implementation, deliberately.** This used to be four hand-written globs in
/// `bin/orbistoun` (`titles/*/eboot.bin`, then `.prx` at three fixed depths), and they
/// silently omitted `.sprx` entirely - eleven modules in the local corpus were never once
/// searched, and nothing said so, because a glob that matches nothing is not an error.
/// That is the same failure `is_version_script` was written to fix for symbol maps (D191).
/// Case-insensitive, because the corpus is read off a filesystem that does not care and
/// a module written out as `EBOOT.BIN` is the same module.
fn is_guest_module(name: &str) -> bool {
    if name.eq_ignore_ascii_case("eboot.bin") {
        return true;
    }
    std::path::Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("prx") || e.eq_ignore_ascii_case("sprx"))
}

/// Every guest module beneath a directory, in a stable order.
///
/// Hand-rolled for the same reason as `audit::find_symbol_maps`: the whole job is "recurse and
/// match a filename", and a directory-walking dependency would be more code to audit than
/// the code it replaces.
fn collect_modules(root: &std::path::Path, found: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        // An unreadable directory costs its own modules, not the run.
        return;
    };
    // Sorted, so the search order - and therefore which module gets credited for a name
    // two modules both contain - does not depend on the filesystem.
    let mut here: Vec<std::path::PathBuf> = entries.flatten().map(|e| e.path()).collect();
    here.sort();
    for path in here {
        if path.is_dir() {
            collect_modules(&path, found);
        } else if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(is_guest_module)
        {
            found.push(path);
        }
    }
}

/// A module's path as it goes into a provenance record.
///
/// Normalised, because the same module reached two ways - a trailing slash on a directory
/// argument, a Windows separator - would otherwise produce two records that look like two
/// different modules. One of those is already in the database: a tab-completed path left
/// `titles/PPSA21564-app0//eboot.bin` on a hundred names.
///
/// **Relative to the title library, never absolute** (worklog 847): a module in the shared library
/// is recorded as `titles/<id>/...`, the form every record has always had. Once the repository's own
/// `titles/` folder went, the library was reached by its absolute path - a home directory - and that
/// went into the tracked database verbatim.
fn record_path(path: &std::path::Path) -> String {
    let library = library_or(None);
    let path = path.strip_prefix(&library).map_or_else(
        |_| path.to_path_buf(),
        |inside| std::path::Path::new("titles").join(inside),
    );
    let text = path.display().to_string().replace('\\', "/");
    let mut out = String::with_capacity(text.len());
    let mut last_was_slash = false;
    for c in text.chars() {
        if c == '/' && last_was_slash {
            continue;
        }
        last_was_slash = c == '/';
        out.push(c);
    }
    out
}

/// What one module contributed to a corpus search.
struct ModuleImports {
    /// Where it is, normalised for a provenance record.
    path: String,
    /// The path as given, for reading it a second time.
    ///
    /// **Twice on purpose.** The candidate strings cannot be harvested on the first pass,
    /// because the target set is not known until every module has been read - and holding
    /// fifty-three modules' candidate sets in memory to avoid re-reading them is the
    /// wrong trade for files this size.
    file: std::path::PathBuf,
    /// The hashes it imports and nothing yet names.
    unnamed: std::collections::HashSet<u64>,
}

/// Reads every module's unnamed imports, so one search can answer all of them.
///
/// **This is the change that made a corpus search affordable.** Searching modules one at a
/// time re-ran the entire 2.6-billion-candidate sweep for each, once per module, forty-two
/// times over - while a wider target set costs a search nothing at all, because every
/// candidate is tested by one hash-set lookup regardless of how many hashes are in it.
fn read_corpus(
    service: &Service,
    modules: &[std::path::PathBuf],
) -> Result<(Vec<ModuleImports>, Vec<orbistoun_nid::Nid>)> {
    let mut per_module = Vec::new();
    let mut union: std::collections::BTreeSet<u64> = std::collections::BTreeSet::new();
    for file in modules {
        let bytes = std::fs::read(file).with_context(|| format!("reading {}", file.display()))?;
        // A module that will not parse is skipped rather than fatal. Previous-generation
        // containers are expected in a real corpus and are not an error - but say so, or
        // a silently skipped module looks exactly like one with nothing to contribute.
        let unnamed = match service.unnamed_imports(&bytes) {
            Ok(nids) => nids,
            Err(e) => {
                eprintln!("  skipped {} - {e}", file.display());
                continue;
            }
        };
        union.extend(unnamed.iter().copied().map(orbistoun_nid::Nid::as_raw));
        per_module.push(ModuleImports {
            path: record_path(file),
            file: file.clone(),
            unnamed: unnamed
                .iter()
                .copied()
                .map(orbistoun_nid::Nid::as_raw)
                .collect(),
        });
    }
    let all = union
        .into_iter()
        .map(orbistoun_nid::Nid::from_raw)
        .collect();
    Ok((per_module, all))
}

/// Every name the sweep settled, each saying how it was found.
///
/// **With the source, always.** The sweep tries four of them and printed one sorted list,
/// so a name harvested from a module's own bytes and one the grammar produced were spelled
/// identically - and which it was decides whether the name may be used at all. Pointed at
/// another emulator's binary, module strings are that project's name list arriving through
/// a file, which D242 refuses at the front door. A reader could not tell, and neither could
/// the author (D257).
fn print_names_found(found: &[orbistoun_names::solve::Solved]) {
    for solved in found {
        println!(
            "  {}  {:<44}  {}",
            solved.nid,
            solved.name,
            how_it_was_found(&solved.derivation.method)
        );
    }
    print_names_per_source(found);
}

/// How many names each source settled.
///
/// So "the sweep named 29,403" can never be read as "this repository can now account
/// for 29,403" - which is the difference between a name the grammar earned and one read
/// out of somebody else's binary (D257).
fn print_names_per_source(found: &[orbistoun_names::solve::Solved]) {
    // Counted per source as well as in total, so "the sweep named 29,403" cannot be read
    // as "this repository can now account for 29,403".
    let mut per_source: std::collections::BTreeMap<String, usize> =
        std::collections::BTreeMap::new();
    for solved in found {
        let how = how_it_was_found(&solved.derivation.method);
        let source = how
            .split_whitespace()
            .next()
            .unwrap_or("unknown")
            .to_owned();
        *per_source.entry(source).or_default() += 1;
    }
    for (source, count) in &per_source {
        println!("  {count:>7} named by {source}");
    }
}

/// Reads identifier-shaped strings out of every module, and names what it can with them.
///
/// # Why a corpus finds names a module cannot find in itself
///
/// A name lying in one title's diagnostic text is the vendor's own spelling of a function
/// that every title on the platform imports. Searching each module against only its own
/// bytes threw that away: `sceKernelCreateSema` blocked two titles for weeks while the
/// string sat in a *third* title's data (D193), and the per-module search would have kept
/// missing it however many titles arrived.
///
/// Both cases are the same mechanism and the same proof - a hash collision - so they are
/// both [`orbistoun_nid::Method::Static`]. They are recorded as different sources because
/// they answer different questions: one says the module explains its own import, the other
/// says it took another module's material to explain it.
fn search_corpus_strings(
    hasher: &orbistoun_nid::NidHasher,
    corpus: &[ModuleImports],
    targets: &orbistoun_names::solve::Targets,
) -> Vec<orbistoun_names::solve::Solved> {
    let today = orbistoun_nid::today();
    let mut found: Vec<orbistoun_names::solve::Solved> = Vec::new();
    let mut solved_already: std::collections::HashSet<u64> = std::collections::HashSet::new();
    let mut tried = 0_u64;

    for module in corpus {
        let Ok(bytes) = std::fs::read(&module.file) else {
            continue;
        };
        let candidates = orbistoun_names::strings::candidates(&bytes);
        tried += candidates.len() as u64;
        for name in candidates {
            let nid = hasher.hash(&name);
            if !targets.wants(nid) || !solved_already.insert(nid.as_raw()) {
                continue;
            }
            // Which source it is, decided by the module that *wants* the hash rather than
            // by the one that carried the string. Asking the question the other way round
            // would call every name cross-module the moment two titles shared a string.
            let by = if module.unnamed.contains(&nid.as_raw()) {
                orbistoun_nid::StaticSource::ModuleStrings
            } else {
                orbistoun_nid::StaticSource::CrossModule
            };
            found.push(orbistoun_names::solve::Solved {
                nid,
                name,
                derivation: orbistoun_nid::Derivation::new(
                    orbistoun_nid::Method::Static {
                        by,
                        from: module.path.clone(),
                    },
                    today.as_str(),
                ),
            });
        }
    }

    let cross = found
        .iter()
        .filter(|s| {
            matches!(
                s.derivation.method,
                orbistoun_nid::Method::Static {
                    by: orbistoun_nid::StaticSource::CrossModule,
                    ..
                }
            )
        })
        .count();
    println!(
        "module strings: {tried} candidates across {} modules, {} named ({cross} from another module's bytes)",
        corpus.len(),
        found.len()
    );

    // **What a confirmed name is worth beyond itself.** Its parts are words the generator
    // was missing, and adding them makes every other name built from the same words
    // reachable *by generation* - which is what lets the provenance audit account for them
    // without the title, since a title can never be in this repository (D193).
    let mut words: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for solved in &found {
        words.extend(orbistoun_names::strings::parts_of(&solved.name));
    }
    // **Written, not suggested.** A word a person has to copy across is a step that does
    // not happen unattended, and the whole point is that the next title does not hit the
    // same gap. Data rather than code, so nothing rebuilds (D195).
    if !words.is_empty() {
        let list: Vec<String> = words.into_iter().collect();
        // Never fatal. The names are already confirmed and written; failing to widen the
        // grammar costs the *next* search, not this one.
        match learn_vocabulary(&list) {
            Ok(0) => {}
            Ok(added) => println!("  learned {added} new word(s) into the candidate grammar"),
            Err(e) => println!("  could not widen the grammar: {e}"),
        }
    }
    found
}

/// What reading a module's persisted argument dumps produced.
struct DumpHarvest {
    /// Names confirmed.
    solved: Vec<orbistoun_names::solve::Solved>,
    /// Captures the previous run left behind.
    captures: usize,
    /// Identifier-shaped runs found in them.
    candidates: usize,
    /// Whether a previous run of this module existed at all.
    had_trace: bool,
}

/// Reads identifier-shaped strings out of what a guest handed to its imports as it ran.
///
/// # The first harvester here that needs the guest to actually execute
///
/// Everything else reads a file at rest. This reads the bytes a pointer argument pointed
/// at, captured by the dispatch path while the guest was running (D198) - memory after the
/// loader mapped and relocated it, which can hold text no module contains as a literal: a
/// path assembled at runtime, a name read out of a data file, a string a decompressor
/// produced.
///
/// Cheap, because the capture already happened for a different reason. A run report is an
/// artefact taken for diagnosis; this is a second question asked of it.
fn search_trace_dumps(
    hasher: &orbistoun_nid::NidHasher,
    targets: &orbistoun_names::solve::Targets,
    module: &std::path::Path,
) -> DumpHarvest {
    let Some(trace) = previous_trace(module) else {
        return DumpHarvest {
            solved: Vec::new(),
            captures: 0,
            candidates: 0,
            had_trace: false,
        };
    };
    if trace.dumps.is_empty() {
        return DumpHarvest {
            solved: Vec::new(),
            captures: 0,
            candidates: 0,
            had_trace: true,
        };
    }

    // Both forms, because they can differ. `text` is what the dump renderer thought was
    // readable; `bytes` is everything, and an identifier can sit next to a byte the
    // renderer gave up on. Separated by a zero, so a run cannot straddle two captures and
    // invent an identifier that was never in guest memory.
    let mut material: Vec<u8> = Vec::new();
    for dump in &trace.dumps {
        material.extend_from_slice(dump.text.as_bytes());
        material.push(0);
        material.extend(decode_hex_bytes(&dump.bytes));
        material.push(0);
    }

    let candidates = orbistoun_names::strings::candidates(&material);
    let (solved, stats) = orbistoun_names::solve::solve_names(
        hasher,
        targets,
        &candidates,
        &orbistoun_nid::Derivation::new(
            orbistoun_nid::Method::Runtime {
                by: orbistoun_nid::RuntimeSource::ArgumentDump,
                how: format!(
                    "read out of guest memory during a run of {}, from what it passed to an import",
                    record_path(module)
                ),
            },
            orbistoun_nid::today().as_str(),
        ),
    );
    DumpHarvest {
        solved,
        captures: trace.dumps.len(),
        candidates: usize::try_from(stats.tried).unwrap_or(usize::MAX),
        had_trace: true,
    }
}

/// The runtime harvest across a whole corpus, reported once rather than per module.
///
/// **One line, including when the answer is nothing.** Fifty-three modules and six previous
/// runs would otherwise print forty-seven identical "no previous run" lines, and a report
/// nobody reads is the same as no report. What it must never do is stay silent about having
/// contributed nothing - a source that quietly does no work looks exactly like one that is
/// working (D213).
fn search_corpus_dumps(
    hasher: &orbistoun_nid::NidHasher,
    corpus: &[ModuleImports],
    targets: &orbistoun_names::solve::Targets,
) -> Vec<orbistoun_names::solve::Solved> {
    let mut found = Vec::new();
    let (mut traced, mut captures, mut candidates) = (0_usize, 0_usize, 0_usize);
    for module in corpus {
        let harvest = search_trace_dumps(hasher, targets, &module.file);
        if harvest.had_trace {
            traced += 1;
        }
        captures += harvest.captures;
        candidates += harvest.candidates;
        found.extend(harvest.solved);
    }

    if traced == 0 {
        println!("argument dumps: no previous run of any module in this corpus - nothing to read");
        return found;
    }
    println!(
        "argument dumps: {candidates} candidates from {captures} captures across {traced} previous run(s), {} named",
        found.len()
    );
    if captures > 0 && found.is_empty() {
        // Said out loud, because "0 named" from a source that is working and a source that
        // is misconfigured read identically. Dumps are forced per-import rather than
        // captured broadly, so most of them are scalars with no bytes at all.
        println!(
            "  (dumps are forced per-import - see WORKFLOW.md. Most captured so far are scalars carrying no text)"
        );
    }
    found
}

/// Bytes back out of the hex a dump is stored as.
///
/// Anything that is not a clean pair of hex digits is dropped rather than guessed at. A
/// mis-decoded byte would invent an identifier boundary that was never there, and the
/// candidate it produced would be tested against every wanted hash for nothing.
fn decode_hex_bytes(text: &str) -> Vec<u8> {
    let digits: Vec<u8> = text
        .bytes()
        .filter(u8::is_ascii_hexdigit)
        .collect::<Vec<u8>>();
    digits
        .chunks_exact(2)
        .filter_map(|pair| {
            let hi = char::from(pair[0]).to_digit(16)?;
            let lo = char::from(pair[1]).to_digit(16)?;
            u8::try_from(hi * 16 + lo).ok()
        })
        .collect()
}

/// Derives candidates from names this project already holds, and keeps the ones that hash.
///
/// # Why this runs last, and what it stands on
///
/// Every other source in the sweep proposes a name out of raw material - words, module
/// strings, an argument dump. This one proposes a name out of *a name already proved*, so
/// what it can reach depends entirely on what the run has established by the time it runs.
/// Putting it earlier would cost it the whole of this run's findings as seeds.
///
/// # Where the seeds come from
///
/// Three places, all of them names and none of them guesses: the harvested standard list,
/// the loaded symbol database, and whatever the rest of this sweep has just proved. A
/// candidate is never a seed - a derivation claiming a proved name was built from an
/// unproved one is a lie no audit could catch afterwards (D606).
///
/// # One pass, and it says so
///
/// A variant of a variant is out of reach: `snprintf_s` becomes a seed only on the *next*
/// run, once it is in the database. That is a real limit and it is cheap to live with,
/// because the alternative - iterating to a fixed point - multiplies the candidate count by
/// the depth and buys names nobody has evidence exist.
fn search_affixes(
    hasher: &orbistoun_nid::NidHasher,
    targets: &orbistoun_names::solve::Targets,
    service: &Service,
    already: &[orbistoun_names::solve::Solved],
) -> Result<Vec<orbistoun_names::solve::Solved>> {
    let affixes = orbistoun_names::affix::Affixes::builtin()?;
    let mut seeds: Vec<String> = orbistoun_names::standard_names();
    seeds.extend(service.symbol_names().map(str::to_owned));
    seeds.extend(already.iter().map(|solved| solved.name.clone()));
    seeds.sort();
    seeds.dedup();

    let started = std::time::Instant::now();
    let (solved, stats) =
        orbistoun_names::affix::solve_affixed(hasher, targets, &affixes, seeds.iter());
    println!(
        "affixed names: {} tried from {} held name(s) in {:.1}s, {} named",
        stats.tried,
        seeds.len(),
        started.elapsed().as_secs_f64(),
        stats.found
    );
    // Named, and how. The rule is the whole of the claim, so printing the name alone
    // would hide the only part a reader can check by eye.
    for name in &solved {
        if let orbistoun_nid::Method::Affixed { seed, rule } = &name.derivation.method {
            println!("  {} = {rule} applied to {seed}", name.name);
        }
    }
    Ok(solved)
}

/// Every hash this sweep will look for: the corpus's unnamed imports, plus any a report says
/// the platform exports.
///
/// **Unioned into the search, kept out of the work list.** `symbols/wanted.txt` is defined as
/// the imports this project cannot name, and a kernel export nothing has ever imported is not
/// one. Folding them together would change what that file means and quietly move the number
/// every report quotes, so the union lives here and `wanted` is still written from `unnamed`
/// alone (D605).
fn everything_to_look_for(
    unnamed: &[orbistoun_nid::Nid],
    exported: &[orbistoun_nid::Nid],
    from_report: &[std::path::PathBuf],
) -> orbistoun_names::solve::Targets {
    if !exported.is_empty() {
        println!(
            "{} platform export(s) have no name yet, from {} report(s) - searched for, not counted as wanted",
            exported.len(),
            from_report.len()
        );
    }
    let mut hunting: Vec<orbistoun_nid::Nid> = unnamed.to_vec();
    hunting.extend(exported.iter().copied());
    hunting.sort_by_key(|nid| nid.as_raw());
    hunting.dedup();
    orbistoun_names::solve::Targets::new(hunting)
}

/// Every hash a probe report says the platform exports and this project cannot name.
///
/// # Why the origin is not asked for
///
/// Everywhere else a probe report is read, the operator has to say what it ran on, because a
/// measurement taken on a stand-in describes the stand-in (D246). Here the report contributes
/// **which hashes to look for** and nothing else. The name that comes back is proved by the
/// hash agreeing with a candidate this project spelled out of its own vocabulary, and it would
/// be equally proved if the report had been invented - a fabricated hash simply names nothing.
///
/// So the grading question does not arise, and pretending it did would be worse than useless:
/// it would make an unasserted report look like a source of names rather than a source of
/// questions.
fn exported_hashes(
    service: &Service,
    reports: &[std::path::PathBuf],
) -> Result<Vec<orbistoun_nid::Nid>> {
    // Unasserted on purpose - see above. Every export comes back `Oracle::Assumed` and the
    // grade is not read, because nothing here turns on it.
    let origin = orbistoun_probe::Origin::unasserted();
    let mut found = Vec::new();
    for path in reports {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading probe report {}", path.display()))?;
        let transcript = orbistoun_probe::Transcript::read(&text)
            .map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
        let exports = transcript.kernel_exports(&origin);
        let before = found.len();
        found.extend(
            exports
                .iter()
                .filter(|export| service.symbol_name(export.nid).is_none())
                .map(|export| export.nid),
        );
        // Said per report, because a report that carried no export table at all and one whose
        // exports this project already names are different situations reading the same as
        // each other in a total (principle 3).
        println!(
            "{}: {} export(s), {} not named here",
            path.display(),
            exports.len(),
            found.len() - before
        );
    }
    found.sort_by_key(|nid| nid.as_raw());
    found.dedup();
    Ok(found)
}

fn search_word_lists(
    hasher: &orbistoun_nid::NidHasher,
    targets: &orbistoun_names::solve::Targets,
    words: Option<&std::path::Path>,
    words_from: WordSource,
) -> Result<Vec<orbistoun_names::solve::Solved>> {
    // The published list and any supplied list are searched separately, because they
    // are different kinds of claim and must be recorded as such. Merging them is what
    // made supplied names appear to have come from the repository's own word list.
    let mut found: Vec<orbistoun_names::solve::Solved> = Vec::new();
    let today = orbistoun_nid::today();
    let (solved, stats) = orbistoun_names::solve::solve_names(
        hasher,
        targets,
        orbistoun_names::standard_names(),
        &orbistoun_nid::Derivation::new(
            orbistoun_nid::Method::PublishedStandard {
                list: orbistoun_names::solve::STANDARD_LIST.to_owned(),
            },
            &today,
        ),
    );
    println!(
        "published names: {} tried, {} named",
        stats.tried, stats.found
    );
    if stats.found == 0 {
        // Worth saying out loud, because it is the one cheap check on the suffix
        // itself. These names are fixed by published standards, so if a module links a
        // C library and not one of them matches, the names are not what is wrong.
        eprintln!(
            "{}",
            concat!(
                "no published standard name matched; if this module imports a C library, ",
                "that is a strong sign --suffix-hex is wrong"
            )
        );
    }
    found.extend(solved);

    if let Some(words) = words {
        let text = std::fs::read_to_string(words)
            .with_context(|| format!("reading {}", words.display()))?;
        let where_from = words.display().to_string();
        let derivation = match words_from {
            WordSource::Probe => orbistoun_nid::Method::Runtime {
                by: orbistoun_nid::RuntimeSource::ProbeTranscript,
                how: format!(
                    "names read from {where_from}, reported by this project's own conformance probe"
                ),
            },
            WordSource::Supplied => orbistoun_nid::Method::Supplied { source: where_from },
        };
        let (solved, stats) = orbistoun_names::solve::solve_names(
            hasher,
            targets,
            orbistoun_names::word_list(&text),
            &orbistoun_nid::Derivation::new(derivation, &today),
        );
        println!(
            "supplied names: {} tried, {} named ({:?})",
            stats.tried, stats.found, words_from
        );
        found.extend(solved);
    }

    Ok(found)
}

/// Every module a search should read: a directory walked, or one file taken as given.
///
/// A directory is one search, not many - the imports of every module under it are unioned
/// before the expensive sweep, so it runs once (D213). Split out of [`cmd_names`] for
/// length rather than for meaning.
fn modules_to_search(path: &std::path::Path) -> Result<Vec<std::path::PathBuf>> {
    if !path.is_dir() {
        return Ok(vec![path.to_path_buf()]);
    }
    let mut modules = Vec::new();
    collect_modules(path, &mut modules);
    anyhow::ensure!(
        !modules.is_empty(),
        concat!(
            "no guest modules under {} - nothing to search against. The generator ",
            "proposes candidates; confirming one needs a real import table to collide ",
            "with. See docs/PROVENANCE.md"
        ),
        path.display()
    );
    println!("{} modules under {}", modules.len(), path.display());
    Ok(modules)
}

/// `names` - search for names that hash to what a module, or a whole corpus, imports.
///
/// # Why a directory is one search rather than many
///
/// Given a directory, every module beneath it is read, their unnamed imports are unioned,
/// and **one** search answers all of them. That is not a convenience: searching per module
/// re-ran the full 2.6-billion-candidate sweep once for each, forty-two times over a local
/// corpus, while widening the target set costs a sweep nothing - each candidate is one
/// hash-set lookup whether the set holds one hash or four thousand.
///
/// It also makes the corpus a source rather than a list. A name lying in one title's
/// diagnostic text explains an import of a title that never mentions it, which a
/// module-at-a-time search structurally cannot find (D213).
pub(crate) fn cmd_names(service: &Service, cli: &Cli, search: &NameSearch<'_>) -> Result<()> {
    let NameSearch {
        path,
        threads,
        grammar,
        words,
        words_from,
        out,
        wanted,
        from_trace,
        from_report,
    } = *search;
    let hasher = orbistoun_nid::NidHasher::new(suffix_for(cli)?);

    let modules = modules_to_search(path)?;

    let (corpus, unnamed) = read_corpus(service, &modules)?;
    println!(
        "{} distinct imports have no name yet, across {} readable module(s)",
        unnamed.len(),
        corpus.len()
    );

    let exported = exported_hashes(service, from_report)?;
    let targets = everything_to_look_for(&unnamed, &exported, from_report);
    if targets.is_empty() {
        return Ok(());
    }

    let threads = if threads == 0 {
        std::thread::available_parallelism().map_or(1, std::num::NonZero::get)
    } else {
        threads
    };

    let mut found: Vec<orbistoun_names::solve::Solved> = Vec::new();

    // Guest material first: no guess is involved in any of it, and every name it settles
    // is one the far more expensive sweep below no longer has to reach.
    found.extend(search_corpus_strings(&hasher, &corpus, &targets));
    if from_trace {
        found.extend(search_corpus_dumps(&hasher, &corpus, &targets));
    }

    // Published names next. They are not guesses and they cost almost nothing to try.
    found.extend(search_word_lists(&hasher, &targets, words, words_from)?);

    let grammar = match grammar {
        Some(file) => {
            let text = std::fs::read_to_string(file)
                .with_context(|| format!("reading {}", file.display()))?;
            orbistoun_names::Grammar::parse(&text)?
        }
        None => orbistoun_names::Grammar::builtin()?,
    };
    let patterns = grammar.patterns()?;
    let space: u64 = patterns.iter().map(orbistoun_names::Pattern::len).sum();
    println!(
        "generated names: {space} candidates across {} patterns, {threads} threads",
        patterns.len()
    );

    let started = std::time::Instant::now();
    let (solved, stats) =
        orbistoun_names::solve::solve_patterns(&hasher, &targets, &patterns, threads);
    let elapsed = started.elapsed();
    // Integer arithmetic throughout: the count runs into the billions, past where an
    // `f64` holds it exactly, and a throughput figure is only ever read to one or two
    // significant digits anyway.
    let millis = u64::try_from(elapsed.as_millis())
        .unwrap_or(u64::MAX)
        .max(1);
    let rate = stats.tried.saturating_mul(1000) / millis;
    println!(
        "generated names: {} tried in {:.1}s ({rate}/s), {} named",
        stats.tried,
        elapsed.as_secs_f64(),
        stats.found
    );
    found.extend(solved);

    // **Last, because it stands on everything before it.** Every other source proposes a
    // name out of raw material; this one proposes a name out of a name already proved, so
    // it can only be as good as what the run has established by the time it runs.
    found.extend(search_affixes(&hasher, &targets, service, &found)?);

    // A hash can be settled by more than one source in a single run - a string in one
    // module and a published C name both produce `atoll`. Keep the one a reader needs least
    // to check, which is **not** the one that ran first.
    //
    // This used to keep the first, on the stated reasoning that sources run cheapest-first.
    // They do not: strings run before the published list, so every published name that also
    // appears in a module's bytes was recorded as a static harvest - a true record of a
    // weaker claim than the run actually had, and one CI can never recheck. Fourteen names
    // landed that way the first time the console export table made them targets, and the
    // knowledge base caught it by disagreeing (D607).
    found.sort_by(|a, b| {
        a.name.cmp(&b.name).then(
            a.derivation
                .method
                .reproducible()
                .rank()
                .cmp(&b.derivation.method.reproducible().rank()),
        )
    });
    found.dedup_by(|a, b| a.name == b.name);
    print_names_found(&found);
    println!(
        "{} of {} named ({:.1}%)",
        found.len(),
        targets.len(),
        percent(found.len(), targets.len())
    );

    if let Some(out) = out {
        write_symbol_db(out, &suffix_for(cli)?, &found)?;
    }

    if let Some(path) = wanted {
        write_wanted(service, path, &unnamed, &found)?;
        // Beside it, never inside it. The two lists count different things and the file name
        // says which (D619).
        if !exported.is_empty() {
            write_exported(
                service,
                &path.with_file_name("exported-unnamed.txt"),
                &exported,
                &found,
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    /// **A hash the database can already name does not stay on the work list.**
    ///
    /// The failing case, written first. The old rule removed only what a given run had
    /// just solved, and a named hash is never searched for again - so it could never be
    /// solved again, so it was never removed. 116 of 3829 committed entries were exactly
    /// this, against a file header promising they disappear as the vocabulary grows.
    #[test]
    fn a_hash_something_can_already_name_leaves_the_work_list() {
        let carried: std::collections::BTreeSet<u64> = [0x1111, 0x2222, 0x3333].into();
        // Nothing new is unnamed and nothing was solved this run - the only thing that has
        // changed since the list was written is that the vocabulary explains 0x2222.
        let now = super::wanted_now(carried, &[], &[], |nid| nid.as_raw() == 0x2222);
        assert_eq!(now, [0x1111, 0x3333].into());
    }

    /// A hash nothing can name stays, however many runs it survives.
    ///
    /// The other half, and the one that keeps the list a work list rather than an empty
    /// file: purging by "can be named" must not purge what merely was not solved today.
    #[test]
    fn a_hash_nothing_can_name_stays_on_the_work_list() {
        let carried: std::collections::BTreeSet<u64> = [0x1111].into();
        let now = super::wanted_now(carried, &[], &[], |_| false);
        assert_eq!(now, [0x1111].into());
    }

    /// What a run just solved goes, before it reaches the database.
    ///
    /// `is_named` answers from a service built before the search, and the database is
    /// written from `found` afterwards - so for exactly one run these names are known to
    /// nothing that this can ask. Dropping the second rule leaves each new name on the
    /// work list until the run after it.
    #[test]
    fn a_name_found_this_run_goes_before_it_reaches_the_database() {
        let nid = orbistoun_nid::Nid::from_raw(0x4444);
        let found = vec![orbistoun_names::solve::Solved {
            nid,
            name: "sceExample".to_owned(),
            derivation: orbistoun_nid::Derivation::new(
                orbistoun_nid::Method::Generated {
                    pattern: "sce{Verb}".to_owned(),
                    index: 0,
                },
                "2026-08-25",
            ),
        }];
        let now = super::wanted_now([0x4444].into(), &[], &found, |_| false);
        assert!(
            now.is_empty(),
            "a name found this run stayed on the work list"
        );
    }

    /// New unnamed imports join, so the list is the union across every module ever seen.
    #[test]
    fn newly_unnamed_imports_join_the_work_list() {
        let now = super::wanted_now(
            [0x1111].into(),
            &[orbistoun_nid::Nid::from_raw(0x5555)],
            &[],
            |_| false,
        );
        assert_eq!(now, [0x1111, 0x5555].into());
    }

    #[test]
    fn every_module_extension_a_corpus_uses_is_recognised() {
        // **The regression this exists for.** Four hand-written globs in `bin/orbistoun`
        // covered `eboot.bin` and `.prx` and silently omitted `.sprx`, so eleven modules
        // in the local corpus were never searched even once - and a glob that matches
        // nothing is not an error, so nothing ever said so (D213).
        for name in ["eboot.bin", "libc.prx", "libSceAmpr.sprx", "EBOOT.BIN"] {
            assert!(super::is_guest_module(name), "{name} is a guest module");
        }
        for name in ["icon0.png", "param.json", "eboot.bin.bak", "notes.prx.txt"] {
            assert!(!super::is_guest_module(name), "{name} is not");
        }
    }

    #[test]
    fn a_module_reached_two_ways_records_as_one_module() {
        // A tab-completed directory argument left `titles/PPSA21564-app0//eboot.bin` on a
        // hundred provenance records, which reads as a different module from the same
        // file reached without the trailing slash.
        use std::path::Path;
        assert_eq!(
            super::record_path(Path::new("titles/X//eboot.bin")),
            "titles/X/eboot.bin"
        );
        assert_eq!(
            super::record_path(Path::new(r"titles\X\eboot.bin")),
            "titles/X/eboot.bin"
        );
    }

    #[test]
    fn a_dump_decodes_to_the_bytes_it_recorded() {
        assert_eq!(super::decode_hex_bytes("48656c6c6f"), b"Hello");
        // Whitespace and separators are how a dump is actually rendered.
        assert_eq!(super::decode_hex_bytes("48 65 6c 6c 6f"), b"Hello");
        // A trailing half-byte is dropped rather than guessed at: inventing the low
        // nibble would invent an identifier boundary that was never in guest memory.
        assert_eq!(super::decode_hex_bytes("48656c6c6f7"), b"Hello");
        assert!(super::decode_hex_bytes("").is_empty());
    }
}
