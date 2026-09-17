//! Derive knowledge entries from the implementations' own documentation.
//!
//! # Why this exists rather than a person writing them
//!
//! `every_implemented_function_is_written_down` failed for **112 functions**, and the guard is
//! right to: implementing something without recording what was learned is how the knowledge
//! ends up existing only in a conversation. But hand-writing 112 formulaic rows satisfies the
//! guard and defeats what it is for - a row saying "POSIX function, follows POSIX" records
//! nothing a reader did not already know.
//!
//! **The material is already written.** Each of those functions carries a doc comment stating
//! its contract, usually with the specification cited in it, because that is the house style.
//! So this derives the entry from that rather than inventing one, and the derivation is
//! mechanical enough to re-run when the documentation changes.
//!
//! # What it will not do
//!
//! It does not invent provenance. A record whose documentation cites no published
//! specification lands as `assumed`, never `published`, and every record goes through
//! [`KnowledgeFile::merge`] so the format's own provenance rules decide whether it is
//! admissible. **A fault stops the write** rather than being reported and ignored (D180).

use std::collections::{BTreeMap, BTreeSet};

use orbistoun_hle::knowledge::{KnowledgeFile, Oracle, Record, citation_is_a_path};

/// A guest symbol bound to the Rust function that implements it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Binding {
    /// The name the guest imports.
    pub(crate) symbol: String,
    /// The Rust function, last path segment only.
    pub(crate) rust_name: String,
}

/// A symbol declared in a `guest_module!` block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Declaration {
    /// The library the guest imports it from.
    pub(crate) library: String,
    /// The name the guest imports.
    pub(crate) symbol: String,
    /// How many integer arguments it takes.
    pub(crate) arity: u8,
}

/// The documentation attached to each function in one source file.
///
/// **Attachment, not proximity.** A block is attached only to the item that immediately
/// follows it, and any other statement in between clears it - which is what stops an orphaned
/// block (a real and repeated mistake in this codebase) being read as though it documented
/// whatever came next.
///
/// # Why plain `//` comments count, for macro-declared functions only
///
/// A doc comment does not attach to a macro invocation, so the ten float maths functions
/// written by `unary_f32!` document themselves with `//` instead - the information is there
/// and the language will not carry it. Reading those as well is the difference between
/// deriving an entry for them and recording that they have no documentation, which would be
/// false. A `///` block always wins where both are present.
#[must_use]
pub(crate) fn docs_in(source: &str) -> BTreeMap<String, Vec<String>> {
    let mut out = BTreeMap::new();
    let mut doc: Vec<String> = Vec::new();
    let mut plain: Vec<String> = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("///") {
            doc.push(rest.strip_prefix(' ').unwrap_or(rest).to_owned());
            plain.clear();
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("//") {
            plain.push(rest.strip_prefix(' ').unwrap_or(rest).to_owned());
            continue;
        }
        if trimmed.starts_with("#[") {
            continue;
        }
        if let Some(name) = declared_name(trimmed) {
            let attached = if doc.is_empty() { &mut plain } else { &mut doc };
            if !attached.is_empty() {
                out.insert(name, std::mem::take(attached));
            }
            doc.clear();
            plain.clear();
            continue;
        }
        if trimmed.is_empty() {
            // A blank line ends a plain-comment paragraph but not a doc block: Rust carries
            // `///` across one, and a `//` block separated from the item by a blank line is
            // a different remark. Without this the ten maths functions each inherited the
            // section banner above them instead of their own line.
            plain.clear();
            continue;
        }
        doc.clear();
        plain.clear();
    }
    out
}

/// The function a line declares, whether written out or produced by a macro.
fn declared_name(trimmed: &str) -> Option<String> {
    function_name(trimmed).or_else(|| macro_declared_name(trimmed))
}

/// The function a macro invocation names, as `unary_f32!(acosf, acos);` names `acosf`.
///
/// The first argument by convention, which is what every generating macro here uses. A macro
/// that named its function somewhere else would go undocumented rather than misdocumented -
/// the entry would be missing, and the run says so.
fn macro_declared_name(trimmed: &str) -> Option<String> {
    let (macro_name, rest) = trimmed.split_once("!(")?;
    if macro_name.is_empty()
        || !macro_name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return None;
    }
    let first: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    (!first.is_empty()).then_some(first)
}

