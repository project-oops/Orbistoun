//! Proposing **words**, never names.
//!
//! # The distinction the whole module turns on
//!
//! A model is asked for vocabulary - `Sema`, `Attr`, `Prio` - and never for an
//! identifier. The words go into the grammar, the grammar generates candidates, and a
//! candidate becomes a name only when its hash collides with one a real module asked
//! for.
//!
//! That is not a stylistic preference. It is what keeps
//! [PROVENANCE.md](../../../docs/PROVENANCE.md)'s central claim true. A name that
//! arrives through this path is recorded `generated` at a pattern and an index, so
//! `orbistoun-cli audit` re-derives it by evaluating that pattern - the same check every
//! other generated name gets, with nothing to take on trust. Had the model been asked
//! for the name directly, the record would say only "something suggested this", nothing
//! could re-derive it, and the repository's answer to *"did you work these out
//! yourselves?"* would have a hole in it exactly where its foundation is.
//!
//! The word route costs nothing to take and needs no new provenance category. So it is
//! the one taken, and [`prompt`] is written so the model is **structurally incapable**
//! of supplying an answer: it is never shown a hash, never told which function is
//! wanted, and never given a mapping. A test asserts the prompt carries no hash.
//!
//! # Why a model is allowed near this at all
//!
//! Because here it cannot do damage. The oracle is a hash collision - arithmetic, not
//! judgement - so a confidently invented word is discarded by the same mechanism that
//! discards a carefully reasoned one, and the only cost is a sweep. This is the rare
//! corner of the project where being wrong is free, which is precisely why it is the
//! first place to point a proposer.
//!
//! What the model actually buys is **fewer sweeps**: the candidate space is already
//! large enough to exhaust, so value comes from proposing the right words sooner, not
//! from proposing more of them.

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
/// `learned` is where the existing widening step already puts words split out of
/// confirmed names (D195), so a proposed word that works ends up indistinguishable from
/// one harvested that way - which is correct, because by then it *is* one.
pub const DEFAULT_SLOT: &str = "learned";

/// How freely to sample.
///
/// **Not zero, and the reason is worth stating because the rest of this workspace
/// defaults to zero.** Determinism is right where a result has to be attributable to a
/// change. A proposer is the opposite case: the oracle is a hash, so a proposal is
/// worth nothing until arithmetic agrees, and *what a round needs is variety*.
///
/// Greedy decoding on this task does not merely repeat between rounds - it repeats
/// *within* one. The first real round returned twenty suggestions of which fourteen
/// were the same word, and all six survivors shared a prefix. Sampling is what makes a
/// second round a second question (D219).
pub const DEFAULT_TEMPERATURE: f32 = 0.9;

/// Most new words one round may add.
///
/// A ceiling rather than a target. The sweep grows with the vocabulary, so an
/// unbounded round is a search that takes longer every time it fails - and a model
/// asked for "as many as you can" produces its worst suggestions at the end of the list.
pub const DEFAULT_BUDGET: usize = 40;

/// Shortest and longest a single word may be.
///
/// One character is not a word, and anything past this is a phrase - which would mean
/// the model answered with a name after being asked not to.
pub const WORD_LENGTH: std::ops::RangeInclusive<usize> = 2..=24;

/// How the reply was read.
///
/// Recorded because a model that cannot produce the asked-for shape is worth knowing
/// about. The strict path failing every round is a prompt problem or a
/// too-small-model problem, and the two look identical if the fallback is silent.
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
/// Deliberately not "which function do you want named". Everything here describes the
/// *convention*, so the model's job is to continue a naming style rather than to recall
/// an answer.
#[derive(Debug, Clone, Default)]
pub struct Context {
    /// Library names the modules declare, which carry the vendor's own vocabulary.
    pub libraries: Vec<String>,
    /// Names already confirmed, shown so the convention can be inferred from evidence
    /// rather than described in prose.
    pub examples: Vec<String>,
    /// Optional free text about what these imports appear to do - a call position, an
    /// argument shape, the subsystem they cluster in.
    ///
    /// The seam through which better targeting arrives later without a signature change.
    pub theme: Option<String>,
    /// What kind of word the slot being extended holds - an action, a thing acted on,
    /// a suffix that modifies the meaning.
    ///
    /// **The highest-leverage field here, on the evidence.** The first live run asked
    /// for vocabulary in general and got one usable word in six rounds - and the word
    /// that worked, `Async`, was a *suffix*, filling the shortest list in the grammar.
    /// A model told which position it is filling answers a far narrower question than
    /// one shown a pile of words and asked for more like them (D219).
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
    /// Reporting only. A word is *credited* by appearing in a confirmed name; the
    /// names themselves are what is authoritative, and a word could in principle be
    /// credited for appearing inside a longer one it did not contribute.
    pub kept: Vec<String>,
    /// What the sweep cost and found.
    pub stats: SearchStats,
    /// How long the sweep took.
    pub swept_ms: u128,
    /// How many kept words were ones the bank did not already hold.
    ///
    /// **The number that says whether anything was learned.** A round can confirm five
    /// names entirely from words it already had, and a total that only ever rises would
    /// present that as progress.
    pub banked: usize,
}

