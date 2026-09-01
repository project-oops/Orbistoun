//! Candidate names read out of a guest module's own bytes.
//!
//! # Why this is the strongest source in the project
//!
//! Naming an import is generate-and-test against a one-way hash, so the only question that
//! ever matters is whether the true name is in the candidate set. Everything else - the
//! grammar, the vocabulary, the shapes - exists to make that set likely to contain it.
//!
//! **A title carries the answer.** Diagnostic format strings and assertion text leave
//! literal function names in the binary, and a name lying in the module's own bytes is not
//! a guess about the vendor's naming: it is the vendor's naming, sitting in a file this
//! project already parses.
//!
//! Symbol tables would be a better source still, and there are none. Every module in the
//! local corpus was checked: five carry a `.shstrtab` and nothing else, no module has a
//! `.symtab`, and the dynamic symbol names are the encoded-hash form rather than text. So
//! this reads data, not tables - which is worth stating, because "and symbol tables" sat in
//! this paragraph for some time describing a source that does not exist here (D213).
//!
//! `sceKernelCreateSema` is the case that motivated this. It blocked two titles for weeks,
//! the generator could not produce it, and the string was in a *third* title's data the
//! whole time. The vocabulary had `Semaphore` and not `Sema`, so
//! `sceKernelCreateSemaphore` was generated and tested and the real name was never in the
//! set - a gap no amount of added guesswork closes reliably, because the missing thing was
//! a spelling rather than an idea (D193).
//!
//! # This is clean-room, and it is worth being precise about why
//!
//! Nothing is consulted. The bytes are the guest's, already read for its import table, and
//! the hash confirms or rejects every candidate exactly as it does for a generated one. A
//! string that hashes to a wanted import *is* that import's name; a string that does not is
//! discarded. There is no database, no other project's source, and no recall - which is the
//! distinction principle 1 draws (D180).
//!
//! What it is not is *re-derivable from this repository alone*, because the module is not
//! in the repository and never will be. That is why confirmed names feed their parts back
//! into the vocabulary: once `Sema` is a word the grammar knows, the name becomes
//! generable, and the provenance audit can account for it without the title.

/// Shortest run worth trying.
///
/// Four characters and below is mostly noise - fragments of machine code that happen to be
/// printable - and the hash rejects noise anyway, so the only cost of a bad candidate is
/// time. Chosen to keep the candidate count sane on a thirty-megabyte binary rather than
/// because a shorter name would be wrong.
const MIN_LENGTH: usize = 5;

/// Longest run worth keeping.
///
/// Beyond this the run is a sentence or a path, not an identifier. A real name embedded in
/// a longer string is still found, because the characters that terminate an identifier -
/// a bracket, a space, a dot - end the run at the right place.
const MAX_LENGTH: usize = 64;

/// Every identifier-shaped run of bytes in `image`, deduplicated.
///
/// Deliberately shape-based rather than clever. A format string like
/// `"scePthreadMutexattrInit(&mutexAttr) returned %s"` yields `scePthreadMutexattrInit` for
/// free, because `(` is not an identifier character - no parsing of the surrounding text is
/// needed, and none would be reliable across the formats different engines use.
pub fn candidates(image: &[u8]) -> Vec<String> {
    let mut out = std::collections::BTreeSet::new();
    let mut run = Vec::with_capacity(MAX_LENGTH);

    for &byte in image {
        if is_identifier_byte(byte) {
            // Runs longer than the ceiling are dropped whole rather than truncated: a
            // truncated identifier is a name that never existed, and would be tested
            // against every wanted hash for nothing.
            if run.len() <= MAX_LENGTH {
                run.push(byte);
            } else {
                run.clear();
                run.push(u8::MAX); // poisons the run so the tail is not kept either
            }
            continue;
        }
        take(&mut run, &mut out);
    }
    take(&mut run, &mut out);
    out.into_iter().collect()
}

