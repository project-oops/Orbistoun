//! Grading a change against a spec, rather than against whether the guest survived it.
//!
//! `FURTHER` says the guest got past something and nothing once a run stops faulting (D301). The
//! conformance probe grades named checks against a spec: `037-math/sqrt` passing means sqrt is
//! correct. With an oracle that cannot be fooled, the generator of changes can stay simple (D303).
//! A check that goes from failing to passing is evidence; one that goes the other way refuses the
//! change, since one function's correctness cannot be weighed against another's.

use std::collections::BTreeMap;

use orbistoun_probe::{Record, Status};

/// What a probe run concluded, check by check.
///
/// Keyed by the check's own identifier, `section/name`, as the probe announces it. Comparing counts
/// alone would let a change that broke one check and fixed another look like no change.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Score {
    /// Every check the run reached, and what it concluded.
    pub checks: BTreeMap<String, Status>,
}

impl Score {
    /// Reads a probe transcript.
    ///
    /// Lines that are not results are ignored rather than refused: a transcript carries
    /// negotiation, metadata and free text.
    #[must_use]
    pub fn read(transcript: &str) -> Self {
        let mut checks = BTreeMap::new();
        for line in transcript.lines() {
            let Ok(parsed) = orbistoun_probe::parse_line(line) else {
                continue;
            };
            if let orbistoun_probe::Line::Record(Record::Res { check, status, .. }) = parsed {
                checks.insert(check, status);
            }
        }
        Self { checks }
    }

    /// How many checks passed.
    #[must_use]
    pub fn passing(&self) -> usize {
        self.checks
            .values()
            .filter(|status| **status == Status::Pass)
            .count()
    }

    /// What changed between this score and a later one.
    #[must_use]
    pub fn against(&self, later: &Self) -> Verdict {
        let mut fixed = Vec::new();
        let mut broken = Vec::new();
        for (check, after) in &later.checks {
            let before = self.checks.get(check);
            match (before, after) {
                (Some(Status::Pass), s) if *s != Status::Pass => broken.push(check.clone()),
                (Some(b), Status::Pass) if *b != Status::Pass => fixed.push(check.clone()),
                _ => {}
            }
        }
        // A check that stopped running counts as broken: a probe that dies earlier writes a shorter
        // report, not a cleaner one.
        for (check, before) in &self.checks {
            if *before == Status::Pass && !later.checks.contains_key(check) {
                broken.push(check.clone());
            }
        }
        Verdict { fixed, broken }
    }
}

/// What a change did to the checks.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Verdict {
    /// Checks that were failing and now pass.
    pub fixed: Vec<String>,
    /// Checks that were passing and now do not, including ones that stopped running.
    pub broken: Vec<String>,
}

impl Verdict {
    /// Whether a change is worth keeping.
    ///
    /// Something has to improve, or the change is unevidenced, and nothing may regress, because one
    /// function's correctness cannot be weighed against another's.
    #[must_use]
    pub fn is_an_improvement(&self) -> bool {
        !self.fixed.is_empty() && self.broken.is_empty()
    }

    /// One line a person reads.
    #[must_use]
    pub fn say(&self) -> String {
        match (self.fixed.len(), self.broken.len()) {
            (0, 0) => "nothing changed".to_owned(),
            (fixed, 0) => format!("{fixed} improved: {}", self.fixed.join(", ")),
            (0, broken) => format!("{broken} regressed: {}", self.broken.join(", ")),
            (fixed, broken) => format!(
                concat!(
                    "{} fixed and {} regressed - refused, because nothing here can weigh ",
                    "one against the other: fixed {}, broken {}"
                ),
                fixed,
                broken,
                self.fixed.join(", "),
                self.broken.join(", ")
            ),
        }
    }
}

/// How one guest fared, for grading a change against a corpus rather than a spec.
///
/// A probe grades only what somebody thought to test; every title on a machine is an independent
/// guest with its own expectations of the same functions, so a corpus of them is a regression suite
/// that grows with its user (D303).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reach {
    /// Distinct imports the guest called before it stopped.
    pub reached: usize,
    /// Whether it stopped at an address it asked for, rather than in non-code.
    ///
    /// An illegal instruction, a breakpoint or a stack overflow means the guest was derailed, and a
    /// change that buys reach while derailing something has broken it.
    pub touched: bool,
    /// Whether it faulted at all.
    pub faulted: bool,
}

/// What a corpus of guests said about a change.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Corpus {
    /// One entry per guest, by title.
    pub guests: BTreeMap<String, Reach>,
}

impl Corpus {
    /// Records how one guest fared.
    pub fn saw(&mut self, title: &str, reach: Reach) {
        self.guests.insert(title.to_owned(), reach);
    }

