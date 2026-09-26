//! Proposing words, never names.
//!
//! A model is asked for vocabulary (`Sema`, `Attr`, `Prio`), never an identifier. The words go
//! into the grammar, the grammar generates candidates, and a candidate becomes a name only when
//! its hash matches one a real module imports. Such a name is recorded `generated` at a pattern
//! and an index, so `orbistoun-cli audit` re-derives it like any other (D214). [`prompt`] never
//! shows a hash, a wanted function or a mapping, and a test asserts it carries no hash. The
//! oracle is a hash collision, so a wrong word costs only a sweep; what a model buys is proposing
//! the right words sooner.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::time::Instant;

use orbistoun_llm::{Ask, Request};
use orbistoun_names::Grammar;
use orbistoun_names::solve::{SearchStats, Solved, Targets, solve_patterns};
use orbistoun_nid::NidHasher;

use crate::bank::Bank;
use crate::{Error, Refusal, Rejected};

/// The vocabulary list new words are added to.
///
/// `learned` is where words split out of confirmed names already go (D195), so a proposed word
/// that works is indistinguishable from one harvested that way.
pub const DEFAULT_SLOT: &str = "learned";

/// How freely to sample.
///
/// Not zero, unlike the rest of the workspace: the hash judges every proposal, so a round needs
/// variety, and greedy decoding repeats words within a round as well as between rounds.
pub const DEFAULT_TEMPERATURE: f32 = 0.9;

/// Most new words one round may add.
///
/// A ceiling, not a target: the sweep grows with the vocabulary, and the last suggestions in a
/// long list are the worst.
pub const DEFAULT_BUDGET: usize = 40;

/// Shortest and longest a single word may be; anything longer is a phrase or a whole name.
pub const WORD_LENGTH: std::ops::RangeInclusive<usize> = 2..=24;

/// How the reply was read.
///
/// Recorded so a model that never produces the strict shape is visible; a silent fallback would
/// hide a prompt or model-size problem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Parsed {
    /// A JSON array, as asked for.
    JsonArray,
    /// Quoted strings scraped out of surrounding prose.
    QuotedStrings,
    /// Bare word-shaped tokens, as a last resort.
    BareTokens,
}

/// What the model is told.
///
/// Everything here describes the convention, never which function is wanted, so the model
/// continues a naming style rather than recalling an answer.
#[derive(Debug, Clone, Default)]
pub struct Context {
    /// Library names the modules declare, which carry the vendor's own vocabulary.
    pub libraries: Vec<String>,
    /// Names already confirmed, shown so the convention can be inferred from evidence
    /// rather than described in prose.
    pub examples: Vec<String>,
    /// Optional free text about what these imports appear to do - a call position, an argument
    /// shape, the subsystem they cluster in.
    pub theme: Option<String>,
    /// What kind of word the slot being extended holds - an action, a thing acted on, a suffix
    /// that modifies the meaning.
    ///
    /// Telling the model which position it fills asks a far narrower question than asking for more
    /// words in general.
    pub role: Option<String>,
    /// How many words to ask for.
    pub want: usize,
}

/// What one round did.
#[derive(Debug, Clone)]
pub struct Round {
    /// The prompt, kept so a round is reproducible rather than merely reported.
    pub asked: String,
    /// Which configured backend answered.
    pub backend: String,
    /// The exact model.
    pub model: String,
    /// How the reply was read.
    pub parsed_as: Parsed,
    /// How many words came back before anything was filtered.
    pub offered: usize,
    /// Words that reached the sweep.
    pub tried: Vec<String>,
    /// Words that did not, and why.
    pub rejected: Vec<Rejected>,
    /// Names proved by the hash.
    pub solved: Vec<Solved>,
    /// Proposed words that appear in a proved name.
    ///
    /// Reporting only: the names are authoritative, and a word could be credited for appearing
    /// inside a longer word it did not contribute.
    pub kept: Vec<String>,
    /// What the sweep cost and found.
    pub stats: SearchStats,
    /// How long the sweep took.
    pub swept_ms: u128,
    /// How many kept words were ones the bank did not already hold: whether anything was learned.
    pub banked: usize,
}

impl Round {
    /// True when the round produced nothing at all.
    ///
    /// Rounds that keep coming back empty mean the vocabulary is exhausted for this target set.
    pub fn is_empty(&self) -> bool {
        self.solved.is_empty()
    }
}

/// Proposes words, sweeps with them, and keeps what the hash confirms.
#[derive(Debug)]
pub struct Vocabulary<'a> {
    asker: &'a dyn Ask,
    grammar: Grammar,
    hasher: NidHasher,
    slot: String,
    threads: usize,
    budget: usize,
    temperature: f32,
    seed: u64,
    bank: Option<Bank>,
    /// Every word already swept in this run, whether it worked or not.
    ///
    /// Session memory, not the bank: the bank holds only hash-confirmed words, but a failed word is
    /// still not swept again this run.
    tried_before: BTreeSet<String>,
}

impl<'a> Vocabulary<'a> {
    /// Builds a proposer over a grammar and a hasher.
    ///
    /// Takes anything that can be asked, so the whole round is testable with no model, network or
    /// download.
    pub fn new(asker: &'a dyn Ask, grammar: Grammar, hasher: NidHasher) -> Self {
        Self {
            asker,
            grammar,
            hasher,
            slot: DEFAULT_SLOT.to_owned(),
            threads: std::thread::available_parallelism().map_or(1, std::num::NonZero::get),
            budget: DEFAULT_BUDGET,
            temperature: DEFAULT_TEMPERATURE,
            seed: orbistoun_llm::engine::DEFAULT_SEED,
            bank: None,
            tried_before: BTreeSet::new(),
        }
    }