/// Whether a byte can appear inside a C identifier.
const fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Ends the current run, keeping it if it is the right shape.
fn take(run: &mut Vec<u8>, out: &mut std::collections::BTreeSet<String>) {
    if (MIN_LENGTH..=MAX_LENGTH).contains(&run.len()) {
        // A name never starts with a digit, which removes a large share of the runs that
        // are really packed integers with printable bytes.
        if run
            .first()
            .is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_')
        {
            if let Ok(text) = std::str::from_utf8(run) {
                // obSCEne's own binary is a corpus module too, and its private symbols carry the
                // `obs_` prefix - report helpers, harness internals, and the census controls -
                // which no platform library exports. Harvesting one puts a name in the database
                // that is not a real symbol, and its deliberately-absent census control then
                // reports *present* under this project's own resolver: the exact leak that made
                // `900-surface/control` fail even under `named` (D392). The prefix is the tell.
                if !text.starts_with("obs_") {
                    out.insert(text.to_owned());
                }
            }
        }
    }
    run.clear();
}

/// The parts of a confirmed name, for feeding back into the candidate vocabulary.
///
/// # Why a confirmed name is worth more than itself
///
/// A name read out of a module names one import. Its *parts* name every other import built
/// from the same words - and they are what the generator was missing. `sceKernelCreateSema`
/// contributes `Sema`, and every `sce…Sema…` name in every other title becomes reachable by
/// generation, from the repository alone, without that title.
///
/// This is what turns the observation into something the provenance audit can account for
/// (D119): the name stops being "seen in a file nobody tracks" and becomes "generable from
/// a vocabulary that is tracked".
///
/// Split on capitals, because that is how the vendor composes: a leading lowercase prefix,
/// then words. `sceKernelCreateSema` yields `Kernel`, `Create`, `Sema`.
pub fn parts_of(name: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    for ch in name.chars() {
        if ch.is_ascii_uppercase() && !current.is_empty() {
            parts.push(std::mem::take(&mut current));
        }
        if ch == '_' {
            if !current.is_empty() {
                parts.push(std::mem::take(&mut current));
            }
            continue;
        }
        current.push(ch);
    }
    if !current.is_empty() {
        parts.push(current);
    }
    // The lowercase lead-in is the prefix, which the grammar already holds; everything
    // after it is a word the vocabulary may be missing.
    parts
        .into_iter()
        .filter(|p| p.chars().next().is_some_and(char::is_uppercase))
        .collect()
}

/// A line break, named so it survives being written by a generator.
const NEWLINE: char = '\n';

/// What a round would cost before it becomes unaffordable.
///
/// **Taken from this project's own accepted number rather than picked.** D262 costed the
/// shapes a 177-word `learned` unlocked at 2,537,649,000 candidates and called that "one
/// afternoon" - affordable, and the basis for adding the shapes that use the slot twice. So
/// that is the ceiling: a vocabulary that pushes a round past what was already agreed to be
/// affordable is one nobody agreed to.
const ROUND_CEILING: u64 = 2_600_000_000;

/// What a vocabulary round would sweep with `learned` at a given size.
///
/// # Why a round rather than the whole space
///
/// A shape has two costs and they rank differently (D264). The whole space is what a full
/// sweep pays; a **round** re-sweeps every shape that uses the grown slot, at full size, and
/// that is the one growing `learned` makes worse. Costing the wrong one is how a shape that
/// took 67% of every round read as +11% and was added.
///
/// Only shapes containing `learned` are counted, for the same reason. Every other shape
/// generates exactly what it generated before.
#[must_use]
pub fn round_cost(grammar: &str, learned: usize) -> u64 {
    let mut total: u64 = 0;
    for parts in pattern_parts(grammar) {
        if !parts.iter().any(|p| p == "learned") {
            continue;
        }
        let mut candidates: u64 = 1;
        for part in &parts {
            let size = if part == "learned" {
                learned as u64
            } else {
                current_words(grammar, part).len() as u64
            };
            candidates = candidates.saturating_mul(size.max(1));
        }
        total = total.saturating_add(candidates);
    }
    total
}