impl Round {
    /// True when the round produced nothing at all.
    ///
    /// The signal a caller loops on: rounds that keep coming back empty mean the
    /// vocabulary is exhausted for this target set, not that the tool is broken.
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
    /// **Session memory, deliberately not the bank.** The bank is a claim - the hash is
    /// the only thing that puts a word in it - and a word that failed has no claim to
    /// make. But a failure is still a fact worth holding for the rest of the run, because
    /// without it the same wrong word is proposed, accepted and swept every round.
    ///
    /// Measured over thirty-six rounds before this existed: yield was entirely in the
    /// first round of each position, and the later rounds re-proposed the same handful of
    /// words - `Group` twelve times - at about thirty-five million candidates each.
    tried_before: BTreeSet<String>,
}

impl<'a> Vocabulary<'a> {
    /// Builds a proposer over a grammar and a hasher.
    ///
    /// Takes anything that can be asked rather than the service itself, so the whole
    /// round - read the reply, refuse what is wrong with it, sweep, credit what
    /// survived - is testable with no model, no network and no download.
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
    /// **Without this a proposer forgets, and a proposer that forgets is worth almost
    /// nothing.** The entire argument for proposing vocabulary is that it compounds - a
    /// word learned once reaches every title after it. Four measured runs earned two,
    /// six, one and two names, and the first and fourth earned *the same two*, from the
    /// same word, because nothing kept it and every run began from the shipped
    /// vocabulary again.
    ///
    /// Attaching a bank merges what it holds into the grammar immediately, so the very
    /// next round is asked a harder question than the last one was.
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
    /// A caller computing a control has to use *this*, not the shipped grammar: a
    /// control that does not know about banked words credits a round for names its own
    /// earlier rounds made reachable.
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

    /// Takes a different sample - the way a caller asks for *different* words.
    ///
    /// Two rounds at one seed produce identical words, so a loop that does not vary
    /// this re-asks the same question forever and reports the same miss each time.
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