    /// Keeps what works, across rounds and across runs.
    ///
    /// Proposing vocabulary pays only if it compounds, so a bank keeps confirmed words between runs.
    /// Attaching one merges its words into the grammar immediately, so the next round asks a harder
    /// question.
    #[must_use]
    pub fn with_bank(mut self, bank: Bank) -> Self {
        self.absorb(bank.words().iter().cloned().collect());
        self.bank = Some(bank);
        self
    }

    /// Puts words into the working grammar.
    fn absorb(&mut self, words: Vec<String>) {
        if words.is_empty() {
            return;
        }
        self.grammar
            .vocabulary
            .entry(self.slot.clone())
            .or_default()
            .extend(words);
    }

    /// What has been kept, if anything is keeping it.
    #[must_use]
    pub fn bank(&self) -> Option<&Bank> {
        self.bank.as_ref()
    }

    /// The working grammar, including everything banked.
    ///
    /// A caller computing a control uses this, so a round is not credited for names earlier rounds
    /// made reachable.
    #[must_use]
    pub fn grammar(&self) -> &Grammar {
        &self.grammar
    }

    /// Samples more or less freely.
    #[must_use]
    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = temperature;
        self
    }

    /// Takes a different sample, the way a caller asks for different words; one seed gives
    /// identical words every round.
    #[must_use]
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }

    /// Puts new words in a different vocabulary list.
    #[must_use]
    pub fn with_slot(mut self, slot: impl Into<String>) -> Self {
        self.slot = slot.into();
        self
    }

    /// Sweeps with this many threads.
    #[must_use]
    pub fn with_threads(mut self, threads: usize) -> Self {
        self.threads = threads.max(1);
        self
    }

    /// Accepts at most this many new words per round.
    #[must_use]
    pub fn with_budget(mut self, budget: usize) -> Self {
        self.budget = budget;
        self
    }

    /// The words the vocabulary holds, without the ones merely tried.
    ///
    /// `sanitise` unions the two itself; keeping them apart lets it say why a word was refused.
    fn in_vocabulary(&self) -> BTreeSet<String> {
        self.grammar
            .vocabulary
            .values()
            .flatten()
            .map(|w| w.to_lowercase())
            .collect()
    }

    /// Asks for words, sweeps with them, keeps what the hash confirmed, and moves on.
    ///
    /// Takes `&mut self`: the seed advances, so two calls ask two questions, and confirmed words go
    /// into the bank and the working grammar for the next round.
    ///
    /// # Errors
    ///
    /// If nothing answered, the reply held no usable words, the grown grammar cannot be resolved,
    /// or the bank cannot be written; a bank that cannot be saved would lose the result it exists to
    /// keep.
    pub fn round(&mut self, targets: &Targets, context: &Context) -> Result<Round, Error> {
        let asked = prompt(context, &self.grammar, &self.slot);
        // Advanced before the ask, so a failed round still moves on rather than retrying an identical
        // question.
        self.seed = self.seed.wrapping_add(1);
        let reply = self.asker.ask(
            &Request::new(asked.clone())
                .with_system(SYSTEM)
                // Forty short words as JSON are about two hundred tokens; a larger ceiling only pays for a
                // preamble.
                .with_max_tokens(320)
                .with_temperature(self.temperature)
                .with_seed(self.seed),
        )?;

        let (offered, parsed_as) =
            read_words(&reply.text).ok_or_else(|| Error::Reply(shape_of(&reply.text)))?;
        let (tried, rejected) = sanitise(
            &offered,
            &self.in_vocabulary(),
            &self.tried_before,
            self.budget,
        );
        // Remembered whatever happens next: a failed word never reaches the bank, but is not swept again
        // this run.
        self.tried_before
            .extend(tried.iter().map(|word| word.to_lowercase()));

        if tried.is_empty() {
            return Ok(Round {
                asked,
                backend: reply.backend,
                model: reply.model,
                parsed_as,
                offered: offered.len(),
                tried,
                rejected,
                solved: Vec::new(),
                kept: Vec::new(),
                stats: SearchStats {
                    tried: 0,
                    wanted: targets.len(),
                    found: 0,
                },
                swept_ms: 0,
                banked: 0,
            });
        }

        // Grown in memory; nothing on disk changes, so a round that finds nothing leaves no trace.
        let mut grown = self.grammar.clone();
        grown
            .vocabulary
            .entry(self.slot.clone())
            .or_default()
            .extend(tried.iter().cloned());
        // A round sweeps the delta, not the space: only the shapes that use the grown slot, since every
        // other shape generates what the caller's ordinary sweep already covered.
        grown
            .pattern
            .retain(|spec| spec.parts.iter().any(|part| part == &self.slot));
        if grown.pattern.is_empty() {
            // A word added to a slot no shape uses generates nothing, every round, indistinguishable from an
            // exhausted vocabulary.
            return Err(Error::SlotUnused(self.slot.clone()));
        }

        // The slot itself is not narrowed to the new words. A `Generated` record is a pattern and an
        // index into a mixed-radix number whose digits are that pattern's list lengths; shortening the
        // slot would make the index name a different candidate in the real grammar. Dropping whole
        // patterns is safe, because indices are per pattern (D214).
        let patterns = grown.patterns()?;

        let started = Instant::now();
        let (solved, stats) = solve_patterns(&self.hasher, targets, &patterns, self.threads);
        let swept_ms = started.elapsed().as_millis();

        let kept: Vec<String> = tried
            .iter()
            .filter(|word| solved.iter().any(|s| s.name.contains(word.as_str())))
            .cloned()
            .collect();

        // Banked here rather than handed back, so no caller can forget the only thing a round produces.
        let banked = match &mut self.bank {
            Some(bank) => {
                let fresh = bank.add(kept.clone());
                if fresh > 0 {
                    bank.save()?;
                }
                fresh
            }
            None => 0,
        };
        if banked > 0 {
            self.absorb(kept.clone());
        }

        Ok(Round {
            asked,
            backend: reply.backend,
            model: reply.model,
            parsed_as,
            offered: offered.len(),
            tried,
            rejected,
            solved,
            kept,
            stats,
            swept_ms,
            banked,
        })
    }
}