/// The `parts = [...]` of every pattern in the grammar text.
///
/// Read from the text rather than a parsed grammar because this runs *while deciding what
/// the text should become* - there is no parsed grammar to consult yet, and building one
/// from a file about to change would be costing the wrong thing.
fn pattern_parts(grammar: &str) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    for line in grammar.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("parts = [") else {
            continue;
        };
        let Some(inside) = rest.strip_suffix(']') else {
            continue;
        };
        out.push(
            inside
                .split(',')
                .map(|p| p.trim().trim_matches('"').to_owned())
                .filter(|p| !p.is_empty())
                .collect(),
        );
    }
    out
}

/// Why a set of words was not added.
///
/// **A refusal that says the numbers**, because "too expensive" is not actionable and
/// "338 billion against a ceiling of 2.6 billion" is. The choice it leaves is a real one:
/// curate the words, or drop a shape that uses the slot twice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    /// How many words were offered.
    pub adding: usize,
    /// How large `learned` would have become.
    pub would_be: usize,
    /// What a round costs now.
    pub now: u64,
    /// What it would have cost.
    pub after: u64,
}

impl Refusal {
    /// What a person needs to read.
    #[must_use]
    pub fn say(&self) -> String {
        format!(
            concat!(
                "refused {} new word(s): `learned` would go from {} to {}, taking a ",
                "vocabulary round from {} to {} candidates - {}x, against a ceiling of {}.\n",
                "  A round re-sweeps every shape using the slot at full size (D264), and ",
                "`learned` appears twice in two shapes, so the cost squares.\n",
                "  Either curate the words, or drop a shape that uses it twice."
            ),
            self.adding,
            self.would_be - self.adding,
            self.would_be,
            self.now,
            self.after,
            self.after / self.now.max(1),
            ROUND_CEILING
        )
    }
}

/// What happened when words were offered to the grammar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Learned {
    /// Nothing offered was new.
    Nothing,
    /// The grammar text, with the words added.
    Grammar(String),
    /// Adding them would have made the search unaffordable.
    ///
    /// **A variant rather than a `None`.** The harvest regrew `learned` from 177 words to
    /// 11,842 once already, and nothing said so - the only instrument that noticed was
    /// wall-clock time on an unrelated test, four days later (D320).
    Refused(Refusal),
}