/// The name of the function a line declares, if it declares one.
fn function_name(trimmed: &str) -> Option<String> {
    let mut rest = trimmed;
    for prefix in [
        "pub(crate) ",
        "pub(super) ",
        "pub ",
        "const ",
        "async ",
        "unsafe ",
    ] {
        rest = rest.strip_prefix(prefix).unwrap_or(rest);
    }
    let rest = rest.strip_prefix("fn ")?;
    let name: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    (!name.is_empty()).then_some(name)
}

/// Every `("symbol", rust_function)` row in a registration table.
///
/// **Bounded to the tables themselves**, which the element type identifies: a registration
/// table is a slice of `(&str, GuestFn)`. Scanning whole files for the row shape instead
/// found sixty-odd tuples that are not registrations at all - fixture data, test vectors, a
/// pair of column headings - and each would have produced a knowledge entry about a function
/// that does not exist.
///
/// **A row is not a line.** `cargo fmt` breaks a long one across four, and a line-based
/// reader silently loses exactly the rows with the longest names - which here meant the
/// attribute accessors, and only the guard downstream noticed. The region is matched as text.
#[must_use]
pub(crate) fn bindings_in(source: &str) -> Vec<Binding> {
    let row = regex::Regex::new(
        r#"\(\s*"([A-Za-z_][A-Za-z0-9_]*)"\s*,\s*([A-Za-z_][A-Za-z0-9_:]*)\s*,?\s*\)"#,
    )
    .expect("the row pattern is a literal");
    let mut out = Vec::new();
    for region in table_regions(source) {
        for found in row.captures_iter(&region) {
            let target = &found[2];
            let rust_name = target.rsplit("::").next().unwrap_or(target);
            out.push(Binding {
                symbol: found[1].to_owned(),
                rust_name: rust_name.to_owned(),
            });
        }
    }
    out
}

/// The body of each registration table in a source file.
fn table_regions(source: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current: Option<String> = None;
    for line in source.lines() {
        if line.contains("GuestFn)]") || line.contains("GuestFloatFn)]") {
            if let Some(region) = current.take() {
                out.push(region);
            }
            current = Some(String::new());
            continue;
        }
        let Some(region) = current.as_mut() else {
            continue;
        };
        if line.starts_with('}') || line.trim() == "];" {
            out.push(current.take().unwrap_or_default());
            continue;
        }
        region.push_str(line);
        region.push('\n');
    }
    out.extend(current);
    out
}

/// Every `("alias", "target")` row in a delegation table.
///
/// A delegated name resolves to the **same function pointer** as the name it points at, so
/// at run time it is implemented and at compile time there is no Rust function called that -
/// which is why a reader looking only for `fn` names records it as missing. The behaviour to
/// write down is the target's.
#[must_use]
pub(crate) fn delegations_in(source: &str) -> Vec<(String, String)> {
    let row = regex::Regex::new(r#"\(\s*"([^"]+)"\s*,\s*"([^"]+)"\s*,?\s*\)"#)
        .expect("the row pattern is a literal");
    let mut out = Vec::new();
    let mut inside = false;
    let mut region = String::new();
    for line in source.lines() {
        if line.contains("&str, &str)] = &[") {
            inside = true;
            continue;
        }
        if !inside {
            continue;
        }
        if line.starts_with('}') || line.trim() == "];" {
            inside = false;
            for found in row.captures_iter(&region) {
                out.push((found[1].to_owned(), found[2].to_owned()));
            }
            region.clear();
            continue;
        }
        region.push_str(line);
        region.push('\n');
    }
    out
}

/// Every symbol declared in a `guest_module!` block, with the library it belongs to.
///
/// Bounded to the macro's own body, because `"name" => 3,` is also the shape of an ordinary
/// match arm and an arity table elsewhere in the same file would otherwise be read as a
/// declaration.
#[must_use]
pub(crate) fn declarations_in(source: &str) -> Vec<Declaration> {
    let mut out = Vec::new();
    let mut inside = false;
    let mut library = String::new();
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("guest_module! {") {
            inside = true;
            continue;
        }
        if !inside {
            continue;
        }
        if line.starts_with('}') {
            inside = false;
            continue;
        }
        if let Some(name) = trimmed.strip_suffix(" {").and_then(quoted) {
            library = name;
            continue;
        }
        for (symbol, arity) in declaration_rows(trimmed) {
            out.push(Declaration {
                library: library.clone(),
                symbol,
                arity,
            });
        }
    }
    out
}