/// Splits offered words into those worth sweeping and those not.
///
/// A free function, testable without a model. Shape is checked before novelty, so a mangled
/// repeat is reported as mangled.
pub fn sanitise(
    offered: &[String],
    in_vocabulary: &BTreeSet<String>,
    tried_before: &BTreeSet<String>,
    budget: usize,
) -> (Vec<String>, Vec<Rejected>) {
    // Padding is measured against both sets: `Group2` is padding whether `Group` is in the
    // vocabulary or failed last round.
    let known: BTreeSet<String> = in_vocabulary.union(tried_before).cloned().collect();
    let known = &known;
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut tried = Vec::new();
    let mut rejected = Vec::new();

    for word in offered {
        let word = word.trim();
        let because = if !is_word(word) {
            Some(Refusal::NotAWord)
        } else if !WORD_LENGTH.contains(&word.chars().count()) {
            Some(Refusal::WrongLength)
        } else if !seen.insert(word.to_lowercase()) {
            Some(Refusal::Duplicate)
        } else if in_vocabulary.contains(&word.to_lowercase()) {
            Some(Refusal::AlreadyKnown)
        } else if tried_before.contains(&word.to_lowercase()) {
            Some(Refusal::AlreadyTried)
        } else if is_padded_repeat(word, known) {
            Some(Refusal::PaddedRepeat)
        } else if tried.len() >= budget {
            Some(Refusal::OverBudget)
        } else {
            None
        };
        match because {
            Some(because) => rejected.push(Rejected {
                word: word.to_owned(),
                because,
            }),
            None => tried.push(word.to_owned()),
        }
    }
    (tried, rejected)
}

/// Standing instructions.
///
/// States the refusal twice, as what to do and what not to, because the failure that matters is
/// a model answering with a whole identifier.
pub const SYSTEM: &str = concat!(
    "You extend the word list that a brute-force search uses to build candidate ",
    "identifiers. The identifiers are built by joining short words together, and you ",
    "are shown how. Your job is to suggest further WORDS of the same kind. ",
    "A word is ONE short concept - Sema, Attr, Prio, Direct, Equeue - capitalised, ",
    "letters and digits only. ",
    "It is NOT a compound: `Schedparam` and `Schedpolicy` are two words each and are ",
    "wrong; `Sched`, `Param` and `Policy` are right. ",
    "It is NOT a whole identifier. ",
    "Vary your suggestions - cover different areas rather than many spellings of one. ",
    "Never repeat a word already listed. ",
    "Reply with only a JSON array of strings and no other text."
);

/// Builds the question.
///
/// Carries no hash, target or mapping: the model continues a naming convention (see the module
/// documentation), and a test holds the prompt to it.
pub fn prompt(context: &Context, grammar: &Grammar, slot: &str) -> String {
    let mut out = String::new();

    if !context.libraries.is_empty() {
        out.push_str("These identifiers belong to libraries named: ");
        out.push_str(&join_capped(&context.libraries, 24));
        out.push_str(".\n\n");
    }

    if !context.examples.is_empty() {
        // Shown split, not whole: whole identifiers as examples teach the model to answer with
        // identifier-shaped compounds, while the seams teach the word shape.
        out.push_str("Identifiers are built by joining short words:\n");
        let words = every_word(grammar);
        for example in context.examples.iter().take(12) {
            out.push_str("  ");
            out.push_str(example);
            if let Some(parts) = decompose(example, &words) {
                out.push_str("  =  ");
                out.push_str(&parts.join(" + "));
            }
            out.push('\n');
        }
        out.push('\n');
    }

    if let Some(role) = &context.role {
        out.push_str("You are extending one position in that join: ");
        out.push_str(role);
        out.push_str("\n\n");
    }

    if let Some(existing) = grammar.vocabulary.get(slot) {
        // The empty string is how a shape says "no suffix", not an example of a word.
        let existing: Vec<String> = existing.iter().filter(|w| !w.is_empty()).cloned().collect();
        if !existing.is_empty() {
            // A sample, not the list: the whole list with "do not repeat these" comes back with digits
            // appended (`Cpu2` through `Cpu30`). The sanitiser catches genuine repeats.
            out.push_str("Words of that kind already exist, for example:\n  ");
            out.push_str(&join_capped(&existing, 25));
            let _ = write!(
                out,
                concat!(
                    "\n\nThere are {} in total. Suggest ones that are NOT variations ",
                    "of those - a different idea, not the same word with a number or a ",
                    "prefix added.\n\n"
                ),
                existing.len()
            );
        }
    }

    // Many proposed words already sit inside standard-library names (`Unset` in `unsetenv`), which
    // decomposing that list supplies for free; the vendor's own domain vocabulary is what is worth
    // asking for.
    out.push_str(concat!(
        "The vocabulary of the C and POSIX standard libraries is already covered, so ",
        "words like Alloc, Read, Write, Lock, Env or Time add nothing. What is missing ",
        "is this platform's own domain vocabulary - the nouns and markers a console's ",
        "audio, graphics, networking, storage and system libraries use and a portable ",
        "standard never would.\n\n"
    ));

    if let Some(theme) = &context.theme {
        out.push_str("What is known about the ones still unnamed: ");
        out.push_str(theme);
        out.push_str("\n\n");
    }

    // `write!` rather than `push_str(&format!(..))`: no intermediate allocation, and writing into a
    // `String` cannot fail.
    let _ = write!(
        out,
        "Suggest {} new words, as a JSON array of strings.",
        context.want.max(1)
    );
    out
}