/// Adds words to the grammar's `learned` vocabulary, in place.
///
/// # Why the loop has to close here
///
/// A name read out of a module names one import and stops. Its *parts* are what the
/// generator was missing, and until they reach the grammar the same gap reappears on the
/// next title - the search that could not spell `Sema` could not spell it twice.
///
/// Written to the file rather than reported, because a suggestion a person has to act on is
/// a step that does not happen at three in the morning. This is data, not code (principle
/// 5), so nothing rebuilds and nothing needs reviewing before it takes effect.
///
/// # Kept separate from the hand-written vocabulary, deliberately
///
/// Two reasons, and both matter. **Provenance**: a word somebody chose and a word a module
/// yielded are different claims, and merging them loses the distinction permanently.
/// **Cost**: the `object` list is squared by one of the patterns, so growing it grows the
/// search quadratically. `learned` was used only by patterns that take it once (D195); two
/// shapes now take it twice, which is what makes the ceiling below load-bearing rather than
/// precautionary (D262).
///
/// # What it returns, and why not an `Option`
///
/// Three outcomes, not two. Nothing new, a rewritten file, and **refused** - a set of words
/// that would take a vocabulary round past what anybody agreed to pay. An `Option` folded the
/// third into the first, and "nothing was new" is exactly what a silent refusal looks like:
/// the harvest regrew this list from 177 words to 11,842 once, and the only instrument that
/// noticed was wall-clock time on an unrelated test four days later (D320, D330).
pub fn learn_words(grammar: &str, words: &[String], injected: &[String]) -> Learned {
    let existing = current_words(grammar, "learned");
    let existing_len = existing.len();
    // Checked against every list, not just this one. A word the grammar can already spell
    // adds candidates that were always reachable and makes the file read as though the
    // vocabulary were twice the size it is.
    //
    // **`injected` is the half this could not see.** `posix` holds every harvested standard
    // name, capitalised, and it is built at load time rather than written here - so reading
    // the file text alone, this function was blind to three thousand words the grammar
    // already had, and re-learned them as though they were new (D258).
    let spellable: Vec<String> = ["module", "verb", "object", "tail", "prefix", "learned"]
        .iter()
        .flat_map(|list| current_words(grammar, list))
        .chain(injected.iter().cloned())
        .collect();
    let mut fresh: Vec<&String> = words
        .iter()
        .filter(|w| !spellable.contains(*w) && is_word(w))
        .collect();
    fresh.sort();
    fresh.dedup();
    if fresh.is_empty() {
        return Learned::Nothing;
    }

    let mut all: Vec<String> = existing
        .into_iter()
        .chain(fresh.into_iter().cloned())
        .collect();
    all.sort();
    all.dedup();

    // **Costed before it is written, not after somebody notices.** The harvest took this
    // list from 177 words to 11,842 once, taking a round from 350 million candidates to 1.5
    // trillion - and nothing said so. The only instrument that reported it was wall-clock
    // time on an unrelated test, four days later (D320, D330).
    let now = round_cost(grammar, existing_len);
    let after = round_cost(grammar, all.len());
    if after > ROUND_CEILING {
        return Learned::Refused(Refusal {
            adding: all.len().saturating_sub(existing_len),
            would_be: all.len(),
            now,
            after,
        });
    }

    let rendered = render_list("learned", &all);

    // First time round there is no list to replace, so it is placed just after the
    // vocabulary table opens - with its neighbours, rather than after the patterns where it
    // would read as unrelated.
    if let Some((from, to)) = find_list(grammar, "learned") {
        return Learned::Grammar(format!("{}{rendered}{}", &grammar[..from], &grammar[to..]));
    }
    let at = grammar.find("[vocabulary]").map_or(0, |i| {
        grammar[i..]
            .find(NEWLINE)
            .map_or(grammar.len(), |n| i + n + 1)
    });
    Learned::Grammar(format!(
        "{}
{rendered}{}",
        &grammar[..at],
        &grammar[at..]
    ))
}

/// The words in one vocabulary list of the shipped grammar, for tests and for costing.
#[must_use]
pub fn words_in(grammar: &str, list: &str) -> Vec<String> {
    current_words(grammar, list)
}

/// Whether a candidate is a word rather than a fragment.
///
/// The vendor composes from capitalised words, so anything else came from a run of bytes
/// that happened to look like one. Cheap to check and it keeps the file readable, which
/// matters for a file a person still has to be able to read.
///
/// **A digit followed by two lowercase letters is C++ mangling, not a word.** Itanium
/// encodes an identifier as its length then its text, so `Agent6enable`, `Agent2gc` and
/// `Document9terminate` are one symbol cut at a boundary that is not a word boundary. A
/// guest module is full of them and no `sce*` export can contain one, so every one that got
/// in was pure cost - and cost that squares, because `learned` appears twice in two shapes.
/// 6,451 of the 11,845 entries this let through had that shape, and they took a vocabulary
/// round from 350 million candidates to 1.5 trillion (D320).
///
/// **Two lowercase letters rather than one, and `Audio3d` is why.** The obvious rule rejects
/// it, and it is a real module word behind 168 confirmed names - the kind of entry D259
/// watched strand seven names when it went missing. Requiring two still refuses 6,253 of the
/// 6,451 fragments and refuses **none** of the 468 words the shipped grammar holds, which is
/// the direction to be wrong in: a fragment that survives costs a sweep, a word that does not
/// survive costs a name.
fn is_word(w: &str) -> bool {
    w.len() >= 2
        && w.chars().next().is_some_and(char::is_uppercase)
        && w.chars().all(|c| c.is_ascii_alphanumeric())
        && !w.as_bytes().windows(3).any(|run| {
            run[0].is_ascii_digit() && run[1].is_ascii_lowercase() && run[2].is_ascii_lowercase()
        })
}