/// The `"name" => arity,` pairs on one line - several share a line in the wider tables.
fn declaration_rows(trimmed: &str) -> Vec<(String, u8)> {
    trimmed
        .split(',')
        .filter_map(|part| {
            let (name, arity) = part.trim().split_once(" => ")?;
            Some((quoted(name)?, arity.trim().parse().ok()?))
        })
        .collect()
}

/// The contents of a `"..."` literal, if that is the whole of the text.
fn quoted(text: &str) -> Option<String> {
    text.trim()
        .strip_prefix('"')?
        .strip_suffix('"')
        .map(ToOwned::to_owned)
}

/// Follows a delegation to the function that actually runs.
///
/// **Transitively, with a bound.** A target can itself be an alias - `posix_close` points at
/// `close`, which points at the vendor spelling - so one hop is not enough, and a table with a
/// cycle in it must not hang a generator.
fn resolve<'a>(
    alias: &str,
    written: &BTreeMap<&'a str, &'a str>,
    aliased: &BTreeMap<&'a str, &'a str>,
) -> Option<(String, &'a str)> {
    /// Longer than any real chain; a table needing more has a cycle in it.
    const HOPS: usize = 8;
    let mut name = *aliased.get(alias)?;
    for _ in 0..HOPS {
        if let Some(rust_name) = written.get(name) {
            return Some((name.to_owned(), rust_name));
        }
        name = *aliased.get(name)?;
    }
    None
}

/// Turns a doc comment into what is known about the function.
#[must_use]
pub(crate) fn record_from(symbol: &str, arity: Option<u8>, doc: &[String]) -> Record {
    let cites = citation(doc);
    let known_by = Some(if cites.is_some() {
        Oracle::Published
    } else {
        Oracle::Assumed
    });
    let assumptions = vec![if cites.is_some() {
        concat!(
            "The published specification is the source. Nothing on the target has confirmed that ",
            "this platform's build follows it, and the failure convention in particular is ",
            "unestablished."
        )
        .to_owned()
    } else {
        concat!(
            "Derived from the implementation's own documentation, which cites no published ",
            "specification. Nothing on the target has confirmed it."
        )
        .to_owned()
    }];
    Record {
        function: symbol.to_owned(),
        arity,
        purpose: purpose(doc),
        edge_cases: edge_cases(doc),
        found_in: Vec::new(),
        known_by,
        cites,
        assumptions,
        note: Some(
            "Derived from the implementation's doc comment by `orbistoun-gen knowledge`."
                .to_owned(),
        ),
    }
}

/// The first thing the documentation says the function is for.
///
/// House style opens with the signature and a dash - ``` `pthread_once(control, routine)` -
/// runs an initialiser exactly once.``` - so the description is what follows it.
///
/// **The signature has to be recognised before the markup is stripped**, because that is the
/// only thing distinguishing it from prose: once the backticks are gone,
/// `scePthreadMutexLock(mutex).` is just a sentence, and a block that opens with a bare
/// signature would record the signature as its own purpose instead of falling through to the
/// paragraph that describes it.
fn purpose(doc: &[String]) -> Option<String> {
    let mut paragraphs = doc
        .split(|line| line.trim().is_empty())
        .filter(|p| !p.iter().all(|l| l.trim().is_empty()) && !p[0].trim_start().starts_with('#'));
    let first = paragraphs.next()?;
    let raw = first.join(" ");
    if let Some((_, rest)) = raw.split_once("` - ") {
        let described = plain(rest);
        if !is_citation_only(&described) {
            return Some(described);
        }
        // The description is the reference itself, so there is no purpose here to record.
        // The next paragraph may still hold one; if it does not, none is recorded at all.
        return paragraphs.next().map(|next| plain(&next.join(" ")));
    }
    if is_bare_signature(&raw) {
        return paragraphs.next().map(|next| plain(&next.join(" ")));
    }
    Some(plain(&raw))
}

/// Whether a paragraph is a signature and nothing else.
fn is_bare_signature(raw: &str) -> bool {
    let text = raw.trim().trim_end_matches('.');
    text.starts_with('`') && text.ends_with('`') && text.matches('`').count() == 2
}

