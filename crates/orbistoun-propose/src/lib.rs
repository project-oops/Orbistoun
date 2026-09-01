//! Turning what a run reported into something a machine can try, and checking it.
//!
//! # The shape every proposer here has
//!
//! ```text
//!   something proposes  ->  something else disposes  ->  only what survived is kept
//! ```
//!
//! The first box may be a language model, a heuristic, or a person. **It is the least
//! important box.** What makes a proposer safe to build is the second one: an oracle
//! that is cheap, mechanical, and cannot be talked into agreeing. A proposer without one
//! is a machine for generating plausible wrong answers, and this project has a written
//! rule against exactly that (CLAUDE.md principle 3, and the automated stub-semantics
//! entry in `docs/BACKLOG.md`).
//!
//! So each proposer in this crate is named after its oracle rather than after its
//! source, and one is not built until its oracle exists.
//!
//! | Proposer | Oracle | Cost of one query | Cost of a wrong proposal |
//! |---|---|---|---|
//! | [`vocabulary`] | the NID hash | a sweep, about a minute | nothing at all |
//! | `orbistoun-turn`'s `experiment` | the fault address, re-run | **a tenth of a second** | nothing |
//!
//! # Where a model earns its place here, and where it does not
//!
//! [`vocabulary`] asks one. It is worth keeping for a narrow, measured reason: of the
//! names it has earned, **not one exists whole in any module string, and not one of the
//! words it proposed appears in a vendor-prefixed one**. The string harvester could not
//! have found them. The yield is low - two words banked over six runs - but it is not
//! redundant, which is a different thing.
//!
//! That crate's `experiment` asks nothing, and a module that did was **deleted**. It proposed which
//! import and argument to suspect, on the assumption that a boot is expensive. Measured,
//! a boot against a wall costs about a tenth of a second, so every import a guest calls
//! can be swept in under a minute - twenty-three of them, exhaustively, in fifty seconds.
//! A prior that saves nothing is not worth the room it takes up.
//!
//! # What this crate does not do
//!
//! **A proposer writes nothing.** Every one returns what it found and what it
//! discarded; deciding what to persist belongs to the caller, which keeps the decision
//! to change a tracked file in one place rather than buried inside a search.
//!
//! [`bank`] is the exception that proves it: a caller that wants the compounding
//! property has somewhere to put the words a hash confirmed, in a file of this crate's
//! own. It is not the grammar, and promoting words into the shipped vocabulary stays a
//! deliberate act with a diff.

#![forbid(unsafe_code)]

pub mod bank;
pub mod suggest;
pub mod vocabulary;

/// Why a proposal round could not be completed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Nothing was available to ask.
    ///
    /// Reported rather than treated as "no proposals": a round that asked nothing and a
    /// round that asked and got nothing are different results, and only one of them
    /// means the vocabulary is exhausted.
    #[error("no model answered: {0}")]
    Model(#[from] orbistoun_llm::Error),
    /// The grammar could not be built.
    #[error("the candidate grammar is unusable: {0}")]
    Grammar(#[from] orbistoun_names::GrammarError),
    /// The model replied with something that was not a word list.
    #[error("the reply was not a word list: {0}")]
    Reply(String),
    /// Words were to be added to a vocabulary that no shape in the grammar uses.
    ///
    /// Loud, because the alternative is silent and permanent: every round would sweep
    /// zero new candidates, report a clean miss, and look exactly like a vocabulary that
    /// has been exhausted.
    #[error(
        "no pattern in the grammar uses the `{0}` vocabulary, so a word added to it would generate nothing"
    )]
    SlotUnused(String),
}

/// A word the model offered that never reached the sweep, and why.
///
/// Kept and reported rather than dropped. A round that silently discards nine of ten
/// suggestions looks identical to a model that only offered one, and the two call for
/// opposite responses - fix the prompt, or ask a better model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejected {
    /// What was offered, verbatim.
    pub word: String,
    /// Why it was not tried.
    pub because: Refusal,
}

/// Why a proposed word was not tried.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// Not shaped like a word in this grammar.
    NotAWord,
    /// Too short or too long to be one part of a name.
    WrongLength,
    /// The grammar already has it, so trying it would search ground already covered.
    AlreadyKnown,
    /// Proposed, swept, and found nothing - in an earlier round of this same run.
    ///
    /// **Held apart from [`Self::AlreadyKnown`] because it is a different fact.** One says
    /// the vocabulary has the word; this says the vocabulary *does not*, and it was tried
    /// anyway and did not work. Measured over thirty-six rounds: `Group` was accepted and
    /// swept against thirty-five million candidates twelve separate times, because nothing
    /// remembered the eleven failures before it.
    AlreadyTried,
    /// A word the grammar already has, with digits stuck on the end.
    ///
    /// Its own refusal because it is a *systematic* answer to being told not to repeat
    /// anything: shown a list and asked for new words, a model that has run out of ideas
    /// returns `Cpu2` through `Cpu30`. Counted separately so a round that was padded
    /// this way is visible as padding rather than as thirty suggestions.
    PaddedRepeat,
    /// Offered twice in one reply.
    Duplicate,
    /// The round's ceiling on new words was already reached.
    OverBudget,
}

impl Refusal {
    /// One phrase, for a report.
    pub fn describe(self) -> &'static str {
        match self {
            Self::NotAWord => "not shaped like a word in this grammar",
            Self::WrongLength => "too short or too long to be one part of a name",
            Self::AlreadyKnown => "already in the vocabulary",
            Self::AlreadyTried => "tried in an earlier round and found nothing",
            Self::PaddedRepeat => "a word already in the vocabulary with digits appended",
            Self::Duplicate => "offered twice in one reply",
            Self::OverBudget => "past this round's ceiling on new words",
        }
    }
}
