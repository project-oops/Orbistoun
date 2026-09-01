//! Acting on what the project has already written down as unknown.
//!
//! # The gap this closes
//!
//! The dispatcher is driven entirely by **run reports** - what crashed, this time. It has
//! never read the other half of what this project knows: `orbistoun-cli questions` prints 277
//! open questions, ranked by how often a guest calls the function, and every one of them was
//! written by somebody who had just finished failing to answer it.
//!
//! The first entry is the one that matters most by a wide margin:
//!
//! ```text
//! 500031 calls in 4 module(s)   libkernel::sceKernelDirectMemoryQuery
//!   ? The map shape the guest will accept is unknown: it completes the walk,
//!     finds nothing it wants, and starts again.
//! ```
//!
//! That question names its own experiment - show the guest a different map - and the
//! apparatus has existed since D218: `MapShape` with three variants, and **nothing that ever
//! selected between them**. The instrument was built, the question was recorded, and no code
//! joined the two. A turn would report *a person must write code* while the experiment sat
//! one env var away.
//!
//! # Why matching on prose is the wrong design, and what is done instead
//!
//! A question is written for a person, so classifying one by its words is guesswork that
//! looks like a rule - and a rule that silently fails to match reads exactly like a question
//! nobody can act on. So a knowledge entry says which experiment answers it, in a field, and
//! this maps that field to a step. Anything unlabelled is reported as needing a person, with
//! the question quoted, which is what the dispatcher already does for a gap it has no rule for
//! (principle 3, D356).

use crate::axis::Axis;

/// An experiment a knowledge entry says would answer one of its open questions.
///
/// **A closed set rather than free text**, so a new one is a compile error here rather than a
/// label nothing recognises. The names are what a knowledge file writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answers {
    /// Run the title against each physical map shape and read what it queries next.
    MapShape,
    /// Watch the destination buffer of a call, to see which fields the guest reads.
    ReadsTheBuffer,
}

impl Answers {
    /// The label a knowledge file uses.
    ///
    /// One list, so the parser and anything printing the choices cannot disagree.
    pub const NAMES: [(&'static str, Self); 2] = [
        ("map-shape", Self::MapShape),
        ("reads-the-buffer", Self::ReadsTheBuffer),
    ];

    /// The experiment a knowledge entry named, or nothing when it named none.
    #[must_use]
    pub fn named(text: &str) -> Option<Self> {
        Self::NAMES
            .iter()
            .find(|(name, _)| *name == text)
            .map(|(_, answers)| *answers)
    }

    /// The axes that answer it, in the order they should be run.
    ///
    /// **Every shape, exhaustively**, for the reason the argument sweep is exhaustive: a boot
    /// costs a fraction of a second and a prior that saves nothing is not worth having. Three
    /// shapes is three boots.
    #[must_use]
    pub fn axes(self) -> Vec<Vec<Axis>> {
        match self {
            Self::MapShape => orbistoun_kernel_shapes()
                .into_iter()
                .map(|shape| vec![Axis::MapShape { shape }])
                .collect(),
            // Four words of the destination struct, which is the most the hardware watches at
            // once. The address is not known until the call is made, so this is one axis
            // rather than a sweep - the watchpoint reports what it saw.
            Self::ReadsTheBuffer => vec![vec![Axis::Watch {
                base: 0,
                words: WATCHED_WORDS,
            }]],
            #[allow(unreachable_patterns, reason = "kept total against a future variant")]
            _ => Vec::new(),
        }
    }
}

/// How many eight-byte words a watchpoint covers, bounded by the debug registers.
const WATCHED_WORDS: usize = 4;

/// The map shapes a sweep runs, by name.
///
/// Named here rather than imported so this crate keeps no dependency on the kernel - the
/// strings are the diagnostic's own vocabulary, and the worker refuses one it does not know
/// rather than falling back silently (D356).
fn orbistoun_kernel_shapes() -> Vec<&'static str> {
    // **`gapped` last and it is the one that decides.** The others are contiguous, so they
    // cannot separate the two readings by construction - they are run because a shape that
    // changes the guest's behaviour at all is worth knowing about, not because they answer
    // this (D357).
    vec!["whole", "reserved-low", "fragmented", "gapped"]
}