/// The claims the documentation emphasised, which are what a reimplementation gets wrong.
///
/// **Bold is the signal, and it is a convention rather than a guess.** This codebase bolds
/// the load-bearing caveat - the thing that is true and surprising - so the emphasised
/// paragraphs are exactly the edge cases the knowledge file wants. Capped, because a record
/// that quotes an entire doc comment has recorded nothing in particular.
fn edge_cases(doc: &[String]) -> Vec<String> {
    /// How many to keep, and how long each may be.
    const KEEP: usize = 3;
    /// Longer than this is a discussion, not an edge case.
    const LONGEST: usize = 400;
    doc.split(|line| line.trim().is_empty())
        .filter(|p| p.iter().any(|l| l.contains("**")))
        .filter(|p| !p[0].trim_start().starts_with('#'))
        .map(|p| plain(&p.join(" ")))
        .filter(|text| text.len() <= LONGEST)
        .take(KEEP)
        .collect()
}

/// Standards this project treats as citable, longest first so `POSIX.1-2008` wins over a
/// bare `POSIX`, and `ISO/IEC 9899` is listed because that is what the C standard is called
/// when a clause number is being quoted from it.
const STANDARDS: &[&str] = &[
    "POSIX.1-2008",
    "ISO/IEC 9899",
    "ISO C11",
    "ISO C",
    "IEEE 754",
    "FreeBSD",
    "Solaris",
    "C11",
    "POSIX",
];

/// Whether a fragment is a reference and nothing else.
///
/// **A citation is not a purpose.** Plenty of these functions are documented as
/// ``` `pthread_mutex_timedlock(mutex, abstime)` - POSIX.1-2008.``` - the whole content is
/// "it is the standard one" - and recording that as what the function is *for* would put a
/// clause number in the field a reader goes to for the behaviour. Better to leave the purpose
/// empty and let the citation carry it, which is what is actually known.
fn is_citation_only(text: &str) -> bool {
    /// Longer than this is prose that happens to open with a standard's name.
    const SHORT: usize = 60;
    let text = text.trim();
    text.len() <= SHORT && STANDARDS.iter().any(|s| text.starts_with(s))
}

/// Where somebody else can check the claim, if the documentation says.
///
/// An explicit `Reference:` line first, because that is what the style asks for; failing
/// that, a standard named anywhere in the prose. **Never a guess** - no mention means no
/// citation, which lands the record as `assumed` rather than `published`.
fn citation(doc: &[String]) -> Option<String> {
    let joined = plain(&doc.join(" "));
    if let Some(at) = joined.find("Reference: ") {
        let rest = &joined[at + "Reference: ".len()..];
        let end = rest.find(". ").map_or(rest.len(), |i| i + 1);
        let cited = rest[..end].trim();
        // **Asked of the format itself**, rather than guessed at here. A reference citing two
        // clauses at once - `7.27.3.4 (localtime) / 7.27.3.3 (gmtime)` - leaves a bare `/` as
        // a fragment, which is a path by that rule, so this falls back to the standard's name
        // instead. The guard is right and stays as it is; what changed is that this asks it.
        if !cited.is_empty() && !citation_is_a_path(cited) {
            return Some(cited.to_owned());
        }
    }
    // A description that is itself a reference is a better citation than the bare standard
    // name found anywhere in the prose: `ISO/IEC 9899 7.12.4.1.` names the clause.
    let opening = doc.iter().find(|l| !l.trim().is_empty())?;
    if let Some((_, rest)) = opening.split_once("` - ") {
        let described = plain(rest);
        if is_citation_only(&described) && !citation_is_a_path(&described) {
            return Some(described);
        }
    }
    STANDARDS
        .iter()
        .find(|s| joined.contains(**s) && !citation_is_a_path(s))
        .map(|s| (*s).to_owned())
}

/// The prose without its markup, so a record reads as a sentence rather than as markdown.
fn plain(text: &str) -> String {
    text.replace("**", "").replace('`', "").trim().to_owned()
}

/// What a derivation run decided, before anything is written.
#[derive(Debug, Default)]
pub(crate) struct Derived {
    /// The files to write, by library name.
    pub(crate) files: BTreeMap<String, KnowledgeFile>,
    /// How many entries were added.
    pub(crate) added: usize,
    /// Symbols that are implemented but declared by nobody, so nothing can reach them.
    pub(crate) undeclared: BTreeSet<String>,
    /// Symbols whose implementation carries no documentation to derive from.
    pub(crate) undocumented: BTreeSet<String>,
    /// Provenance faults. **Any fault means nothing is written.**
    pub(crate) faults: Vec<String>,
}

