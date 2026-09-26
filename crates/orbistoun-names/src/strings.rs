//! Candidate names read out of a guest module's own bytes.
//!
//! Naming an import is generate-and-test against a one-way hash, so what matters is whether the
//! true name is in the candidate set. Diagnostic format strings and assertion text leave literal
//! function names in a module's data, which is the vendor's own spelling (`Sema`, where the
//! vocabulary had only `Semaphore`). Modules carry no text symbol tables, so this reads data.
//! Nothing is consulted: the bytes are the guest's and the hash confirms or rejects each string
//! (D180). A confirmed name's parts are fed back into the vocabulary, so the name becomes
//! generable from this repository and the provenance audit can account for it without the title.

/// Shortest run worth trying.
///
/// Shorter runs are mostly printable machine code; the hash rejects noise anyway, so this keeps
/// the candidate count sane on a large binary.
const MIN_LENGTH: usize = 5;

/// Longest run worth keeping.
///
/// Longer runs are sentences or paths. A name embedded in a longer string is still found, since
/// a bracket, space or dot ends the run.
const MAX_LENGTH: usize = 64;

/// Every identifier-shaped run of bytes in `image`, deduplicated.
///
/// Shape-based: `"scePthreadMutexattrInit(&mutexAttr) returned %s"` yields
/// `scePthreadMutexattrInit` because `(` is not an identifier character, with no parsing of
/// engine-specific formats.
pub fn candidates(image: &[u8]) -> Vec<String> {
    let mut out = std::collections::BTreeSet::new();
    let mut run = Vec::with_capacity(MAX_LENGTH);

    for &byte in image {
        if is_identifier_byte(byte) {
            // Runs longer than the ceiling are dropped whole: a truncated identifier never existed.
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
        // A name never starts with a digit, which removes many packed integers with printable bytes.
        if run
            .first()
            .is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_')
        {
            if let Ok(text) = std::str::from_utf8(run) {
                // obSCEne's corpus module has private `obs_` symbols that no platform library exports.
                // Harvesting one would put a non-symbol in the database, and its deliberately absent census
                // control would then resolve as present (D392).
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
/// A name read from a module names one import; its parts reach every import built from the same
/// words, from the repository alone (D213). Split on capitals, as the vendor composes: a
/// lowercase prefix, then words, so `sceKernelCreateSema` yields `Kernel`, `Create`, `Sema`.
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
    // The lowercase lead-in is the prefix, which the grammar already holds.
    parts
        .into_iter()
        .filter(|p| p.chars().next().is_some_and(char::is_uppercase))
        .collect()
}

/// A line break, named so it survives being written by a generator.
const NEWLINE: char = '\n';

/// What a vocabulary round may cost before it is unaffordable.
///
/// The accepted cost of the shapes that use `learned` twice with the curated list, about 2.6
/// billion candidates; a vocabulary that pushes a round past it is refused (D330).
const ROUND_CEILING: u64 = 2_600_000_000;

/// What a vocabulary round would sweep with `learned` at a given size.
///
/// A round re-sweeps every shape using the grown slot at full size, which is the cost growing
/// `learned` raises, so only shapes containing `learned` are counted.
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
/// Read from the text, since this runs while deciding what the text becomes and no parsed
/// grammar exists yet.
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

/// Why a set of words was not added, with the numbers, so the choice (curate the words, or drop
/// a shape that uses the slot twice) is actionable.
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
    /// Adding them would have made the search unaffordable; a variant rather than `None`, so a
    /// refusal is never read as nothing new (D330).
    Refused(Refusal),
}

/// Adds words to the grammar's `learned` vocabulary, in place.
///
/// Written to the file rather than suggested, so the loop closes unattended; it is data, so
/// nothing rebuilds. Kept apart from the hand-written lists because a chosen word and a harvested
/// word are different claims, and because `learned` has a cost ceiling (D330). Returns nothing
/// new, a rewritten grammar, or a refusal.
pub fn learn_words(grammar: &str, words: &[String], injected: &[String]) -> Learned {
    let existing = current_words(grammar, "learned");
    let existing_len = existing.len();
    // Checked against every list, and against `injected`, the `posix` words built at load time and
    // absent from the file text; a word the grammar can already spell adds no reachable names.
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

    // Costed before it is written: a round past the ceiling is refused out loud.
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

    // With no list to replace, it goes just after the vocabulary table opens, beside its
    // neighbours.
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
/// The vendor composes from capitalised words. A digit followed by two lowercase letters is
/// Itanium mangling (length then text, as in `Agent6enable`), which no `sce*` export contains
/// and which multiplies a quadratic sweep. Two lowercase letters rather than one keeps real
/// words such as `Audio3d`.
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

/// The header [`render_list`] writes above a generated list.
///
/// One definition, because `find_list` must claim exactly what `render_list` writes, or each
/// regeneration stacks another header.
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
    // Back over the header, in reverse, so a regeneration replaces it rather than stacking on it.
    for line in GENERATED_HEADER.iter().rev() {
        let full = format!("{line}{NEWLINE}");
        if grammar[..from].ends_with(&full) {
            from -= full.len();
        }
    }
    // The first `]` that ends a line: a one-line list such as `prefix = ["sce"]` has no `]` on a
    // line of its own, and looking only for that would swallow the next list.
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

    /// Regenerating a list replaces its header rather than stacking another one.
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
        // Nothing around it was eaten.
        assert!(
            grammar.contains("[vocabulary]"),
            "the section header was lost"
        );
        assert!(
            grammar.contains("other = [\"x\"]"),
            "the next list was lost"
        );
    }

    /// A name inside a format string is recovered.
    #[test]
    fn a_name_inside_a_format_string_is_recovered() {
        // The case this exists for: a diagnostic string carrying the function name in the module's
        // bytes.
        let image = b"[SCE] scePthreadMutexattrInit(&mutexAttr) returned %s in %s(%d)";
        let found = candidates(image);
        assert!(found.iter().any(|c| c == "scePthreadMutexattrInit"));
        assert!(found.iter().any(|c| c == "mutexAttr"), "and its neighbours");
    }

    /// obSCEne's own `obs_` symbols are not harvested.
    #[test]
    fn obscenes_own_symbols_are_not_harvested() {
        // `obs_` symbols are skipped (D392), and a real platform name beside them is still taken.
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

    /// Runs are bounded at both ends.
    #[test]
    fn runs_are_bounded_at_both_ends() {
        // Short runs are printable machine code and long ones are sentences; every candidate costs a
        // hash.
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

    /// A run that overflows leaves no tail behind.
    #[test]
    fn a_run_that_overflows_does_not_leave_a_tail_behind() {
        // Truncating a long run would emit its tail as though it were an identifier.
        let image = format!("{}Sema", "q".repeat(300)).into_bytes();
        assert!(candidates(&image).is_empty());
    }

    /// A candidate never starts with a digit.
    #[test]
    fn a_candidate_never_starts_with_a_digit() {
        // Removes runs that are packed integers with printable bytes.
        assert!(
            !candidates(b" 123456abcdef ")
                .iter()
                .any(|c| c.starts_with('1'))
        );
    }

    /// The parts of a confirmed name are the words the generator lacked.
    #[test]
    fn the_parts_of_a_confirmed_name_are_the_words_the_generator_was_missing() {
        // `Sema` was the missing word, so `sceKernelCreateSema` could not be generated.
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

    /// A new word is written into the learned list.
    #[test]
    fn a_new_word_is_written_into_the_learned_list() {
        // The write path, which a live run reaches only when a title yields a new name, is pinned
        // here.
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

    /// Nothing is written when every word is already known.
    #[test]
    fn nothing_is_written_when_every_word_is_already_known() {
        // A no-op is reported as nothing rather than an identical string, so the caller need not diff a
        // tracked file.
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

    /// A word the grammar can already spell is not duplicated into `learned`.
    #[test]
    fn a_word_the_grammar_can_already_spell_is_not_duplicated_into_learned() {
        // `Create` is in the hand-written verb list, so adding it again adds nothing.
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

    /// The list is created when the grammar has none.
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

    /// Fragments are refused, so the file stays readable.
    #[test]
    fn fragments_are_refused_so_the_file_stays_readable() {
        // Fragments would accumulate in a file a person still reads.
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

    /// What is written still parses as a grammar.
    #[test]
    fn what_is_written_can_still_be_parsed_as_a_grammar() {
        // A grammar the next run cannot read would break the search one run later than the cause; the
        // caller re-parses before writing, and this pins the shape it relies on.
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

    /// A length-prefixed mangling fragment is not a word.
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

    /// A vendor word carrying digits is still a word.
    #[test]
    fn a_vendor_word_carrying_digits_is_still_a_word() {
        for word in ["Api2", "Attribute2", "Http2", "Audio3d", "Sha256"] {
            assert!(super::is_word(word), "{word} is real vendor vocabulary");
        }
    }

    /// A one-line vocabulary list is read as itself, not run on into the next list (D330).
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

    /// The ceiling admits the curated list and refuses a harvest (D330).
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

    /// A harvest large enough to break the search is refused with the numbers (D330).
    #[test]
    fn a_harvest_large_enough_to_break_the_search_is_refused_out_loud() {
        let grammar = crate::DEFAULT_VENDOR_GRAMMAR;
        // Enough to push a round past the ceiling, shaped like the harvest's output.
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