/// One open question, as a knowledge entry records it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    /// The function it is about.
    pub function: String,
    /// What is unknown, in the words somebody wrote.
    pub asked: String,
    /// How many times the corpus called that function.
    ///
    /// **The ranking, and it is not arbitrary.** A question about a function nothing calls can
    /// wait; the one at the top of this order blocks 67.5% of every call recorded.
    pub calls: u64,
    /// The experiment that would answer it, where the entry names one.
    pub answers: Option<Answers>,
}

impl Question {
    /// Whether the loop can attempt this without a person.
    #[must_use]
    pub const fn is_automatic(&self) -> bool {
        self.answers.is_some()
    }
}

/// The questions worth attempting, most-called first.
///
/// **Only the labelled ones, and the rest are not hidden.** A question with no experiment
/// named is still a question; the caller reports it rather than this filtering it away, so
/// "nothing to do" and "nothing labelled" stay different facts.
#[must_use]
pub fn attemptable(questions: &[Question]) -> Vec<&Question> {
    let mut out: Vec<&Question> = questions.iter().filter(|q| q.is_automatic()).collect();
    out.sort_by(|a, b| {
        b.calls
            .cmp(&a.calls)
            .then_with(|| a.function.cmp(&b.function))
    });
    out
}

/// What a question's run is read for, and what each reading would mean.
///
/// # Why an experiment needs this and not just axes
///
/// [`Answers::axes`] says what to *run*. Without a companion saying what to *read*, a turn can
/// only report what every diagnostic reports - the fault moved, the guest reached further -
/// and those answer a question about crashing, not the question that was asked.
///
/// The map-shape question is the case that makes it obvious. Whether the guest accepts a map
/// is not visible in reach at all: it walks every shape correctly and restarts. What separates
/// the two live readings of the second field is **which offset it queries next**, and that is
/// arithmetic on numbers already in the trace (D357).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reading {
    /// Which boundary the guest feeds back when it walks a map.
    ///
    /// Settled only by a map with a hole in it: where every region begins where the last
    /// ended, `end` and `start + size` are the same number and no run can separate them.
    WalksBy(Walk),
    /// Nothing in the run distinguished the possibilities.
    Undecided(&'static str),
}

/// Which value a guest feeds back to walk a memory map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Walk {
    /// The end of the region it was shown - so a hole is skipped.
    End,
    /// Where the next region starts - so a hole is stepped over to its far side.
    NextStart,
}

/// Reads which boundary a guest walked by, from the offsets it queried.
///
/// # What makes this decidable
///
/// Given a map with a hole - a region ending at `E`, the next starting at `S > E` - a guest
/// that feeds back the end queries `E`, and one that feeds back the next start queries `S`.
/// Two different numbers, both in the trace, and no judgement in between.
///
/// **Undecided is an answer and is reported as one.** Where the map is contiguous the two
/// readings coincide by construction, and saying so is the whole point: a run that could not
/// have separated them must not be recorded as having failed to.
#[must_use]
pub fn walked_by(map: &[(u64, u64, bool)], queried: &[u64]) -> Reading {
    let holes: Vec<(u64, u64)> = map
        .windows(2)
        .filter(|pair| pair[1].0 > pair[0].1)
        .map(|pair| (pair[0].1, pair[1].0))
        .collect();
    if holes.is_empty() {
        return Reading::Undecided(
            "the map is contiguous, so feeding back an end and a next start are the same number",
        );
    }
    if queried.len() < 2 {
        return Reading::Undecided("the guest queried fewer than two offsets");
    }

    for (end, next_start) in holes {
        if queried.contains(&end) && !queried.contains(&next_start) {
            return Reading::WalksBy(Walk::End);
        }
        if queried.contains(&next_start) && !queried.contains(&end) {
            return Reading::WalksBy(Walk::NextStart);
        }
    }
    Reading::Undecided("the guest queried neither side of any hole")
}

#[cfg(test)]
mod tests {
    use super::{Answers, Question, attemptable};

    fn question(function: &str, calls: u64, answers: Option<Answers>) -> Question {
        Question {
            function: function.to_owned(),
            asked: "something is unknown".to_owned(),
            calls,
            answers,
        }
    }