/// Derives an entry for every implemented symbol that has none.
///
/// `sources` is every Rust source in the workspace; `existing` is the knowledge files as they
/// stand, keyed by library.
#[must_use]
pub(crate) fn derive(
    sources: &[String],
    existing: &BTreeMap<String, KnowledgeFile>,
    today: &str,
) -> Derived {
    let mut docs = BTreeMap::new();
    let mut bindings = Vec::new();
    let mut declarations = Vec::new();
    let mut delegations = Vec::new();
    let mut delegated: BTreeMap<String, String> = BTreeMap::new();
    for source in sources {
        docs.extend(docs_in(source));
        bindings.extend(bindings_in(source));
        declarations.extend(declarations_in(source));
        delegations.extend(delegations_in(source));
    }
    let written: BTreeMap<&str, &str> = bindings
        .iter()
        .map(|b| (b.symbol.as_str(), b.rust_name.as_str()))
        .collect();
    let aliased: BTreeMap<&str, &str> = delegations
        .iter()
        .map(|(alias, target)| (alias.as_str(), target.as_str()))
        .collect();
    let mut resolved: Vec<Binding> = Vec::new();
    for (alias, _) in &delegations {
        if written.contains_key(alias.as_str()) {
            continue;
        }
        let Some((target, rust_name)) = resolve(alias, &written, &aliased) else {
            continue;
        };
        resolved.push(Binding {
            symbol: alias.clone(),
            rust_name: rust_name.to_owned(),
        });
        delegated.insert(alias.clone(), target);
    }
    drop(written);
    drop(aliased);
    bindings.extend(resolved);
    let declared: BTreeMap<&str, &Declaration> = declarations
        .iter()
        .map(|d| (d.symbol.as_str(), d))
        .collect();
    let recorded: BTreeSet<&str> = existing
        .values()
        .flat_map(|f| f.functions.iter().map(|k| k.name.as_str()))
        .collect();

    let mut out = Derived {
        files: existing.clone(),
        ..Derived::default()
    };
    for binding in &bindings {
        if recorded.contains(binding.symbol.as_str()) {
            continue;
        }
        let Some(declaration) = declared.get(binding.symbol.as_str()) else {
            out.undeclared.insert(binding.symbol.clone());
            continue;
        };
        let Some(doc) = docs.get(&binding.rust_name) else {
            out.undocumented.insert(binding.symbol.clone());
            continue;
        };
        let mut record = record_from(&binding.symbol, Some(declaration.arity), doc);
        if let Some(target) = delegated.get(&binding.symbol) {
            // **Said out loud rather than left to be inferred.** The entry describes the
            // target's behaviour because that is the code that runs, and a reader has to know
            // the description came from somewhere else - not least because the two spellings
            // can differ in arity, which is the fault D385 records.
            record.edge_cases.push(format!(
                "Resolves to `{target}`, and this describes that function."
            ));
            // **The target is deliberately not named in the question.** One sentence used to
            // carry both the fact and the question, and naming the target inside the question
            // made every entry's copy of it a different sentence - so a hundred and forty-nine
            // entries resting on one premise were counted as a hundred and forty-nine
            // premises, and `questions --premises` cannot group what it cannot see is the same
            // sentence (D539). The name is in the edge case above, where a reader needs it.
            record
                .assumptions
                .push(orbistoun_hle::knowledge::DELEGATION_ASSUMPTION.to_owned());
        }
        let file = out
            .files
            .entry(declaration.library.clone())
            .or_insert_with(|| KnowledgeFile {
                library: declaration.library.clone(),
                functions: Vec::new(),
            });
        out.faults.extend(file.merge(&record, today));
        out.added += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{Binding, Declaration, bindings_in, declarations_in, docs_in, record_from};

    const SAMPLE: &str = r#"
guest_module! {
    "libScePosix" {
        "sem_timedwait" => 2,
        "htonl" => 1, "htons" => 1,
    }
}

/// `sem_timedwait(sem, abstime)` - take one, waiting until a deadline.
///
/// **`abstime` is an absolute deadline, not a duration.** Reading it as relative restarts
/// the clock on every retry.
///
/// Reference: POSIX.1-2008 `sem_timedwait`. A deadline already past is a zero wait.
fn sem_timedwait(args: &[u64; 6]) -> u64 { 0 }

/// A helper, which no guest calls.
fn helper() -> u64 { 0 }

const TABLE: &[(&str, GuestFn)] = &[
    ("sem_timedwait", sem_timedwait),
    ("posix_open", "open"),
    (
        "pthread_mutexattr_getprioceiling",
        pthread_mutexattr_getprioceiling,
    ),
];

const FIXTURES: &[(&str, u32)] = &[
    ("42", answer),
];
"#;

    /// A doc block attaches to the function after it, and to nothing else.
    ///
    /// The failure this prevents is the orphaned block: a doc comment separated from its
    /// function by a statement would otherwise be read as documenting whatever came next,
    /// and the record would describe the wrong function entirely.
    #[test]
    fn a_doc_block_attaches_only_to_the_function_that_follows_it() {
        let docs = docs_in(SAMPLE);
        assert!(docs.contains_key("sem_timedwait"));
        assert_eq!(
            docs["helper"],
            vec!["A helper, which no guest calls.".to_owned()]
        );
        assert!(
            !docs["sem_timedwait"].iter().any(|l| l.contains("helper")),
            "the blocks must not run together"
        );
    }

    /// Registration rows are found; a delegation row, and a tuple outside a table, are not.
    ///
    /// `("posix_open", "open")` maps a name to another *name*, so reading it as a function
    /// would look up a doc comment that does not exist. And `FIXTURES` has the row shape
    /// exactly while being test data - the bound on the element type is the only thing
    /// telling them apart, which is why it exists.
    #[test]
    fn a_delegation_row_is_not_a_binding() {
        let found = bindings_in(SAMPLE);
        assert_eq!(
            found,
            vec![
                Binding {
                    symbol: "sem_timedwait".to_owned(),
                    rust_name: "sem_timedwait".to_owned(),
                },
                // Split across four lines by `cargo fmt`, and lost entirely by the reader
                // that worked a line at a time.
                Binding {
                    symbol: "pthread_mutexattr_getprioceiling".to_owned(),
                    rust_name: "pthread_mutexattr_getprioceiling".to_owned(),
                },
            ]
        );
    }

    /// A delegation names a symbol, and the behaviour to record is the target's.
    #[test]
    fn delegations_are_read_as_pairs_of_names() {
        let source =
            "const DELEGATED: &[(&str, &str)] = &[\n    (\"inet_pton\", \"__inet_pton\"),\n];\n";
        assert_eq!(
            super::delegations_in(source),
            vec![("inet_pton".to_owned(), "__inet_pton".to_owned())]
        );
    }

    /// Declarations are read from the macro body, several to a line where they share one.
    #[test]
    fn declarations_carry_their_library_and_arity() {
        let found = declarations_in(SAMPLE);
        assert_eq!(
            found,
            vec![
                Declaration {
                    library: "libScePosix".to_owned(),
                    symbol: "sem_timedwait".to_owned(),
                    arity: 2
                },
                Declaration {
                    library: "libScePosix".to_owned(),
                    symbol: "htonl".to_owned(),
                    arity: 1
                },
                Declaration {
                    library: "libScePosix".to_owned(),
                    symbol: "htons".to_owned(),
                    arity: 1
                },
            ]
        );
    }

    /// The record says what the documentation says, and cites what it cited.
    #[test]
    fn a_record_carries_the_purpose_the_citation_and_the_emphasised_caveat() {
        let docs = docs_in(SAMPLE);
        let record = record_from("sem_timedwait", Some(2), &docs["sem_timedwait"]);

        assert_eq!(
            record.purpose.as_deref(),
            Some("take one, waiting until a deadline.")
        );
        assert_eq!(
            record.cites.as_deref(),
            Some("POSIX.1-2008 sem_timedwait."),
            "the explicit Reference line, not the bare standard name"
        );
        assert_eq!(
            record.known_by,
            Some(orbistoun_hle::knowledge::Oracle::Published)
        );
        assert!(
            record
                .edge_cases
                .iter()
                .any(|e| e.contains("absolute deadline")),
            "the bolded caveat is the edge case: {:?}",
            record.edge_cases
        );
    }

    /// **Documentation citing nothing lands as `assumed`, never `published`.**
    ///
    /// The guard that stops this being a machine for manufacturing provenance. A record is
    /// only as strong as the source it names, and a doc comment naming no standard names no
    /// source - so the tier has to drop, and the assumption has to say why.
    #[test]
    fn documentation_that_cites_nothing_is_assumed_rather_than_published() {
        let doc = vec!["`whatever(a)` - does a thing nobody has written down.".to_owned()];
        let record = record_from("whatever", Some(1), &doc);
        assert_eq!(
            record.known_by,
            Some(orbistoun_hle::knowledge::Oracle::Assumed)
        );
        assert_eq!(record.cites, None);
        assert!(
            record.assumptions[0].contains("cites no published specification"),
            "and it says so: {:?}",
            record.assumptions
        );
    }

    /// A function documented only by its reference records the reference, not a purpose.
    ///
    /// **The failure this prevents is a clause number in the purpose field.** A reader goes
    /// there for what the function does; `POSIX.1-2008.` answers a different question, and
    /// answering the wrong question in a field is worse than leaving it empty.
    #[test]
    fn a_reference_is_recorded_as_a_citation_rather_than_as_a_purpose() {
        let doc = vec!["`pthread_mutex_timedlock(mutex, abstime)` - POSIX.1-2008.".to_owned()];
        let record = record_from("pthread_mutex_timedlock", Some(2), &doc);
        assert_eq!(
            record.purpose, None,
            "no purpose is stated, so none is recorded"
        );
        assert_eq!(record.cites.as_deref(), Some("POSIX.1-2008."));
        assert_eq!(
            record.known_by,
            Some(orbistoun_hle::knowledge::Oracle::Published)
        );
    }

    /// The C standard is citable under the name a clause is quoted from.
    #[test]
    fn the_formal_name_of_the_c_standard_is_recognised() {
        let doc = vec!["`acosf(x)` - ISO/IEC 9899 7.12.4.1.".to_owned()];
        let record = record_from("acosf", Some(1), &doc);
        assert_eq!(
            record.cites.as_deref(),
            Some("ISO/IEC 9899 7.12.4.1."),
            "the clause, not the bare standard name"
        );
        assert_eq!(
            record.known_by,
            Some(orbistoun_hle::knowledge::Oracle::Published),
            "a named clause is a published source"
        );
    }

    /// A macro-declared function is documented by the `//` comment above it.
    ///
    /// The case that made this necessary: ten float maths functions are written by
    /// `unary_f32!`, a doc comment will not attach to a macro invocation, so their
    /// documentation is a plain comment. Reading only `///` recorded them as undocumented,
    /// which was false.
    #[test]
    fn a_macro_declared_function_carries_the_comment_above_it() {
        let source = "// `acosf(x)` - ISO/IEC 9899 7.12.4.1.\nunary_f32!(acosf, acos);\n";
        let docs = docs_in(source);
        assert_eq!(
            docs.get("acosf").map(Vec::as_slice),
            Some(["`acosf(x)` - ISO/IEC 9899 7.12.4.1.".to_owned()].as_slice())
        );
    }

    /// A reference spanning two clauses is cited by its standard, not refused.
    ///
    /// `localtime`'s reference names two ISO C clauses separated by a slash, and the format
    /// refuses a citation containing one - the rule that stops a filesystem path being passed
    /// off as a source. **The guard stays exactly as it is**; the derivation falls back to the
    /// standard's name, which is true and admissible.
    #[test]
    fn a_two_clause_reference_falls_back_to_the_standard_rather_than_faulting() {
        let doc = vec![
            "`localtime(timer)` - a `time_t` broken down.".to_owned(),
            String::new(),
            "Reference: ISO C 7.27.3.4 (`localtime`) / 7.27.3.3 (`gmtime`).".to_owned(),
        ];
        let record = record_from("localtime", Some(1), &doc);
        let cites = record.cites.expect("a citation is still recorded");
        assert!(
            !orbistoun_hle::knowledge::citation_is_a_path(&cites),
            "and it is admissible: {cites}"
        );
        assert_eq!(cites, "ISO C");
    }

    /// A block opening with a bare signature takes its purpose from the paragraph after.
    #[test]
    fn a_bare_signature_falls_through_to_the_description() {
        let doc = vec![
            "`scePthreadMutexLock(mutex)`.".to_owned(),
            String::new(),
            "Takes the lock, blocking until it is free.".to_owned(),
        ];
        let record = record_from("scePthreadMutexLock", Some(1), &doc);
        assert_eq!(
            record.purpose.as_deref(),
            Some("Takes the lock, blocking until it is free.")
        );
    }
}
