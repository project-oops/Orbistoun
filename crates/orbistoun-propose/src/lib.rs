//! Turning what a run reported into something a machine can try, and checking it.
//!
//! Every proposer here pairs a source (a model, a heuristic or a person) with an oracle that is
//! cheap, mechanical and cannot be talked into agreeing, and only what the oracle accepts is
//! kept (D214). [`vocabulary`]'s oracle is the NID hash: a sweep per query, nothing lost on a
//! wrong proposal. A proposer writes nothing: it returns what it found and discarded, and the
//! caller decides what to persist. [`bank`] is this crate's own file of hash-confirmed words;
//! promoting them into the shipped vocabulary is a deliberate change with a diff.

#![forbid(unsafe_code)]

pub mod bank;
pub mod suggest;
pub mod vocabulary;

/// Why a proposal round could not be completed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Nothing was available to ask.
    ///
    /// Reported rather than treated as "no proposals": asking nothing and getting nothing are
    /// different results.
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
    /// Loud, because otherwise every round sweeps zero new candidates and reads as an exhausted
    /// vocabulary.
    #[error(
        "no pattern in the grammar uses the `{0}` vocabulary, so a word added to it would generate nothing"
    )]
    SlotUnused(String),
}

/// A word the model offered that never reached the sweep, and why.
///
/// Reported so a round that discarded most suggestions is distinguishable from a model that
/// offered few.
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
    /// Proposed, swept, and found nothing in an earlier round of this same run.
    ///
    /// Distinct from [`Self::AlreadyKnown`]: the vocabulary lacks the word, and it has already
    /// failed, so it is not swept again.
    AlreadyTried,
    /// A word the grammar already has, with digits stuck on the end.
    ///
    /// A model out of ideas pads with `Cpu2` through `Cpu30`; counted separately so padding is
    /// visible as padding.
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