    /// Every word the grammar already holds, lowercased for comparison.
    /// The words the vocabulary holds, without the ones merely tried.
    ///
    /// Refusing a repeat needs both, and `sanitise` unions them itself; saying *why* a
    /// word was refused needs them apart, which is why only this one exists.
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
    /// **Takes `&mut self`, and the two reasons are the point of it.** The seed advances,
    /// so calling this twice asks two questions rather than the same one twice - without
    /// that, a loop re-asks its way to the same answer forever. And anything a confirmed
    /// name was built from goes into the bank and into the working grammar, so the next
    /// round starts from a better position than this one did.
    ///
    /// # Errors
    ///
    /// If nothing answered, if the reply held no usable words at all, if the grown
    /// grammar cannot be resolved, or if the bank cannot be written. A bank that cannot
    /// be saved **is** an error: silently continuing would drop exactly the result the
    /// bank exists to keep, which is the failure this whole mechanism was added for.
    pub fn round(&mut self, targets: &Targets, context: &Context) -> Result<Round, Error> {
        let asked = prompt(context, &self.grammar, &self.slot);
        // Advanced before the ask, so a round that fails still moves on: retrying an
        // identical question after an unusable reply gets an identical unusable reply.
        self.seed = self.seed.wrapping_add(1);
        let reply = self.asker.ask(
            &Request::new(asked.clone())
                .with_system(SYSTEM)
                // Forty short words as JSON is about two hundred tokens. Six hundred
                // was effectively the whole cost of a round - the sweep beside it takes
                // under a second in release - and bought only room for a preamble
                // nobody reads.
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
        // **Remembered whatever happens next.** A word that fails is not a claim and never
        // reaches the bank, but it is a fact for the rest of this run - without it the same
        // word is proposed, accepted and swept every round, at tens of millions of
        // candidates a time, and the later rounds explore nothing.
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

        // Grown in memory. Nothing on disk changes, and a round that finds nothing
        // leaves no trace - which is what makes a wrong proposal genuinely free.
        let mut grown = self.grammar.clone();
        grown
            .vocabulary
            .entry(self.slot.clone())
            .or_default()
            .extend(tried.iter().cloned());
        // **A round sweeps the delta, not the space.** Two narrowings, and both are
        // exact rather than approximate - together they turn 2.6 billion candidates into
        // about a hundred and fifty thousand.
        //
        // First, only the shapes that use the grown slot. Every other shape generates
        // exactly what it generated before, and the caller's ordinary sweep has already
        // covered it.
        grown
            .pattern
            .retain(|spec| spec.parts.iter().any(|part| part == &self.slot));
        if grown.pattern.is_empty() {
            // A word added to a slot no shape uses generates nothing at all. Silently,
            // and forever - every round would report a clean miss while hashing zero
            // candidates, which is indistinguishable from an exhausted vocabulary.
            return Err(Error::SlotUnused(self.slot.clone()));
        }

        // **And that is as far as the narrowing goes**, deliberately.
        //
        // The obvious next step is to restrict the slot itself to only the new words,
        // which would take a round from about thirty million candidates to a hundred and
        // fifty thousand. It was written, measured, and **reverted**, because it quietly
        // destroys the thing this module exists to protect.
        //
        // A `Generated` record is a pattern and an *index*, and an index is a position in
        // a mixed-radix number whose digits are the lengths of that pattern's word lists.
        // Shortening the slot changes the radix, so the index recorded would name a
        // different candidate in any grammar anybody actually holds - a provenance record
        // that verifies against nothing. Filtering *patterns* is safe precisely because
        // indices are per-pattern: dropping a shape cannot move a position inside the
        // ones that remain.
        //
        // Eighty-three times cheaper with the record intact beats seventeen hundred times
        // cheaper with the record meaningless. Principle 11 (D214).
        let patterns = grown.patterns()?;

        let started = Instant::now();
        let (solved, stats) = solve_patterns(&self.hasher, targets, &patterns, self.threads);
        let swept_ms = started.elapsed().as_millis();

        let kept: Vec<String> = tried
            .iter()
            .filter(|word| solved.iter().any(|s| s.name.contains(word.as_str())))
            .cloned()
            .collect();

        // **Kept here rather than handed back for somebody to remember.** A caller that
        // forgets loses the only thing a round produces, and the round has no way to
        // know it happened.
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
/// A free function rather than a method so it is testable without a configured model:
/// this is where a bad round is turned into a harmless one, and it should be checkable
/// on a machine with no AI at all.
///
/// Order matters. Shape is checked before novelty, so a mangled repeat is reported as
/// mangled rather than as known - the two say different things about the reply.
pub fn sanitise(
    offered: &[String],
    in_vocabulary: &BTreeSet<String>,
    tried_before: &BTreeSet<String>,
    budget: usize,
) -> (Vec<String>, Vec<Rejected>) {
    // Padding is measured against everything the model has been told not to repeat, which
    // is both sets - `Group2` is padding whether `Group` is in the vocabulary or merely
    // failed last round.
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
/// States the refusal twice - once as what to do, once as what not to - because the one
/// failure that matters is a model helpfully answering with a whole identifier.
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
/// Carries **no hash, no target, and no mapping** - the model is asked to continue a
/// naming convention, not to recall an answer. That constraint is the provenance
/// argument in this module's documentation, and a test holds the prompt to it.
pub fn prompt(context: &Context, grammar: &Grammar, slot: &str) -> String {
    let mut out = String::new();

    if !context.libraries.is_empty() {
        out.push_str("These identifiers belong to libraries named: ");
        out.push_str(&join_capped(&context.libraries, 24));
        out.push_str(".\n\n");
    }

    if !context.examples.is_empty() {
        // **Shown split, not whole.** Handing a model `sceKernelAllocateDirectMemory`
        // and asking it for "words" produced identifier-shaped compounds -
        // `Schedparam`, `Schedpolicy` - because that is what the examples looked like.
        // Showing the seam teaches the shape directly, and no prose has to describe
        // it (D219).
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
        // The empty string is a legitimate entry - it is how a shape says "no suffix" -
        // and showing it to a model as an example of a word is nonsense.
        let existing: Vec<String> = existing.iter().filter(|w| !w.is_empty()).cloned().collect();
        if !existing.is_empty() {
            // **A sample, not the list.** Showing all of it and saying "do not repeat
            // these" reliably produces the list back with digits appended - `Ex2`,
            // `Cpu2` through `Cpu30` - because that satisfies the instruction as
            // written. A sample sets the shape without handing over something to
            // enumerate, and the sanitiser catches genuine repeats anyway, which is
            // where that job belongs (D219).
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

    // **Measured, not a hunch.** Of the words a model proposed over thirty-six rounds,
    // thirty-five per cent already existed inside a name in the shipped standard-library
    // list - `Unset` in `unsetenv`, `Object` in `kinfo_getvmobject`, `Resource` in
    // `setclassresources` - and two of the ones it repeated most, `Group` and `Node`, are
    // in there too. Those arrive for free once that list is decomposed into parts, so
    // asking for them again buys nothing.
    //
    // What no standard name contains is the vendor's own domain vocabulary. The same run
    // produced `Dma`, `Midi`, `Bios`, `Endpoint` and `Bandwidth` unprompted, and that is
    // the two-thirds worth asking for.
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

    // `write!` rather than `push_str(&format!(..))`: the same bytes without the
    // intermediate allocation, and writing into a `String` cannot fail.
    let _ = write!(
        out,
        "Suggest {} new words, as a JSON array of strings.",
        context.want.max(1)
    );
    out
}

/// Every word the grammar knows, longest first.
///
/// Longest first because [`decompose`] is greedy: with `Mem` ahead of `Memory`, the
/// word `Memory` would split as `Mem` + `ory`, and `ory` is not a word.
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
/// Greedy longest-match, and `None` the moment a piece is not a known word: a partial
/// split shown to a model is worse than none, because it teaches a seam that is not
/// there. Only ever used to illustrate a prompt, so being conservative costs nothing.
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
/// **A systematic failure rather than a taste judgement.** Told not to repeat anything in
/// a list it has just been shown, a model out of ideas returns the list again with
/// numbers appended - `Ex2`, `Ex3`, `Cpu2` through `Cpu30`. Measured, twice: one round
/// spent thirty of its forty suggestions counting.
///
/// They are worth refusing rather than merely wasteful. The grammar composes words, so
/// `Ex2` is reachable from `Ex` already; sweeping it re-covers ground at full cost.
///
/// Digits are not banned outright - `Api2` and `Attribute2` are real entries in the
/// shipped vocabulary. What is refused is a *known stem* wearing them.
fn is_padded_repeat(word: &str, known: &BTreeSet<String>) -> bool {
    let stem = word.trim_end_matches(|c: char| c.is_ascii_digit());
    stem.len() != word.len() && known.contains(&stem.to_lowercase())
}

/// A word in this grammar: capitalised, then alphanumeric.
///
/// Matches what the shipped vocabulary actually contains - `Abort`, `Api2`,
/// `Attribute2`. Anything else is not rejected as bad taste, it is rejected because the
/// grammar concatenates these directly and a word with a separator in it would generate
/// candidates that cannot be identifiers.
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
/// Three routes, tried in order, and which one worked is reported rather than hidden -
/// a model that never manages the strict shape is worth knowing about, and a silent
/// fallback makes that invisible. The sanitiser is the real gate, so a loose read is
/// safe: prose scraped by the last route is refused there and reported by word.
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

    /// **The provenance property.** The prompt carries no hash.
    ///
    /// If a model were shown a hash and asked for the name behind it, a hit would be a
    /// *recall* dressed as a derivation - `audit` could not re-derive it, and
    /// PROVENANCE.md's claim that every name comes out of inputs visible in the tree
    /// would have a hole where its foundation is. Asking only for words keeps every
    /// confirmed name `generated` at a real index.
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

    /// A *sample* of what is known is shown, with an instruction not to vary it.
    ///
    /// Showing the whole list and saying "do not repeat these" is what produced `Ex2`,
    /// `Cpu2` through `Cpu30`, and `Belt2` through `Belt30` - the instruction satisfied
    /// exactly as written. A sample sets the shape without handing over something to
    /// enumerate, and the sanitiser catches genuine repeats, which is where that job
    /// belongs.
    #[test]
    fn the_prompt_shows_a_sample_and_asks_for_something_different() {
        let asked = prompt(&context(), &grammar(), DEFAULT_SLOT);
        assert!(asked.contains("already exist, for example"), "{asked}");
        assert!(asked.contains("NOT variations"), "{asked}");
    }

    /// A long list is capped rather than sent whole.
    ///
    /// The shipped vocabulary is hundreds of words. Sending all of them would crowd out
    /// the question on a small model's context and cost tokens on a large one's.
    #[test]
    fn a_long_list_is_capped() {
        let asked = prompt(&context(), &grammar(), DEFAULT_SLOT);
        assert!(asked.contains("more"), "{asked}");
        assert!(asked.len() < 8_000, "prompt is {} bytes", asked.len());
    }

    /// A word is capitalised and alphanumeric; anything else is not one.
    ///
    /// The grammar concatenates these directly, so a word carrying a separator would
    /// generate candidates that cannot be identifiers at all - every one of them a
    /// wasted hash.
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
    ///
    /// Small models preface everything. Refusing here would discard a perfectly good
    /// answer over a sentence.
    #[test]
    fn an_array_inside_prose_is_still_read_strictly() {
        let (words, how) =
            read_words("Sure! Here you go:\n[\"Sema\", \"Attr\"]\nHope that helps.").expect("read");
        assert_eq!(words, vec!["Sema", "Attr"]);
        assert_eq!(how, Parsed::JsonArray);
    }

    /// A reply that is not an array falls back, and says that it did.
    ///
    /// The fallback existing is not the point - reporting it is. A model that never
    /// manages the asked-for shape is a prompt problem or a too-small model, and both
    /// are invisible if the fallback is silent.
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
    ///
    /// "The model said nothing" and "the model had nothing to add" call for opposite
    /// responses, and only the second means the vocabulary is exhausted.
    #[test]
    fn an_empty_reply_is_a_failure() {
        assert!(read_words("").is_none());
        assert!(read_words("   \n  ").is_none());
    }

    /// An empty JSON array does not count as a strict read.
    ///
    /// Otherwise `[]` would report `JsonArray` with nothing in it, and a caller
    /// counting successful rounds would call that one.
    #[test]
    fn an_empty_array_is_not_a_strict_read() {
        assert!(read_words("[]").is_none());
    }

    /// A word the grammar already has is refused.
    ///
    /// Not tidiness: re-adding it makes the sweep re-cover ground it already covered,
    /// so the search gets slower while finding exactly nothing new.
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

    /// Case does not smuggle a repeat past the check.
    ///
    /// The grammar's words are capitalised, so `attr` and `Attr` generate identical
    /// candidates - accepting the second because it looks different is a whole extra
    /// sweep for nothing.
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
        // `sema` is refused for its shape, which is a different fact from being a
        // repeat, and the report should not blur them.
        assert_eq!(rejected[0].because, Refusal::NotAWord);
        assert_eq!(rejected[1].because, Refusal::Duplicate);
    }

    /// **A whole identifier is refused, which is the failure that matters.**
    ///
    /// The one thing the prompt asks the model not to do is answer with a name. If one
    /// slipped through it would be swept as a single vocabulary entry, could collide,
    /// and would then be recorded `generated` - a recall wearing a derivation's record,
    /// which is precisely what the word route exists to prevent.
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

    /// A known word with digits on the end is refused as the padding it is.
    ///
    /// Measured twice: told not to repeat anything in a list it had just been shown, the
    /// model returned the list with numbers appended - one round spent thirty of its
    /// forty suggestions counting `Cpu2` to `Cpu30`. The grammar composes, so `Ex2` is
    /// already reachable from `Ex`; sweeping it re-covers ground at full cost.
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

    /// Digits are not banned - only known stems wearing them.
    ///
    /// `Api2` and `Attribute2` are real entries in the shipped vocabulary, so a blanket
    /// rule against digits would refuse words of exactly the shape being looked for.
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
    ///
    /// The sweep grows with the vocabulary, so an unbounded round makes every
    /// subsequent failure slower. Silently truncating would hide that a model's later
    /// suggestions were never tried at all.
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

    // --- the whole chain, with only the model faked ---------------------------------

    /// A model that says exactly what it is told to.
    ///
    /// Everything downstream of a reply is deterministic, and it is the part actually
    /// worth pinning: what is refused, what is swept, and whether what survives is a
    /// record the audit accepts. None of that needs a real model, and all of it would be
    /// untestable without this seam.
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

    /// The shipped grammar with one word taken back out, as it was before that word was
    /// ever learned.
    /// **The prompt says which vocabulary is already covered.**
    ///
    /// Measured: thirty-five per cent of what a model proposed over thirty-six rounds was
    /// already inside a standard-library name, so it arrives free from decomposing that
    /// list and asking for it again wastes the round. Pinned as a test because it is one
    /// sentence in a long string and would be easy to lose in an edit.
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

    /// **The refusal filter sees the injected vocabulary, not just the file.**
    ///
    /// `posix` is derived from the shipped standard-name list at parse time and never
    /// written to the grammar file, so anything reading the TOML text is blind to some
    /// thousands of words the grammar actually has - and would keep asking a model for
    /// words it already holds. This reads `Grammar::vocabulary`, which is the map after
    /// injection, and this test is here because the difference is invisible by inspection.
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

        // One that survives the shape rules, so the refusal under test is the *novelty*
        // check rather than `NotAWord` - most standard names are too long or not
        // alphabetic, and picking blindly measures the wrong filter.
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
    ///
    /// The real one is nine hundred million candidates per shape, which is the right size
    /// for the end-to-end proof above and the wrong size for everything else.
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

    /// **A word tried once is not swept again in the same run.**
    ///
    /// Measured before this existed: over thirty-six rounds the model re-proposed the same
    /// handful of words, and `Group` was accepted and swept against thirty-five million
    /// candidates twelve separate times. Nothing remembered the eleven failures, because
    /// the bank holds only successes - and the round's grown grammar is a local clone that
    /// is discarded, which the comment beside it described as making a wrong proposal
    /// "genuinely free". It is free on disk. It is not free in the loop.
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

    /// **A word that failed is refused, and still never reaches the bank.**
    ///
    /// The two are different claims and only one of them is durable. The bank is evidence -
    /// the hash is the only thing that puts a word in it - so a failure must be remembered
    /// for the run and forgotten by the file.
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
    /// **Dropping shapes is safe here and dropping words would not be**, which is the whole
    /// of D214: an index is a position in a mixed-radix number whose digits are one
    /// pattern's own word-list lengths, so removing a *pattern* cannot move a position
    /// inside the ones that remain, while removing a *word* moves every one of them. Every
    /// record these tests produce is still checked against the **complete** shape set by
    /// [`adopting`], which is a stronger claim than checking it against the grammar that
    /// made it.
    ///
    /// **Why the quadratic shapes come out.** A round re-sweeps every shape using the grown
    /// slot at full size (D264), and `learned` appears twice in two of them - about 250
    /// million candidates each against 17 million for the shapes that use it once, so 93%
    /// of a round is spent on the two shapes that between them account for 2 of the 323
    /// generated records in the database. The loop can decide that is worth paying. A unit
    /// test cannot: at 11,842 words these tests ran over nine minutes each without
    /// finishing, and the gate died on them (D320).
    ///
    /// Nothing else is faked. Real vocabulary, real hasher, real sweep, real hash.
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

    /// The grammar a record has to survive: the word adopted, and **every** shape back.
    ///
    /// Appended rather than sorted into place, because that is what a round does to the
    /// slot it grows - and the index only means anything against the ordering that
    /// produced it.
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

    /// **The claim this whole module makes, proved end to end.**
    ///
    /// A real grammar, a real hasher, a real sweep, and a real hash. The only fake is the
    /// model, which offers one word.
    ///
    /// The wrong word finds nothing and the right one recovers a real name - so the word
    /// is demonstrably what did it, rather than the name having been reachable all along.
    /// And the record that comes back is then handed to `solve::verify`, which is the
    /// function `orbistoun-cli audit` itself runs: the assertion is not "a name appeared"
    /// but **"the audit re-derives it"**, which is the property the word-not-name route
    /// exists to protect.
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

        // And the record it carries is one the audit re-derives, against the grammar as
        // it would stand once the word is adopted - with **every** shape back, including
        // the two the sweep above did not pay for. A record made under a reduced shape set
        // and accepted against the complete one is a stronger claim than one checked
        // against the grammar that made it, because per-pattern indices are exactly what
        // makes that hold (D214).
        let patterns = adopting(WORD).patterns().expect("patterns resolve");
        assert!(
            orbistoun_names::solve::verify(NAME, &hit.solved[0].derivation, &patterns, &[]),
            "the audit would refuse {:?}",
            hit.solved[0].derivation
        );
    }

    /// **The real thing.** A real model, asked for real words, swept for real.
    ///
    /// Opt-in, because it downloads a model and runs inference:
    ///
    /// ```text
    /// cargo test -p orbistoun-propose -- --ignored --nocapture a_real_model
    /// ```
    ///
    /// Everything above this point fakes the model, which is right for pinning
    /// behaviour and useless for the one question a fake cannot answer: does a small
    /// model, given this prompt, produce words that are *shaped like vendor vocabulary*
    /// at all? That is not a property of the code, so it is not asserted - it is
    /// printed, for a person to judge.
    ///
    /// What **is** asserted is the mechanism: something answered, the reply was read,
    /// and every word that reached the sweep passed the guards. Notably it asserts that
    /// no whole identifier got through, which is the failure the prompt asks the model
    /// to avoid and the guards exist to catch when it does not.
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

    /// **A word that worked is kept, and the next round is asked a harder question.**
    ///
    /// The compounding property, end to end with only the model faked. Without it the
    /// proposer is worth almost nothing: four measured runs earned two, six, one and two
    /// names, and the first and fourth earned *the same two* from the same word, because
    /// nothing kept it and every run began from the shipped vocabulary again.
    ///
    /// Round one earns the name and banks the word. Round two is handed the identical
    /// reply and gets nothing, because the word is now part of the grammar - which is
    /// exactly right, and is what "learned" has to mean.
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

    /// What was kept survives the proposer that kept it.
    ///
    /// A bank held only in memory is the failure this exists to prevent, so the file is
    /// what the next run reads - not a field somebody remembered to carry over.
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

    /// Rounds ask different questions without being told to.
    ///
    /// A loop that re-asks one question gets one answer however many times it runs. The
    /// seed advancing inside `round` is what makes "run it again" mean something, rather
    /// than something every caller has to remember.
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
    /// The other shapes generate exactly what they generated before, so hashing them
    /// again buys no coverage at all - it is the difference between about thirty million
    /// candidates and 2.6 billion.
    ///
    /// The bound is deliberately loose. Pinning the exact figure would make this a test
    /// of the shipped vocabulary's size, which changes every time a word is learned; what
    /// is being protected is that the filter happens at all.
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

    /// **The slot itself is not narrowed, and that costs eighty-three times.**
    ///
    /// Restricting the slot to only the new words would take a round from thirty million
    /// candidates to a hundred and fifty thousand. It destroys the record: an index is a
    /// position in a mixed-radix number whose digits are the word-list lengths, so a
    /// shorter list makes the recorded index name a different candidate in any grammar
    /// anybody holds - and `verify` then refuses a name that is perfectly real.
    ///
    /// This test is what stops that being re-optimised by somebody reading the sweep
    /// figures and not the provenance argument: it asserts the record survives adoption,
    /// which is the property the narrowing would break (D214).
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

        // The grammar as it stands once the word is written into the vocabulary, which
        // is what an audit runs against afterwards.
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
                &[]
            ),
            "the index stopped meaning what it meant: {:?}",
            solved.derivation
        );
    }

    /// Adding words to a slot no shape uses is refused, loudly.
    ///
    /// The failure it prevents is silent and permanent: every round would hash zero new
    /// candidates, report a clean miss, and be indistinguishable from a vocabulary that
    /// has genuinely been exhausted.
    #[test]
    fn a_slot_no_pattern_uses_is_refused() {
        let targets = orbistoun_names::solve::Targets::new([hasher().hash("anything")]);
        let error = super::Vocabulary::new(&Canned(r#"["Sema"]"#), without("Sema"), hasher())
            .with_slot("nothing-uses-this")
            .round(&targets, &context())
            .expect_err("a slot nothing references is refused");
        assert!(matches!(error, crate::Error::SlotUnused(_)), "{error}");
    }

    /// A reply with no usable word sweeps nothing rather than sweeping everything.
    ///
    /// Growing a grammar by zero words and then sweeping it would re-cover ground the
    /// base search already covered, at full cost, every single round.
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
