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
/// Accumulates: each run sees only the imports of the module it was given, so writing only the
/// run's findings would discard every name learned elsewhere. An existing derivation wins over a
/// new one for the same name, because a record says when a name was first worked out (D213).
fn write_symbol_db(
    path: &std::path::Path,
    suffix: &[u8],
    found: &[orbistoun_names::solve::Solved],
) -> Result<()> {
    // The suffix actually hashed with, not the raw argument, which is empty when the shipped
    // default is used; a database with no suffix cannot be loaded.
    let suffix_hex = orbistoun_nid::encode_hex(suffix);
    let mut file = match std::fs::read_to_string(path) {
        Ok(text) => orbistoun_nid::SymbolDbFile::from_json(&text)
            .with_context(|| format!("parsing the existing {}", path.display()))?,
        // Absent is the ordinary first run. Any other error is real, so a permissions fault does
        // not silently start from nothing.
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
            // Written alongside the name by the code that found it, so the provenance is checkable
            // (D213).
            file.derivations
                .insert(solved.name.clone(), solved.derivation.clone());
        }
    }
    // Sorted, so a diff shows what was learned rather than the search order.
    file.names = known.into_iter().collect();

    // Names only in `names`. NIDs are derived, never stored, so a file cannot hold a pair that does
    // not hash to itself (docs/SYMBOLS.md).
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
/// A separate file from `wanted.txt`, which means imports orbistoun cannot name; a kernel export
/// nothing imported is not one, and folding them in would change that file's meaning. Written down
/// because the report directory is overwritten. Accumulated, never truncated: a hash leaves only
/// when something names it, as in `write_wanted`.
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
    // The same rule and the same function as `wanted_now`, so the two lists agree on what "still
    // unnamed" means.
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
/// Persisted so the next round of vocabulary work has a list to aim at.
fn write_wanted(
    service: &Service,
    path: &std::path::Path,
    unnamed: &[orbistoun_nid::Nid],
    found: &[orbistoun_names::solve::Solved],
) -> Result<()> {
    use std::fmt::Write as _;

    // One line per entry rather than a continued literal, which would carry the source indentation
    // into the file.
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

    // Accumulated across runs like the database: each run sees one module, and the work list is the
    // union of what every module has wanted.
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
/// Pure and separate from the file so the rule is tested. The rule is "cannot be named now", not
/// "was never solved": a named hash is excluded from every later search, so a rule keyed on this
/// run's results would never remove it.
fn wanted_now(
    mut carried: std::collections::BTreeSet<u64>,
    unnamed: &[orbistoun_nid::Nid],
    found: &[orbistoun_names::solve::Solved],
    is_named: impl Fn(orbistoun_nid::Nid) -> bool,
) -> std::collections::BTreeSet<u64> {
    carried.extend(unnamed.iter().map(|nid| nid.as_raw()));
    carried.retain(|raw| !is_named(orbistoun_nid::Nid::from_raw(*raw)));
    // This run's own results too: the database is written from `found` after this, and the service
    // answering `is_named` was built before either.
    for solved in found {
        carried.remove(&solved.nid.as_raw());
    }
    carried
}

/// Adds confirmed words to the grammar file, and says how many were new.
///
/// Writes the shipped grammar in place. That file is tracked data, so a run changes what the next
/// one can reach (D195).
fn learn_vocabulary(words: &[String]) -> Result<usize> {
    let path = std::path::Path::new("crates/orbistoun-names/data/vendor.toml");
    if !path.exists() {
        // Running from an installed binary rather than a checkout: the names are still confirmed
        // and written, and only the grammar cannot grow.
        return Ok(0);
    }
    let before =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    // The built-in POSIX vocabulary too, which is not in the file and would otherwise be re-learned
    // word by word.
    let injected = orbistoun_names::posix_vocabulary();
    let after = match orbistoun_names::strings::learn_words(&before, words, &injected) {
        orbistoun_names::strings::Learned::Nothing => return Ok(0),
        orbistoun_names::strings::Learned::Grammar(after) => after,
        // Printed, not returned as zero: a refusal must not read as "nothing new".
        orbistoun_names::strings::Learned::Refused(refusal) => {
            println!("  {}", refusal.say());
            return Ok(0);
        }
    };
    // Parsed before it is trusted: a grammar this cannot read would break the next search.
    let now = orbistoun_names::Grammar::parse(&after)
        .map(|g| g.vocabulary.get("learned").map_or(0, Vec::len))
        .context("the widened grammar no longer parses - not written")?;
    let was = orbistoun_names::Grammar::parse(&before)
        .ok()
        .and_then(|g| g.vocabulary.get("learned").map(Vec::len))
        .unwrap_or(0);
    let added = now.saturating_sub(was);

    // The cost, reported where the vocabulary grows. A refusal is loud (D330); this makes the cost
    // of an accepted widening visible too.
    let before_cost = orbistoun_names::strings::round_cost(&before, was);
    let after_cost = orbistoun_names::strings::round_cost(&after, now);
    println!(
        "  learned {was} -> {now} words; a vocabulary round goes {before_cost} -> {after_cost} candidates"
    );
    if before_cost > 0 && after_cost / before_cost >= 2 {
        // A doubling is where somebody should look, not an error. Said once, with the multiple.
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
/// The one implementation, covering `eboot.bin`, `.prx` and `.sprx`. Case-insensitive, because the
/// corpus is read off a filesystem that does not care and `EBOOT.BIN` is the same module.
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
/// Hand-rolled for the same reason as `audit::find_symbol_maps`.
fn collect_modules(root: &std::path::Path, found: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        // An unreadable directory costs its own modules, not the run.
        return;
    };
    // Sorted, so the search order - and which module is credited for a name two modules contain -
    // does not depend on the filesystem.
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
/// Normalised, so the same module reached two ways (a trailing slash, a Windows separator) is one
/// record. Relative to the title library, never absolute: a module in the shared library is
/// recorded as `titles/<id>/...`, so no machine path enters the tracked database.
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
    /// The candidate strings are harvested on a second pass, once every module's targets are known;
    /// holding every module's candidates in memory instead is the wrong trade.
    file: std::path::PathBuf,
    /// The hashes it imports and nothing yet names.
    unnamed: std::collections::HashSet<u64>,
}

/// Reads every module's unnamed imports, so one search can answer all of them.
///
/// A wider target set costs a search nothing - each candidate is one hash-set lookup - while
/// searching per module repeats the whole sweep for each.
fn read_corpus(
    service: &Service,
    modules: &[std::path::PathBuf],
) -> Result<(Vec<ModuleImports>, Vec<orbistoun_nid::Nid>)> {
    let mut per_module = Vec::new();
    let mut union: std::collections::BTreeSet<u64> = std::collections::BTreeSet::new();
    for file in modules {
        let bytes = std::fs::read(file).with_context(|| format!("reading {}", file.display()))?;
        // A module that will not parse is skipped, not fatal: previous-generation containers are
        // expected in a real corpus. It is reported, so it does not look like a module with nothing
        // to contribute.
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
/// The source decides whether a name may be used: module strings from another emulator's binary are
/// that project's name list, which is refused (D242).
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

/// How many names each source settled, so a total is not read as what this repository can account
/// for.
fn print_names_per_source(found: &[orbistoun_names::solve::Solved]) {
    // Counted per source as well as in total.
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
/// A name in one title's diagnostic text is the vendor's own spelling of a function every title may
/// import, so every module's strings are tested against every module's unnamed imports. Both cases
/// are [`orbistoun_nid::Method::Static`], a hash match; they are recorded as different sources
/// because one module explains its own import and the other needs another module's material.
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
            // Which source it is, decided by the module that wants the hash rather than the one
            // carrying the string; the other way round, every shared string would count as
            // cross-module.
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

    // A confirmed name's parts are words the generator lacked; adding them makes other names built
    // from them reachable by generation, which the provenance audit can account for without the
    // title (D195).
    let mut words: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for solved in &found {
        words.extend(orbistoun_names::strings::parts_of(&solved.name));
    }
    // Written, not suggested, so the next title does not hit the same gap unattended. Data rather
    // than code, so nothing rebuilds.
    if !words.is_empty() {
        let list: Vec<String> = words.into_iter().collect();
        // Never fatal: the names are already confirmed and written, and a failed widening costs
        // only the next search.
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
/// Reads the bytes a pointer argument pointed at, captured by the dispatch path at call time
/// (D194): guest memory as mapped and relocated, which can hold text no module contains as a
/// literal, such as a path assembled at runtime or a string a decompressor produced.
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

    // Both forms, because they can differ: `text` is what the dump renderer found readable, `bytes`
    // is everything. Separated by a zero so a run cannot straddle two captures and invent an
    // identifier.
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
/// One line, including when the answer is nothing: a source that silently contributes nothing looks
/// like one that is working.
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
        // Said explicitly, because "0 named" reads the same from a working source and a
        // misconfigured one. Dumps are forced per import, so most are scalars with no bytes.
        println!(
            "  (dumps are forced per-import - see WORKFLOW.md. Most captured so far are scalars carrying no text)"
        );
    }
    found
}

/// Bytes back out of the hex a dump is stored as.
///
/// Anything that is not a clean pair of hex digits is dropped: a mis-decoded byte would invent an
/// identifier boundary.
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
/// Runs last because it proposes names from names already proved, so it can seed from this run's
/// findings. Seeds are the harvested standard list, the loaded database and this sweep's proved
/// names; a candidate is never a seed (D606). One pass: a variant of a variant becomes reachable on
/// the next run, once its seed is in the database.
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
    // Named with the rule, because the rule is the whole of the claim.
    for name in &solved {
        if let orbistoun_nid::Method::Affixed { seed, rule } = &name.derivation.method {
            println!("  {} = {rule} applied to {seed}", name.name);
        }
    }
    Ok(solved)
}

/// Every hash this sweep will look for: the corpus's unnamed imports, plus any a report says the
/// platform exports.
///
/// Unioned into the search but kept out of the work list: `symbols/wanted.txt` is the imports this
/// project cannot name, and is still written from `unnamed` alone.
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
/// The operator's assertion is not asked for: the report contributes only which hashes to look for,
/// and a name that comes back is proved by the hash matching a candidate from this project's own
/// vocabulary. A fabricated hash names nothing.
fn exported_hashes(
    service: &Service,
    reports: &[std::path::PathBuf],
) -> Result<Vec<orbistoun_nid::Nid>> {
    // Unasserted on purpose; the grade is not read.
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
        // Said per report: a report with no export table and one whose exports are all named would
        // read the same in a total.
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

/// Searches the published word list, then any list the caller supplied.
///
/// Kept apart because they are different claims - one fixed by published standards, the other
/// whatever a caller handed over - and are recorded under different derivations (D213).
fn search_word_lists(
    hasher: &orbistoun_nid::NidHasher,
    targets: &orbistoun_names::solve::Targets,
    words: Option<&std::path::Path>,
    words_from: WordSource,
) -> Result<Vec<orbistoun_names::solve::Solved>> {
    // The published list and any supplied list are searched separately and recorded as different
    // kinds of claim.
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
        // The one cheap check on the suffix: these names are fixed by published standards, so if a
        // module links a C library and none matches, the suffix is wrong.
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
/// A directory is one search: every module's imports are unioned before the sweep runs once.
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
/// Given a directory, every module beneath it is read, their unnamed imports are unioned, and one
/// search answers them all; widening the target set costs a sweep nothing. It also lets one title's
/// strings explain another title's imports.
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

    // Guest material first: no guess is involved, and every name it settles leaves the expensive
    // sweep.
    found.extend(search_corpus_strings(&hasher, &corpus, &targets));
    if from_trace {
        found.extend(search_corpus_dumps(&hasher, &corpus, &targets));
    }

    // Published names next: not guesses, and nearly free to try.
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
    // Integer arithmetic: the count runs into the billions, past exact `f64`, and throughput is
    // read to one or two significant digits.
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

    // Last, because it proposes names out of names already proved.
    found.extend(search_affixes(&hasher, &targets, service, &found)?);

    // A hash can be settled by more than one source in one run - a string in one module and a
    // published C name both produce `atoll`. Keep the strongest claim, the one a reader needs least
    // to check, not the one that ran first.
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
        // Beside the database, never inside it: the two lists count different things.
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
    /// A hash the database can already name does not stay on the work list.
    #[test]
    fn a_hash_something_can_already_name_leaves_the_work_list() {
        let carried: std::collections::BTreeSet<u64> = [0x1111, 0x2222, 0x3333].into();
        // Nothing new is unnamed and nothing was solved this run; only the vocabulary now explains
        // 0x2222.
        let now = super::wanted_now(carried, &[], &[], |nid| nid.as_raw() == 0x2222);
        assert_eq!(now, [0x1111, 0x3333].into());
    }

    /// A hash nothing can name stays, however many runs it survives.
    #[test]
    fn a_hash_nothing_can_name_stays_on_the_work_list() {
        let carried: std::collections::BTreeSet<u64> = [0x1111].into();
        let now = super::wanted_now(carried, &[], &[], |_| false);
        assert_eq!(now, [0x1111].into());
    }

    /// What a run just solved goes, before it reaches the database.
    ///
    /// `is_named` answers from a service built before the search, so for one run these names are
    /// known to nothing this can ask.
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

    /// Every module extension a corpus uses is recognised, `.sprx` and upper case included.
    #[test]
    fn every_module_extension_a_corpus_uses_is_recognised() {
        for name in ["eboot.bin", "libc.prx", "libSceAmpr.sprx", "EBOOT.BIN"] {
            assert!(super::is_guest_module(name), "{name} is a guest module");
        }
        for name in ["icon0.png", "param.json", "eboot.bin.bak", "notes.prx.txt"] {
            assert!(!super::is_guest_module(name), "{name} is not");
        }
    }

    /// A module reached with a doubled separator records as the same module.
    #[test]
    fn a_module_reached_two_ways_records_as_one_module() {
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

    /// A dump decodes to the bytes it recorded, dropping anything that is not a hex pair.
    #[test]
    fn a_dump_decodes_to_the_bytes_it_recorded() {
        assert_eq!(super::decode_hex_bytes("48656c6c6f"), b"Hello");
        // Whitespace and separators, as a dump is rendered.
        assert_eq!(super::decode_hex_bytes("48 65 6c 6c 6f"), b"Hello");
        // A trailing half-byte is dropped rather than guessed at.
        assert_eq!(super::decode_hex_bytes("48656c6c6f7"), b"Hello");
        assert!(super::decode_hex_bytes("").is_empty());
    }
}