/// The words already in one vocabulary list.
fn current_words(grammar: &str, name: &str) -> Vec<String> {
    let Some((from, to)) = find_list(grammar, name) else {
        return Vec::new();
    };
    grammar[from..to]
        .split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect()
}

/// Where one vocabulary list starts and ends, including its trailing newline.
/// The header [`render_list`] writes above a generated list.
///
/// **One definition, because two functions have to agree about it.** `find_list` returns
/// the span that `render_list`'s output replaces, so anything the one writes that the
/// other does not claim is left behind. It did not claim these two lines, so every
/// regeneration prepended a fresh copy above the last - the file grew two comment lines
/// per run, and had two copies by the time anybody read it.
const GENERATED_HEADER: [&str; 2] = [
    "# Words this project read out of guest modules and confirmed by hash.",
    "# GENERATED by `orbistoun-cli names` - hand-written words belong above.",
];

fn find_list(grammar: &str, name: &str) -> Option<(usize, usize)> {
    let needle = format!(
        "
{name} = ["
    );
    let mut from = grammar.find(&needle)? + 1;
    // Back over the header, so a regeneration replaces it rather than stacking on it.
    // In reverse, because they are being peeled off the end of what precedes the list.
    for line in GENERATED_HEADER.iter().rev() {
        let full = format!("{line}{NEWLINE}");
        if grammar[..from].ends_with(&full) {
            from -= full.len();
        }
    }
    // **The first `]` that ends a line, not the first one on a line of its own.**
    //
    // A one-line list - `prefix = ["sce"]`, `none = [""]` - has no `\n]` anywhere near it, so
    // looking only for that ran the span on to the *next* list's closing bracket and swallowed
    // it whole. `current_words(grammar, "prefix")` therefore answered 76 words for a list
    // holding one, which made every cost computed from it 76 times too large - and was
    // invisible for as long as the only caller passed multi-line lists (D330).
    let close = grammar[from..]
        .match_indices(']')
        .find(|(at, _)| {
            let rest = &grammar[from + at + 1..];
            rest.is_empty() || rest.starts_with(NEWLINE)
        })
        .map(|(at, _)| from + at + 1)?;
    let end = grammar[close..]
        .find(NEWLINE)
        .map_or(grammar.len(), |n| close + n + 1);
    Some((from, end))
}

/// One vocabulary list, as it should appear in the file.
fn render_list(name: &str, words: &[String]) -> String {
    use core::fmt::Write as _;

    let mut out = String::new();
    for line in GENERATED_HEADER {
        let _ = writeln!(out, "{line}");
    }
    let _ = writeln!(out, "{name} = [");
    for w in words {
        let _ = writeln!(out, "  \"{w}\",");
    }
    out.push_str(
        "]
",
    );
    out
}

#[cfg(test)]
mod tests {
    use super::{GENERATED_HEADER, candidates, find_list, parts_of, render_list};

    /// **Regenerating a list replaces its header rather than stacking another one.**
    ///
    /// `find_list` gives the span `render_list`'s output replaces. It used to start at
    /// `name = [`, while `render_list` wrote two comment lines above that - so each run
    /// left the previous header in place and added its own. The committed vocabulary had
    /// two copies before anyone noticed, and it grows by two lines per run, forever.
    #[test]
    fn regenerating_a_list_does_not_stack_another_header() {
        let mut grammar =
            "[vocabulary]\n\nthing = [\n  \"One\",\n]\n\nother = [\"x\"]\n".to_owned();
        for _ in 0..3 {
            let (from, to) = find_list(&grammar, "thing").expect("the list is found");
            grammar.replace_range(from..to, &render_list("thing", &["One".to_owned()]));
        }
        assert_eq!(
            grammar.matches(GENERATED_HEADER[1]).count(),
            1,
            "three regenerations left more than one header:\n{grammar}"
        );
        // And nothing around it was eaten on the way.
        assert!(
            grammar.contains("[vocabulary]"),
            "the section header was lost"
        );
        assert!(
            grammar.contains("other = [\"x\"]"),
            "the next list was lost"
        );
    }

    #[test]
    fn a_name_inside_a_format_string_is_recovered() {
        // **The case this exists for.** Unity's diagnostic carries the function name and
        // the emulator only saw it by implementing `printf` and waiting for an error path.
        // The bytes were there regardless.
        let image = b"[SCE] scePthreadMutexattrInit(&mutexAttr) returned %s in %s(%d)";
        let found = candidates(image);
        assert!(found.iter().any(|c| c == "scePthreadMutexattrInit"));
        assert!(found.iter().any(|c| c == "mutexAttr"), "and its neighbours");
    }

    #[test]
    fn obscenes_own_symbols_are_not_harvested() {
        // obSCEne's binary is a corpus module, and its private symbols use the `obs_` prefix -
        // including the census control that must stay absent. Harvesting one puts a non-symbol
        // in the database, which then reports present under our own resolver (D392). A real
        // platform name sitting right beside it is still taken.
        let image = b"obs_census_control_absent sceKernelGetProcessTime obs_report_measure";
        let found = candidates(image);
        assert!(
            !found.iter().any(|c| c.starts_with("obs_")),
            "an obs_ name was harvested: {found:?}"
        );
        assert!(
            found.iter().any(|c| c == "sceKernelGetProcessTime"),
            "a real platform name beside it is still taken"
        );
    }

    #[test]
    fn runs_are_bounded_at_both_ends() {
        // Short runs are mostly machine code that happens to be printable, and long ones
        // are sentences. Neither can be a name, and every candidate costs a hash.
        let long = "x".repeat(200);
        let image = format!("abc {long} sceKernelCreateSema").into_bytes();
        let found = candidates(&image);
        assert!(found.iter().any(|c| c == "sceKernelCreateSema"));
        assert!(!found.iter().any(|c| c == "abc"), "too short");
        assert!(
            !found.iter().any(|c| c.len() > 64),
            "a truncated identifier is a name that never existed"
        );
    }

    #[test]
    fn a_run_that_overflows_does_not_leave_a_tail_behind() {
        // Truncating a long run would emit its last 64 characters as though they were an
        // identifier, and that fragment would be tested against every wanted hash forever.
        let image = format!("{}Sema", "q".repeat(300)).into_bytes();
        assert!(candidates(&image).is_empty());
    }

    #[test]
    fn a_candidate_never_starts_with_a_digit() {
        // Removes a large share of runs that are really packed integers with printable
        // bytes in them.
        assert!(
            !candidates(b" 123456abcdef ")
                .iter()
                .any(|c| c.starts_with('1'))
        );
    }

    #[test]
    fn the_parts_of_a_confirmed_name_are_the_words_the_generator_was_missing() {
        // `Semaphore` was in the vocabulary and `Sema` was not, so
        // `sceKernelCreateSemaphore` was generated and tested while the real name was never
        // in the candidate set. Feeding the parts back is what stops that recurring.
        assert_eq!(
            parts_of("sceKernelCreateSema"),
            vec!["Kernel", "Create", "Sema"]
        );
        assert_eq!(
            parts_of("pthread_mutexattr_settype"),
            Vec::<String>::new(),
            "an all-lowercase name contributes no vendor-shaped words"
        );
    }

    #[test]
    fn a_new_word_is_written_into_the_learned_list() {
        // **The write path, which a live run only exercises when a title yields a name
        // nothing has seen before.** That is the rare case by design, so it is pinned here
        // rather than left to chance - a feature whose only proof is an event that may not
        // happen for weeks is a feature nobody knows is broken.
        let before = "[vocabulary]
prefix = [\"sce\"]

learned = [
  \"Equeue\",
]
";
        let after = match super::learn_words(before, &["Sema".to_owned()], &[]) {
            super::Learned::Grammar(text) => text,
            other => panic!("written: {other:?}"),
        };
        assert!(after.contains("\"Sema\""), "the new word is present");
        assert!(after.contains("\"Equeue\""), "and the old one survives");
        assert!(
            after.contains("prefix = [\"sce\"]"),
            "and the rest of the file is intact"
        );
    }

    #[test]
    fn nothing_is_written_when_every_word_is_already_known() {
        // Returning `None` rather than an identical string is what lets the caller say
        // "learned 0" without diffing a file, and stops a no-op run touching a tracked file
        // and looking like a change.
        let grammar = "[vocabulary]
learned = [
  \"Sema\",
]
";
        assert!(matches!(
            super::learn_words(grammar, &["Sema".to_owned()], &[]),
            super::Learned::Nothing
        ));
    }

    #[test]
    fn a_word_the_grammar_can_already_spell_is_not_duplicated_into_learned() {
        // `Create` lives in the hand-written verb list. Adding it again would grow the
        // candidate space by nothing and make the file read as twice the vocabulary it is.
        let grammar = "[vocabulary]
verb = [
  \"Create\",
]
";
        assert!(matches!(
            super::learn_words(grammar, &["Create".to_owned()], &[]),
            super::Learned::Nothing
        ));
    }

    #[test]
    fn the_list_is_created_when_the_grammar_has_none() {
        // The first run against a fresh grammar has nothing to replace.
        let grammar = "[vocabulary]
prefix = [\"sce\"]
";
        let after = match super::learn_words(grammar, &["Sema".to_owned()], &[]) {
            super::Learned::Grammar(text) => text,
            other => panic!("created: {other:?}"),
        };
        assert!(after.contains("learned = ["));
        assert!(after.contains("\"Sema\""));
    }

    #[test]
    fn fragments_are_refused_so_the_file_stays_readable() {
        // Runs of bytes that merely look like words would accumulate forever in a file a
        // person still has to be able to read.
        let grammar = "[vocabulary]
learned = [
]
";
        for junk in ["x", "lowercase", "Has_Underscore", ""] {
            assert!(
                matches!(
                    super::learn_words(grammar, &[junk.to_owned()], &[]),
                    super::Learned::Nothing
                ),
                "{junk:?} should be refused"
            );
        }
    }

    #[test]
    fn what_is_written_can_still_be_parsed_as_a_grammar() {
        // **The one that matters most.** A grammar the next run cannot read would break the
        // search rather than widen it, and the failure would arrive one run later than the
        // cause. The caller re-parses before writing; this pins the shape it relies on.
        let grammar = crate::DEFAULT_VENDOR_GRAMMAR;
        let after = match super::learn_words(grammar, &["Zzunlikelyword".to_owned()], &[]) {
            super::Learned::Grammar(text) => text,
            other => panic!("written: {other:?}"),
        };
        let parsed = crate::Grammar::parse(&after).expect("still parses");
        assert!(
            parsed.vocabulary["learned"]
                .iter()
                .any(|w| w == "Zzunlikelyword"),
            "and the word is reachable by the generator"
        );
        parsed.patterns().expect("patterns still resolve");
    }

    /// **Mangled C++ is not vocabulary, and it is what a guest module is full of.**
    ///
    /// Itanium encodes an identifier as its length followed by its text, so a symbol cut
    /// at a camel-case boundary leaves `Agent6enable` behind. This filter accepted every
    /// one of them: 6,451 of the 11,845 entries it let into `learned` had that shape.
    ///
    /// The cost is not linear. `learned` appears **twice** in two shapes, so the sweep is
    /// quadratic in it - the junk took a vocabulary round from 350 million candidates to
    /// 1.5 trillion, and the test suite from seconds to over fifteen minutes (D320).
    ///
    /// Asserted as a refusal rather than as a count of what survives, because a filter is
    /// only interesting where it says no.
    #[test]
    fn a_length_prefixed_mangling_fragment_is_not_a_word() {
        for fragment in [
            "Agent6enable",
            "Agent2gc",
            "Document9terminate",
            "Layer18accumulated",
            "L8password",
            "Names11minsize",
        ] {
            assert!(
                !super::is_word(fragment),
                "{fragment} is a mangled symbol cut at a boundary, not a vendor word"
            );
        }
    }

    /// **Digits stay.** The vendor writes them, and the rule has to leave those alone.
    ///
    /// Every word here is in the shipped vocabulary. A filter that rejected them would
    /// strand names proved by hash long ago, which is the failure D259 recorded and this
    /// one is deliberately narrower than.
    #[test]
    fn a_vendor_word_carrying_digits_is_still_a_word() {
        for word in ["Api2", "Attribute2", "Http2", "Audio3d", "Sha256"] {
            assert!(super::is_word(word), "{word} is real vendor vocabulary");
        }
    }

    /// **A one-line list is one word, and it used to be seventy-six.**
    ///
    /// `prefix = ["sce"]` has no `]` on a line of its own, so a span looking only for that ran
    /// on into the next list and swallowed it. Invisible while every caller passed multi-line
    /// lists, and it made the first cost computed from these numbers 76 times too large
    /// (D330).
    #[test]
    fn a_one_line_vocabulary_list_is_read_as_itself() {
        let grammar = crate::DEFAULT_VENDOR_GRAMMAR;

        assert_eq!(super::words_in(grammar, "prefix"), vec!["sce".to_owned()]);
        assert_eq!(super::words_in(grammar, "none"), vec![String::new()]);
        assert!(
            super::words_in(grammar, "module").len() > 50,
            "and a multi-line list still reads whole"
        );
    }

    /// **The shipped grammar is affordable, and a harvest into it is not.**
    ///
    /// The two numbers that matter, pinned together so neither can drift alone. `learned` is
    /// curated at 177 words; the string harvest offered 5,592 once and nothing stopped it,
    /// which took a round from millions to hundreds of billions and was noticed four days
    /// later by a slow test (D320).
    #[test]
    fn the_ceiling_admits_the_curated_list_and_refuses_a_harvest() {
        let grammar = crate::DEFAULT_VENDOR_GRAMMAR;
        let curated = super::words_in(grammar, "learned").len();

        let now = super::round_cost(grammar, curated);
        let harvested = super::round_cost(grammar, 5_592);

        assert!(
            now < super::ROUND_CEILING,
            "the shipped grammar has to be runnable: {now} against {}",
            super::ROUND_CEILING
        );
        assert!(
            harvested > super::ROUND_CEILING * 10,
            "and a harvest has to be refused with room to spare: {harvested}"
        );
    }

    /// **The ceiling refuses a harvest, and says the numbers.**
    ///
    /// The end-to-end path, because `round_cost` being right is not the same as `learn_words`
    /// consulting it. A gate nobody has watched refuse something is a gate nobody knows
    /// anything about (principle 3) - and this one exists because the harvest regrew `learned`
    /// from 177 words to 11,842 with nothing to stop it (D320, D330).
    #[test]
    fn a_harvest_large_enough_to_break_the_search_is_refused_out_loud() {
        let grammar = crate::DEFAULT_VENDOR_GRAMMAR;
        // Enough to push a round past the ceiling, and shaped like the harvest's own output.
        let flood: Vec<String> = (0..2_000).map(|n| format!("Zzword{n:04}")).collect();

        let super::Learned::Refused(refusal) = super::learn_words(grammar, &flood, &[]) else {
            panic!("two thousand new words must not be written silently");
        };

        let said = refusal.say();
        assert!(said.contains("refused"), "{said}");
        assert!(
            said.contains(&refusal.after.to_string()),
            "the refusal has to carry the number it refused on: {said}"
        );
        assert!(
            refusal.after > refusal.now,
            "and the number has to be the larger one"
        );
        assert!(
            said.contains("curate") || said.contains("drop a shape"),
            "a refusal without a way forward is an obstacle: {said}"
        );
    }

    /// A handful of words is still accepted, so the ceiling is not a wall.
    #[test]
    fn a_few_words_still_get_through() {
        let grammar = crate::DEFAULT_VENDOR_GRAMMAR;

        assert!(matches!(
            super::learn_words(grammar, &["Zzsingularword".to_owned()], &[]),
            super::Learned::Grammar(_)
        ));
    }
}