    /// **A question with no experiment named is not attempted, and not hidden either.**
    ///
    /// Filtering it away here would make "we have no rule for this" indistinguishable from
    /// "there is nothing to ask", which is the distinction `Step::Person` exists to hold.
    #[test]
    fn only_a_question_naming_its_experiment_is_attempted() {
        let all = vec![
            question("sceQuiet", 1, Some(Answers::MapShape)),
            question("sceLoud", 500_000, None),
        ];

        let attempt = attemptable(&all);
        assert_eq!(attempt.len(), 1, "the unlabelled one is not attempted");
        assert_eq!(attempt[0].function, "sceQuiet");
        assert_eq!(all.len(), 2, "and it is still in the list to report");
    }

    /// **Ranked by how often a guest calls the function.**
    ///
    /// A question about something nothing calls can wait. The top of this order is what blocks
    /// two thirds of every call in the corpus.
    #[test]
    fn the_most_called_question_is_attempted_first() {
        let all = vec![
            question("sceRare", 12, Some(Answers::MapShape)),
            question("sceHot", 500_031, Some(Answers::MapShape)),
        ];

        assert_eq!(attemptable(&all)[0].function, "sceHot");
    }

    /// The map-shape experiment runs every shape, one boot each.
    #[test]
    fn the_map_shape_experiment_sweeps_every_shape() {
        let axes = Answers::MapShape.axes();

        assert_eq!(axes.len(), 4, "and the fourth is the one with a hole in it");
        assert!(
            axes.iter().all(|a| a.len() == 1),
            "one shape per run - crossing them would ask a different question"
        );
    }

    /// A label nothing recognises is refused rather than guessed at.
    #[test]
    fn an_unknown_experiment_label_is_not_invented() {
        assert_eq!(Answers::named("map-shape"), Some(Answers::MapShape));
        assert_eq!(Answers::named("map shape"), None, "not close enough");
        assert_eq!(Answers::named(""), None);
    }

    /// **A contiguous map cannot answer the question, and says so.**
    ///
    /// Where each region begins where the last ended, feeding back an end and feeding back the
    /// next start produce identical numbers. A run under such a map has not failed to decide -
    /// it could not have decided, and recording those as the same thing is how a question stays
    /// open while looking asked (D218, D357).
    #[test]
    fn a_map_with_no_hole_cannot_separate_the_two_readings() {
        let contiguous = [(0, 0x1000, false), (0x1000, 0x4000, true)];

        assert!(matches!(
            super::walked_by(&contiguous, &[0, 0x1000, 0x4000]),
            super::Reading::Undecided(_)
        ));
    }

    /// **A hole decides it, and the decision is arithmetic.**
    ///
    /// Region ends at 0x2000, next begins at 0x4000. A guest feeding back the end queries
    /// 0x2000; one feeding back the next start queries 0x4000. Both numbers are in the trace.
    #[test]
    fn a_hole_separates_walking_by_end_from_walking_by_next_start() {
        let gapped = [(0, 0x2000, false), (0x4000, 0x8000, false)];

        assert_eq!(
            super::walked_by(&gapped, &[0, 0x2000]),
            super::Reading::WalksBy(super::Walk::End)
        );
        assert_eq!(
            super::walked_by(&gapped, &[0, 0x4000]),
            super::Reading::WalksBy(super::Walk::NextStart)
        );
    }

    /// Querying both sides decides nothing, and is not read as either.
    #[test]
    fn querying_both_sides_of_a_hole_is_undecided() {
        let gapped = [(0, 0x2000, false), (0x4000, 0x8000, false)];

        assert!(matches!(
            super::walked_by(&gapped, &[0, 0x2000, 0x4000]),
            super::Reading::Undecided(_)
        ));
    }

    /// One offset is not a walk.
    #[test]
    fn a_single_query_is_not_enough_to_read_a_walk() {
        let gapped = [(0, 0x2000, false), (0x4000, 0x8000, false)];

        assert!(matches!(
            super::walked_by(&gapped, &[0]),
            super::Reading::Undecided(_)
        ));
    }
}