/// Every word the grammar knows, longest first, because [`decompose`] is greedy: with `Mem`
/// first, `Memory` would split as `Mem` + `ory`.
fn every_word(grammar: &Grammar) -> Vec<String> {
    let mut words: Vec<String> = grammar
        .vocabulary
        .values()
        .flatten()
        .filter(|w| !w.is_empty())
        .cloned()
        .collect();
    words.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    words.dedup();
    words
}

/// Splits an identifier into the words the grammar would have joined.
///
/// Greedy longest-match, and `None` as soon as a piece is not a known word: a partial split would
/// teach a seam that is not there. Used only to illustrate a prompt.
pub fn decompose(name: &str, words: &[String]) -> Option<Vec<String>> {
    let mut parts = Vec::new();
    let mut rest = name;
    while !rest.is_empty() {
        let word = words.iter().find(|w| rest.starts_with(w.as_str()))?;
        parts.push(word.clone());
        rest = &rest[word.len()..];
    }
    (parts.len() > 1).then_some(parts)
}

fn join_capped(items: &[String], cap: usize) -> String {
    let shown: Vec<&str> = items.iter().take(cap).map(String::as_str).collect();
    let mut joined = shown.join(", ");
    if items.len() > cap {
        let _ = write!(joined, ", and {} more", items.len() - cap);
    }
    joined
}

/// Whether this is a known word with digits stuck on the end.
///
/// A model out of ideas repeats the list it was shown with numbers appended. The grammar
/// composes, so `Ex2` is already reachable from `Ex`. Digits alone are allowed (`Api2`,
/// `Attribute2` are real entries); a known stem wearing them is refused.
fn is_padded_repeat(word: &str, known: &BTreeSet<String>) -> bool {
    let stem = word.trim_end_matches(|c: char| c.is_ascii_digit());
    stem.len() != word.len() && known.contains(&stem.to_lowercase())
}

/// A word in this grammar: capitalised, then alphanumeric, as the shipped vocabulary is
/// (`Abort`, `Api2`); the grammar concatenates words, so a separator would generate
/// non-identifiers.
pub fn is_word(candidate: &str) -> bool {
    let mut chars = candidate.chars();
    match chars.next() {
        Some(first) if first.is_ascii_uppercase() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric())
}

/// Reads words out of whatever the model said.
///
/// Three routes in order, and which one worked is reported. The sanitiser is the real gate, so a
/// loose read is safe: scraped prose is refused there, word by word.
pub fn read_words(text: &str) -> Option<(Vec<String>, Parsed)> {
    if let (Some(open), Some(close)) = (text.find('['), text.rfind(']')) {
        if open < close {
            if let Ok(words) = serde_json::from_str::<Vec<String>>(&text[open..=close]) {
                if !words.is_empty() {
                    return Some((words, Parsed::JsonArray));
                }
            }
        }
    }

    let quoted: Vec<String> = text
        .split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect();
    if !quoted.is_empty() {
        return Some((quoted, Parsed::QuotedStrings));
    }

    let bare: Vec<String> = text
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_owned)
        .collect();
    (!bare.is_empty()).then_some((bare, Parsed::BareTokens))
}