    /// What changed between this corpus and a later one.
    ///
    /// Reach up somewhere, down nowhere, and nothing newly derailed (D303). One guest getting
    /// further is ordinary and a wrong answer can buy it; several agreeing while none regresses is
    /// a stronger claim.
    #[must_use]
    pub fn against(&self, later: &Self) -> Verdict {
        let mut fixed = Vec::new();
        let mut broken = Vec::new();
        for (title, after) in &later.guests {
            let Some(before) = self.guests.get(title) else {
                continue;
            };
            // Derailing is a regression whatever it did to reach: how far a guest running in
            // non-code got is not a measurement.
            if before.touched && !after.touched {
                broken.push(format!("{title} (derailed into non-code)"));
            } else if after.reached < before.reached {
                broken.push(format!(
                    "{title} (reached {} against {})",
                    after.reached, before.reached
                ));
            } else if after.reached > before.reached {
                fixed.push(format!(
                    "{title} (reached {} against {})",
                    after.reached, before.reached
                ));
            }
        }
        Verdict { fixed, broken }
    }
}

#[cfg(test)]
mod tests {
    use super::{Score, Verdict};

    /// A transcript the probe could have written.
    fn transcript(results: &[(&str, &str)]) -> String {
        results
            .iter()
            .map(|(check, status)| format!("OBS|res|{check}|{status}|||spec"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// One guest fared better and none worse.
    #[test]
    fn a_corpus_keeps_a_change_no_guest_regressed_on() {
        let reach = |reached, touched| super::Reach {
            reached,
            touched,
            faulted: true,
        };
        let mut before = super::Corpus::default();
        before.saw("A", reach(10, true));
        before.saw("B", reach(20, true));

        let mut after = super::Corpus::default();
        after.saw("A", reach(14, true));
        after.saw("B", reach(20, true));

        assert!(before.against(&after).is_an_improvement());
    }

    /// Derailing is a regression however far the guest got.
    #[test]
    fn a_guest_derailed_into_non_code_is_a_regression_even_if_it_reached_further() {
        let mut before = super::Corpus::default();
        before.saw(
            "A",
            super::Reach {
                reached: 10,
                touched: true,
                faulted: true,
            },
        );
        let mut after = super::Corpus::default();
        after.saw(
            "A",
            super::Reach {
                reached: 40,
                touched: false,
                faulted: true,
            },
        );

        let verdict = before.against(&after);
        assert!(!verdict.is_an_improvement(), "{}", verdict.say());
        assert!(verdict.say().contains("derailed"), "{}", verdict.say());
    }

    /// A guest that got less far refuses the change, whatever another said.
    #[test]
    fn one_guest_regressing_refuses_a_change_the_others_liked() {
        let reach = |reached| super::Reach {
            reached,
            touched: true,
            faulted: true,
        };
        let mut before = super::Corpus::default();
        before.saw("A", reach(10));
        before.saw("B", reach(20));

        let mut after = super::Corpus::default();
        after.saw("A", reach(30));
        after.saw("B", reach(11));

        let verdict = before.against(&after);
        assert!(
            !verdict.is_an_improvement(),
            "nothing here can weigh one guest against another: {}",
            verdict.say()
        );
    }

    /// The grading is per check, not per count.
    #[test]
    fn a_change_that_fixes_one_and_breaks_one_is_not_no_change() {
        let before = Score::read(&transcript(&[("a/one", "pass"), ("a/two", "fail")]));
        let after = Score::read(&transcript(&[("a/one", "fail"), ("a/two", "pass")]));

        let verdict = before.against(&after);
        assert_eq!(
            before.passing(),
            after.passing(),
            "the counts are identical"
        );
        assert_eq!(verdict.fixed, vec!["a/two"]);
        assert_eq!(verdict.broken, vec!["a/one"]);
        assert!(
            !verdict.is_an_improvement(),
            "and it must be refused: nothing here can weigh one against the other"
        );
    }

    /// Something must improve, or there is no evidence for the change.
    #[test]
    fn a_change_that_breaks_nothing_and_fixes_nothing_is_not_kept() {
        let same = transcript(&[("a/one", "pass"), ("a/two", "fail")]);
        let verdict = Score::read(&same).against(&Score::read(&same));

        assert_eq!(verdict, Verdict::default());
        assert!(!verdict.is_an_improvement());
        assert_eq!(verdict.say(), "nothing changed");
    }

    /// A change worth keeping fixes something and breaks nothing.
    #[test]
    fn a_check_that_starts_passing_is_evidence() {
        let before = Score::read(&transcript(&[("a/one", "fail")]));
        let after = Score::read(&transcript(&[("a/one", "pass")]));

        let verdict = before.against(&after);
        assert!(verdict.is_an_improvement());
        assert!(verdict.say().contains("a/one"), "{}", verdict.say());
    }

    /// A check that stops running counts as broken, so a crash never reads as an improvement.
    #[test]
    fn a_check_that_disappears_is_a_regression_not_a_silence() {
        let before = Score::read(&transcript(&[("a/one", "pass"), ("a/two", "pass")]));
        let after = Score::read(&transcript(&[("a/one", "pass")]));

        let verdict = before.against(&after);
        assert_eq!(verdict.broken, vec!["a/two"]);
        assert!(
            !verdict.is_an_improvement(),
            "the probe dying earlier is not the tree getting better"
        );
    }
}