/// Describes an unusable reply without quoting it.
fn shape_of(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return "the model replied with nothing at all".to_owned();
    }
    format!(
        "{} characters holding no array, no quoted string and no word",
        trimmed.chars().count()
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{
        Context, DEFAULT_SLOT, Parsed, WORD_LENGTH, is_word, prompt, read_words, sanitise,
    };
    use crate::Refusal;
    use orbistoun_names::{DEFAULT_VENDOR_GRAMMAR, Grammar};

    fn known(words: &[&str]) -> BTreeSet<String> {
        words.iter().map(|w| w.to_lowercase()).collect()
    }

    fn offered(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| (*w).to_owned()).collect()
    }

    fn grammar() -> Grammar {
        Grammar::builtin().expect("the shipped grammar parses")
    }

    fn context() -> Context {
        Context {
            libraries: vec!["libkernel".to_owned()],
            examples: vec!["sceKernelAllocateDirectMemory".to_owned()],
            theme: Some("called once during startup".to_owned()),
            role: None,
            want: 20,
        }
    }

    /// The prompt carries no hash, so a confirmed name is a derivation `audit` can re-derive, never
    /// a recall.
    #[test]
    fn the_prompt_carries_no_hash() {
        let asked = prompt(&context(), &grammar(), DEFAULT_SLOT);
        let lowered = asked.to_lowercase();
        assert!(!lowered.contains("0x"), "{asked}");
        assert!(!lowered.contains("nid"), "{asked}");
        assert!(!lowered.contains("hash"), "{asked}");
        // Sixteen consecutive hex digits is what a hash looks like written out.
        let hex_runs = lowered
            .split(|c: char| !c.is_ascii_hexdigit())
            .filter(|run| run.len() >= 16)
            .count();
        assert_eq!(hex_runs, 0, "{asked}");
    }

    /// The prompt asks for words and says not to answer with an identifier.
    #[test]
    fn the_prompt_asks_for_parts_not_names() {
        assert!(super::SYSTEM.contains("NOT a whole identifier"));
        assert!(prompt(&context(), &grammar(), DEFAULT_SLOT).contains("new words"));
    }

    /// A sample of what is known is shown, with an instruction not to vary it.
    #[test]
    fn the_prompt_shows_a_sample_and_asks_for_something_different() {
        let asked = prompt(&context(), &grammar(), DEFAULT_SLOT);
        assert!(asked.contains("already exist, for example"), "{asked}");
        assert!(asked.contains("NOT variations"), "{asked}");
    }

    /// A long list is capped rather than sent whole.
    #[test]
    fn a_long_list_is_capped() {
        let asked = prompt(&context(), &grammar(), DEFAULT_SLOT);
        assert!(asked.contains("more"), "{asked}");
        assert!(asked.len() < 8_000, "prompt is {} bytes", asked.len());
    }

    /// A word is capitalised and alphanumeric; anything else is not one.
    #[test]
    fn a_word_is_capitalised_and_alphanumeric() {
        for good in ["Sema", "Attr", "Api2", "Attribute2", "A1"] {
            assert!(is_word(good), "{good}");
        }
        for bad in ["sema", "_Attr", "Attr_", "Attr-2", "Attr 2", "", "2Attr"] {
            assert!(!is_word(bad), "{bad}");
        }
    }

    /// A clean JSON array is read as one.
    #[test]
    fn a_json_array_is_read_strictly() {
        let (words, how) = read_words(r#"["Sema", "Attr"]"#).expect("read");
        assert_eq!(words, vec!["Sema", "Attr"]);
        assert_eq!(how, Parsed::JsonArray);
    }

    /// An array wrapped in prose is still read strictly.
    #[test]
    fn an_array_inside_prose_is_still_read_strictly() {
        let (words, how) =
            read_words("Sure! Here you go:\n[\"Sema\", \"Attr\"]\nHope that helps.").expect("read");
        assert_eq!(words, vec!["Sema", "Attr"]);
        assert_eq!(how, Parsed::JsonArray);
    }

    /// A reply that is not an array falls back, and says that it did.
    #[test]
    fn a_loose_reply_falls_back_and_says_so() {
        let (words, how) = read_words("I suggest \"Sema\" and \"Attr\".").expect("read");
        assert_eq!(words, vec!["Sema", "Attr"]);
        assert_eq!(how, Parsed::QuotedStrings);

        let (words, how) = read_words("Sema Attr Prio").expect("read");
        assert_eq!(words, vec!["Sema", "Attr", "Prio"]);
        assert_eq!(how, Parsed::BareTokens);
    }

    /// An empty reply is a failure, not an empty word list.
    #[test]
    fn an_empty_reply_is_a_failure() {
        assert!(read_words("").is_none());
        assert!(read_words("   \n  ").is_none());
    }

    /// An empty JSON array does not count as a strict read.
    #[test]
    fn an_empty_array_is_not_a_strict_read() {
        assert!(read_words("[]").is_none());
    }

    /// A word the grammar already has is refused.
    #[test]
    fn a_known_word_is_refused() {
        let (tried, rejected) = sanitise(
            &offered(&["Sema", "Attr"]),
            &known(&["attr"]),
            &BTreeSet::new(),
            10,
        );
        assert_eq!(tried, vec!["Sema"]);
        assert_eq!(rejected[0].word, "Attr");
        assert_eq!(rejected[0].because, Refusal::AlreadyKnown);
    }

    /// Case does not pass a repeat through the check.
    #[test]
    fn case_does_not_smuggle_a_repeat_through() {
        let (tried, rejected) =
            sanitise(&offered(&["Attr"]), &known(&["ATTR"]), &BTreeSet::new(), 10);
        assert!(tried.is_empty());
        assert_eq!(rejected[0].because, Refusal::AlreadyKnown);
    }

    /// The same word twice in one reply is counted once.
    #[test]
    fn a_repeat_within_one_reply_is_refused() {
        let (tried, rejected) = sanitise(
            &offered(&["Sema", "sema", "Sema"]),
            &known(&[]),
            &BTreeSet::new(),
            10,
        );
        assert_eq!(tried, vec!["Sema"]);
        assert_eq!(rejected.len(), 2);
        // `sema` is refused for its shape, a different fact from being a repeat.
        assert_eq!(rejected[0].because, Refusal::NotAWord);
        assert_eq!(rejected[1].because, Refusal::Duplicate);
    }

    /// A whole identifier is refused: swept as one entry it could collide and be recorded
    /// `generated`, a recall wearing a derivation's record.
    #[test]
    fn a_whole_identifier_is_refused() {
        let (tried, rejected) = sanitise(
            &offered(&[
                "sceKernelAllocateDirectMemory",
                "SceKernelAllocateDirectMemory",
            ]),
            &known(&[]),
            &BTreeSet::new(),
            10,
        );
        assert!(tried.is_empty(), "{tried:?}");
        assert_eq!(rejected[0].because, Refusal::NotAWord, "lowercase initial");
        assert_eq!(
            rejected[1].because,
            Refusal::WrongLength,
            "capitalised, but far too long to be one part"
        );
    }

    /// A known word with digits appended is refused as padding.
    #[test]
    fn a_known_word_with_digits_appended_is_refused() {
        let (tried, rejected) = sanitise(
            &offered(&["Cpu2", "Cpu30", "Ex2"]),
            &known(&["cpu", "ex"]),
            &BTreeSet::new(),
            10,
        );
        assert!(tried.is_empty(), "{tried:?}");
        assert!(
            rejected.iter().all(|r| r.because == Refusal::PaddedRepeat),
            "{rejected:?}"
        );
    }

    /// Digits are refused only on a word already known.
    #[test]
    fn a_digit_is_only_a_problem_on_a_word_already_known() {
        let (tried, rejected) = sanitise(
            &offered(&["Api2", "Sema3"]),
            &known(&["cpu"]),
            &BTreeSet::new(),
            10,
        );
        assert_eq!(tried, vec!["Api2", "Sema3"], "{rejected:?}");
    }

    /// The budget caps a round, and what it cut is reported.
    #[test]
    fn the_budget_caps_a_round_and_says_what_it_cut() {
        let (tried, rejected) = sanitise(
            &offered(&["Aa", "Bb", "Cc", "Dd"]),
            &known(&[]),
            &BTreeSet::new(),
            2,
        );
        assert_eq!(tried, vec!["Aa", "Bb"]);
        assert_eq!(rejected.len(), 2);
        assert!(rejected.iter().all(|r| r.because == Refusal::OverBudget));
    }

    /// Nothing usable yields no words and a reason for each.
    #[test]
    fn an_unusable_reply_yields_reasons_not_silence() {
        let (tried, rejected) = sanitise(
            &offered(&["here", "are", "some", "!!"]),
            &known(&[]),
            &BTreeSet::new(),
            10,
        );
        assert!(tried.is_empty());
        assert_eq!(rejected.len(), 4);
    }

    // The whole chain, with only the model faked.

    /// A model that says exactly what it is told to, so everything downstream of a reply can be
    /// pinned without a real model.
    #[derive(Debug)]
    struct Canned(&'static str);

    impl orbistoun_llm::Ask for Canned {
        fn ask(
            &self,
            _request: &orbistoun_llm::Request,
        ) -> Result<orbistoun_llm::Reply, orbistoun_llm::Error> {
            Ok(orbistoun_llm::Reply {
                text: self.0.to_owned(),
                backend: "canned".to_owned(),
                model: "canned".to_owned(),
                attempts: Vec::new(),
            })
        }
    }

    fn hasher() -> orbistoun_nid::NidHasher {
        orbistoun_nid::NidHasher::new(orbistoun_nid::default_suffix())
    }

    /// The prompt says the standard vocabulary is already covered.
    #[test]
    fn the_prompt_says_the_standard_vocabulary_is_already_covered() {
        let text = prompt(&context(), &tiny(), DEFAULT_SLOT);
        assert!(
            text.contains("already covered"),
            "the prompt no longer says what not to ask for:\n{text}"
        );
        assert!(
            text.contains("domain vocabulary"),
            "the prompt no longer says what to ask for instead:\n{text}"
        );
    }

    /// The refusal filter sees words injected at parse time (`posix`), not only those written in
    /// the grammar file.
    #[test]
    fn the_refusal_filter_sees_words_that_are_injected_rather_than_written() {
        let grammar = Grammar::builtin().expect("the shipped grammar parses");
        let injected = grammar
            .vocabulary
            .get("posix")
            .expect("posix is injected at parse time");
        assert!(!injected.is_empty(), "posix injected but empty");
        assert!(
            !DEFAULT_VENDOR_GRAMMAR.contains(
                "
posix = ["
            ),
            "posix is written to the file now, so this test is measuring nothing"
        );

        // One that survives the shape rules, so the refusal under test is novelty, not `NotAWord`.
        let word = injected
            .iter()
            .find(|w| is_word(w) && WORD_LENGTH.contains(&w.chars().count()))
            .expect("some injected word is word-shaped")
            .clone();
        let proposer = super::Vocabulary::new(&Canned("[]"), grammar, hasher());
        let (tried, rejected) = sanitise(
            std::slice::from_ref(&word),
            &proposer.in_vocabulary(),
            &BTreeSet::new(),
            10,
        );
        assert!(tried.is_empty(), "{word} was swept despite being known");
        assert_eq!(
            rejected.first().map(|r| r.because),
            Some(Refusal::AlreadyKnown)
        );
    }

    /// A grammar small enough that a round sweeps it instantly.
    fn tiny() -> Grammar {
        Grammar::parse(concat!(
            "[vocabulary]\n",
            "prefix = [\"sce\"]\n",
            "learned = [\"Sema\"]\n",
            "\n",
            "[[pattern]]\n",
            "name = \"tiny\"\n",
            "parts = [\"prefix\", \"learned\"]\n"
        ))
        .expect("the tiny grammar parses")
    }

    /// A word tried once is not swept again in the same run.
    #[test]
    fn a_word_that_failed_is_not_swept_again() {
        let targets = orbistoun_names::solve::Targets::new([hasher().hash("sceNothingMatches")]);
        let mut proposer = super::Vocabulary::new(&Canned(r#"["Zzqwx"]"#), tiny(), hasher());

        let first = proposer.round(&targets, &context()).expect("a round runs");
        assert_eq!(first.tried, vec!["Zzqwx"], "the first round did not try it");
        assert!(
            first.solved.is_empty(),
            "the canned word should find nothing"
        );

        let second = proposer.round(&targets, &context()).expect("a round runs");
        assert!(
            second.tried.is_empty(),
            "the same failed word was swept a second time: {:?}",
            second.tried
        );
        assert_eq!(
            second.rejected.first().map(|r| r.because),
            Some(Refusal::AlreadyTried),
            "it was refused, but not for the reason that would tell a reader why"
        );
    }

    /// A word that failed is remembered for the run and never banked.
    #[test]
    fn a_word_that_failed_is_remembered_but_never_banked() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("words.txt");
        let targets = orbistoun_names::solve::Targets::new([hasher().hash("sceNothingMatches")]);
        let mut proposer = super::Vocabulary::new(&Canned(r#"["Zzqwx"]"#), tiny(), hasher())
            .with_bank(crate::bank::Bank::open(&path).expect("opens"));

        let round = proposer.round(&targets, &context()).expect("a round runs");
        assert_eq!(round.banked, 0, "a word that found nothing was banked");
        let written = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(
            !written.contains("Zzqwx"),
            "a failure reached the bank, which is supposed to be evidence:\n{written}"
        );
    }

    /// The shipped grammar, minus one word, minus the shapes that use `learned` twice.
    ///
    /// Dropping patterns cannot move an index inside the remaining ones, while dropping a word would
    /// (D214), and every record is checked against the complete shape set by [`adopting`]. The
    /// quadratic shapes dominate a round's cost and are too slow for a unit test. Nothing else is
    /// faked.
    fn without(word: &str) -> Grammar {
        let mut grammar = Grammar::builtin().expect("the shipped grammar parses");
        grammar
            .vocabulary
            .get_mut(DEFAULT_SLOT)
            .expect("the slot exists")
            .retain(|w| w != word);
        grammar
            .pattern
            .retain(|spec| spec.parts.iter().filter(|p| *p == DEFAULT_SLOT).count() < 2);
        grammar
    }

    /// The grammar a record has to survive: the word adopted, and every shape back.
    ///
    /// Appended rather than sorted into place, as a round grows its slot, since the index means
    /// something only against that ordering.
    fn adopting(word: &str) -> Grammar {
        let mut grammar = without(word);
        grammar
            .vocabulary
            .get_mut(DEFAULT_SLOT)
            .expect("the slot exists")
            .push(word.to_owned());
        grammar.pattern = Grammar::builtin()
            .expect("the shipped grammar parses")
            .pattern;
        grammar
    }

    /// A proposed word recovers a real name, and the audit accepts the record.
    ///
    /// Real grammar, hasher, sweep and hash; only the model is faked. The wrong word finds nothing
    /// and the right one recovers the name, and the record passes `solve::verify`, the function
    /// `orbistoun-cli audit` runs.
    #[test]
    fn a_proposed_word_recovers_a_real_name_and_the_audit_accepts_it() {
        const NAME: &str = "sceKernelCreateSema";
        const WORD: &str = "Sema";

        let target = hasher().hash(NAME);
        let targets = orbistoun_names::solve::Targets::new([target]);
        let base = without(WORD);

        // A word that is not the answer changes nothing.
        let miss = super::Vocabulary::new(&Canned(r#"["Zzqwx"]"#), base.clone(), hasher())
            .round(&targets, &context())
            .expect("a round runs");
        assert_eq!(miss.tried, vec!["Zzqwx"]);
        assert!(miss.solved.is_empty(), "{:?}", miss.solved);
        assert!(miss.is_empty());

        // The right one recovers the name.
        let hit = super::Vocabulary::new(&Canned(r#"["Sema"]"#), base.clone(), hasher())
            .round(&targets, &context())
            .expect("a round runs");
        assert_eq!(hit.solved.len(), 1, "{:?}", hit.solved);
        assert_eq!(hit.solved[0].name, NAME);
        assert_eq!(hit.solved[0].nid.as_raw(), target.as_raw());
        assert_eq!(hit.kept, vec![WORD]);

        // The record is re-derived against the grammar with the word adopted and every shape back,
        // including the two the sweep skipped: per-pattern indices make that hold.
        let patterns = adopting(WORD).patterns().expect("patterns resolve");
        assert!(
            orbistoun_names::solve::verify(
                NAME,
                &hit.solved[0].derivation,
                &patterns,
                &[],
                // A generated record, so no affix rule can be consulted.
                &orbistoun_names::affix::Affixes::default(),
            ),
            "the audit would refuse {:?}",
            hit.solved[0].derivation
        );
    }

    /// A real model, asked for real words, swept for real. Opt-in, because it downloads a model and
    /// runs inference:
    ///
    /// ```text
    /// cargo test -p orbistoun-propose -- --ignored --nocapture a_real_model
    /// ```
    ///
    /// Whether the words look like vendor vocabulary is printed for a person to judge. What is
    /// asserted is the mechanism: something answered, the reply was read, and every swept word
    /// passed the guards, with no whole identifier.
    #[test]
    #[ignore = "downloads a model and runs inference; opt-in via --ignored"]
    fn a_real_model_proposes_words_shaped_like_the_vocabulary() {
        let dir = tempfile::tempdir().expect("temp dir");
        let llm = orbistoun_llm::Llm::open(dir.path()).expect("the service opens");
        assert!(
            llm.is_available(),
            "nothing is configured on this machine: {}",
            llm.host().summary()
        );

        let base = without("Sema");
        let round = super::Vocabulary::new(&llm, base, hasher())
            .with_budget(20)
            .round(
                &orbistoun_names::solve::Targets::new([hasher().hash("sceKernelCreateSema")]),
                &Context {
                    libraries: vec!["libkernel".to_owned(), "libSceNet".to_owned()],
                    examples: vec![
                        "sceKernelAllocateDirectMemory".to_owned(),
                        "sceKernelGetDirectMemorySize".to_owned(),
                        "sceKernelCreateEqueue".to_owned(),
                    ],
                    theme: Some("synchronisation primitives created during startup".to_owned()),
                    role: None,
                    want: 20,
                },
            )
            .expect("a round runs against a real model");

        eprintln!(
            "REAL-ROUND backend={} model={} parsed_as={:?} offered={} tried={} swept={} in {}ms",
            round.backend,
            round.model,
            round.parsed_as,
            round.offered,
            round.tried.len(),
            round.stats.tried,
            round.swept_ms
        );
        eprintln!("  accepted: {:?}", round.tried);
        for rejected in &round.rejected {
            eprintln!(
                "  refused:  {:?} - {}",
                rejected.word,
                rejected.because.describe()
            );
        }
        eprintln!(
            "  names:    {:?}",
            round.solved.iter().map(|s| &s.name).collect::<Vec<_>>()
        );

        assert!(round.offered > 0, "the model offered nothing at all");
        for word in &round.tried {
            assert!(is_word(word), "{word:?} reached the sweep unshaped");
            assert!(
                WORD_LENGTH.contains(&word.chars().count()),
                "{word:?} reached the sweep at the wrong length - a whole name got through"
            );
        }
    }

    /// A word that worked is kept and stops being new: round one earns the name and banks the word,
    /// and round two, given the same reply, earns nothing.
    #[test]
    fn a_word_that_worked_is_kept_and_stops_being_new() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("words.txt");
        let targets = orbistoun_names::solve::Targets::new([hasher().hash("sceKernelCreateSema")]);

        let mut proposer =
            super::Vocabulary::new(&Canned(r#"["Sema"]"#), without("Sema"), hasher())
                .with_bank(crate::bank::Bank::open(&path).expect("opens"));

        let first = proposer.round(&targets, &context()).expect("a round runs");
        assert_eq!(first.solved.len(), 1, "{:?}", first.solved);
        assert_eq!(first.kept, vec!["Sema"]);
        assert_eq!(first.banked, 1, "the word that worked was not kept");

        let second = proposer.round(&targets, &context()).expect("a round runs");
        assert!(second.tried.is_empty(), "{:?}", second.tried);
        assert_eq!(second.banked, 0, "a word already held was counted as new");
        assert_eq!(
            second.rejected.first().map(|r| r.because),
            Some(Refusal::AlreadyKnown),
            "the banked word did not reach the grammar"
        );
    }

    /// What was kept survives into the next run through the file on disk.
    #[test]
    fn what_was_kept_survives_into_the_next_run() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("words.txt");
        let targets = orbistoun_names::solve::Targets::new([hasher().hash("sceKernelCreateSema")]);

        {
            let mut first =
                super::Vocabulary::new(&Canned(r#"["Sema"]"#), without("Sema"), hasher())
                    .with_bank(crate::bank::Bank::open(&path).expect("opens"));
            first.round(&targets, &context()).expect("a round runs");
        }

        // A fresh proposer, a fresh grammar, the same bank on disk.
        let later = super::Vocabulary::new(&Canned("[]"), without("Sema"), hasher())
            .with_bank(crate::bank::Bank::open(&path).expect("reopens"));
        assert!(later.bank().expect("a bank").words().contains("Sema"));
        assert!(
            later
                .grammar()
                .vocabulary
                .get(DEFAULT_SLOT)
                .expect("the slot")
                .iter()
                .any(|w| w == "Sema"),
            "the bank was read but never reached the grammar"
        );
    }

    /// Successive rounds ask different questions, because `round` advances the seed.
    #[test]
    fn successive_rounds_are_different_questions() {
        let targets = orbistoun_names::solve::Targets::new([hasher().hash("nothing-matches")]);
        let mut proposer = super::Vocabulary::new(&Canned(r#"["Aaa"]"#), without("Sema"), hasher());

        let before = proposer.seed;
        proposer.round(&targets, &context()).expect("a round runs");
        let between = proposer.seed;
        proposer.round(&targets, &context()).expect("a round runs");

        assert_ne!(before, between);
        assert_ne!(between, proposer.seed);
    }

    /// A round sweeps only the shapes that use the grown slot.
    ///
    /// The bound is loose, so the test does not pin the shipped vocabulary's size.
    #[test]
    fn a_round_sweeps_only_the_shapes_that_use_the_new_words() {
        let targets = orbistoun_names::solve::Targets::new([hasher().hash("sceKernelCreateSema")]);
        let round = super::Vocabulary::new(&Canned(r#"["Sema"]"#), without("Sema"), hasher())
            .round(&targets, &context())
            .expect("a round runs");

        let whole: u64 = Grammar::builtin()
            .expect("grammar")
            .patterns()
            .expect("patterns")
            .iter()
            .map(orbistoun_names::Pattern::len)
            .sum();
        assert!(round.stats.tried > 0, "a round that swept nothing");
        assert!(
            round.stats.tried < whole / 10,
            "swept {} of {whole}",
            round.stats.tried
        );
    }

    /// The recorded index survives the word being adopted, so the slot is never narrowed to only
    /// the new words (D214).
    #[test]
    fn the_recorded_index_survives_the_word_being_adopted() {
        const NAME: &str = "sceKernelCreateSema";
        const WORD: &str = "Sema";

        let targets = orbistoun_names::solve::Targets::new([hasher().hash(NAME)]);
        let base = without(WORD);
        let round = super::Vocabulary::new(&Canned(r#"["Sema"]"#), base.clone(), hasher())
            .round(&targets, &context())
            .expect("a round runs");
        let solved = round.solved.first().expect("the name is found");

        // The grammar once the word is written into the vocabulary, which an audit runs against.
        let mut adopted = base;
        adopted
            .vocabulary
            .get_mut(DEFAULT_SLOT)
            .expect("the slot exists")
            .push(WORD.to_owned());

        assert!(
            orbistoun_names::solve::verify(
                NAME,
                &solved.derivation,
                &adopted.patterns().expect("patterns resolve"),
                &[],
                &orbistoun_names::affix::Affixes::default(),
            ),
            "the index stopped meaning what it meant: {:?}",
            solved.derivation
        );
    }

    /// Adding words to a slot no shape uses is refused, loudly.
    #[test]
    fn a_slot_no_pattern_uses_is_refused() {
        let targets = orbistoun_names::solve::Targets::new([hasher().hash("anything")]);
        let error = super::Vocabulary::new(&Canned(r#"["Sema"]"#), without("Sema"), hasher())
            .with_slot("nothing-uses-this")
            .round(&targets, &context())
            .expect_err("a slot nothing references is refused");
        assert!(matches!(error, crate::Error::SlotUnused(_)), "{error}");
    }

    /// A reply with no usable word sweeps nothing rather than re-sweeping everything.
    #[test]
    fn a_round_with_no_usable_word_sweeps_nothing() {
        let targets = orbistoun_names::solve::Targets::new([hasher().hash("sceKernelCreateSema")]);
        let round = super::Vocabulary::new(&Canned(r#"["nope", "!!"]"#), without("Sema"), hasher())
            .round(&targets, &context())
            .expect("a round runs");
        assert!(round.tried.is_empty());
        assert_eq!(round.stats.tried, 0);
        assert_eq!(round.rejected.len(), 2);
    }
}
