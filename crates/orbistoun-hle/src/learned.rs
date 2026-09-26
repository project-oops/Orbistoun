//! What the loop measured, in a form somebody can send.
//!
//! `learned.toml` holds measurements, not settings: anyone running the orbistoun binary on a
//! title they own produces them, with no repository and no title data changing hands (D297).
//! A measurement is reproducible by anyone with the same title and falsifiable by a command. A
//! maintainer without the title accepts it as `assumed`, and it is promoted when an owner of the
//! title confirms it, which is what [`Oracle`] is for.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{HleError, StubPolicy, StubRegion, StubReturn, knowledge::Oracle};

/// Everything a machine has measured and kept.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Learned {
    /// One per function, in the order they were established.
    #[serde(default, rename = "measurement")]
    pub measurements: Vec<Measurement>,
}

/// One function's behaviour, as a guest demonstrated it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Measurement {
    /// The function, bare - no `library::` prefix.
    pub function: String,
    /// The library that declares it.
    ///
    /// Recorded from the finding, because the only other lookup is built from the knowledge files
    /// and cannot place a function that has no entry yet.
    #[serde(default)]
    pub library: String,
    /// Which guest demonstrated it.
    ///
    /// `region_bytes` is established against one title and another may index further, so the entry
    /// is a fact about a run, not the platform (D297).
    pub measured: String,
    /// When, so a reader can tell a fresh claim from one that predates a rewrite.
    pub on: String,
    /// Which build established it.
    pub by: String,
    /// How it was established. Always [`Oracle::GuestObserved`] from a sweep.
    pub known: Oracle,
    /// What has to be seen before this is worth keeping.
    pub evidence: Evidence,
    /// What the function should answer, where the measurement says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answers: Option<StubReturn>,
    /// A region to give the guest, and how it should arrive.
    ///
    /// Writing a base through an argument and returning one are the same behaviour delivered
    /// differently, so the loop can try both and compare (D300).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<StubRegion>,
    /// Claims this rests on that nothing measured. Never dropped: they separate a measurement from
    /// an assertion.
    #[serde(default)]
    pub assumes: Vec<String>,
}

/// What has to be observed before a measurement is worth acting on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Evidence {
    /// The guest reached code it could not reach before.
    ///
    /// Enough only for a change to what a function answers: a wrong answer shows up as a moved
    /// wall, while a wrong write shows up only when something unrelated breaks.
    Further,
    /// A conformance check covering this function passed.
    ///
    /// Required for anything that touches guest memory; the only oracle here that says correct
    /// rather than proceeded.
    ConformanceCheck,
}

impl Learned {
    /// Reads a file, or nothing at all when there is none.
    ///
    /// # Errors
    ///
    /// When the file exists and cannot be parsed; a malformed file never reads as empty.
    pub fn load(path: &std::path::Path) -> Result<Self, HleError> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(source) => {
                return Err(HleError::Read {
                    path: path.to_path_buf(),
                    source,
                });
            }
        };
        toml::from_str(&text).map_err(|source| HleError::Parse {
            path: path.to_path_buf(),
            source: Box::new(source),
        })
    }

    /// The file as text, for writing it back.
    ///
    /// # Errors
    ///
    /// When the measurements cannot be serialised.
    pub fn to_toml(&self) -> Result<String, HleError> {
        Ok(toml::to_string_pretty(self)?)
    }

    /// Adds a measurement, replacing any earlier one for the same function: the newer run measured
    /// the newer emulator.
    pub fn record(&mut self, measurement: Measurement) {
        self.measurements
            .retain(|held| held.function != measurement.function);
        self.measurements.push(measurement);
    }

    /// The policy these measurements imply.
    ///
    /// Derived, never stored: a measurement is a claim about a guest, a policy a decision about a
    /// machine (D297). `default_return` is untouched, since it governs functions nothing measured.
    #[must_use]
    pub fn policy(&self) -> StubPolicy {
        let mut overrides = HashMap::new();
        let mut regions = HashMap::new();
        let mut known = HashMap::new();
        for measurement in &self.measurements {
            if let Some(answer) = measurement.answers {
                overrides.insert(measurement.function.clone(), answer);
            }
            if let Some(region) = measurement.region {
                regions.insert(measurement.function.clone(), region);
            }
            // Each answer carries how it was established into the policy, so the compatibility record can
            // tell a measured answer from a typed one (D557).
            if measurement.answers.is_some() || measurement.region.is_some() {
                known.insert(measurement.function.clone(), measurement.known);
            }
        }
        StubPolicy {
            default_return: StubReturn::Unimplemented,
            overrides,
            regions,
            known,
        }
    }

    /// How this file and another disagree, function by function.
    ///
    /// A submitted measurement is checked by re-deriving it locally and comparing (D297). Only the
    /// claim is compared; `on`, `by` and `measured` differ between any two machines.
    #[must_use]
    pub fn disagreements(&self, other: &Self) -> Vec<Disagreement> {
        let mine: HashMap<&str, &Measurement> = self
            .measurements
            .iter()
            .map(|m| (m.function.as_str(), m))
            .collect();
        let mut out = Vec::new();
        for theirs in &other.measurements {
            match mine.get(theirs.function.as_str()) {
                None => out.push(Disagreement::NotMeasuredHere {
                    function: theirs.function.clone(),
                }),
                Some(ours) if ours.answers != theirs.answers || ours.region != theirs.region => {
                    out.push(Disagreement::Differs {
                        function: theirs.function.clone(),
                        here: format!("{:?} / {:?}", ours.answers, ours.region),
                        there: format!("{:?} / {:?}", theirs.answers, theirs.region),
                    });
                }
                Some(_) => {}
            }
        }
        out
    }
}

/// One way a submitted measurement fails to match what this machine found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Disagreement {
    /// This machine has no measurement for it.
    ///
    /// Not a refutation: usually the title is absent or the run never reached the call.
    NotMeasuredHere {
        /// The function the submission names.
        function: String,
    },
    /// Both measured it and they do not agree.
    Differs {
        /// The function.
        function: String,
        /// What this machine found.
        here: String,
        /// What the submission claims.
        there: String,
    },
}

#[cfg(test)]
mod tests {
    use super::{Evidence, Learned, Measurement};
    use crate::{Delivery, StubRegion, StubReturn, knowledge::Oracle};

    /// One measurement, as a sweep would produce it.
    fn measurement(function: &str, region: u64) -> Measurement {
        Measurement {
            function: function.to_owned(),
            library: "libkernel".to_owned(),
            measured: "PPSA02664".to_owned(),
            on: "2026-08-26".to_owned(),
            by: "orbistoun 0.1.0".to_owned(),
            known: Oracle::GuestObserved,
            evidence: Evidence::ConformanceCheck,
            answers: Some(StubReturn::Ok),
            region: Some(StubRegion {
                via: Delivery::Argument(0),
                bytes: region,
            }),
            assumes: vec!["the size is a guess".to_owned()],
        }
    }

    /// A file survives being written and read back, assumptions included.
    #[test]
    fn a_measurement_survives_a_round_trip() {
        let mut learned = Learned::default();
        learned.record(measurement("sceFoo", 0x2000));
        let text = learned.to_toml().expect("serialises");
        let back: Learned = toml::from_str(&text).expect("parses");

        assert_eq!(back.measurements.len(), 1);
        assert_eq!(back.measurements[0], learned.measurements[0]);
        assert!(
            !back.measurements[0].assumes.is_empty(),
            "assumptions must survive, or the claim arrives stronger than it left"
        );
    }

    /// A second measurement of one function replaces the first.
    #[test]
    fn re_measuring_replaces_rather_than_accumulates() {
        let mut learned = Learned::default();
        learned.record(measurement("sceFoo", 0x2000));
        learned.record(measurement("sceFoo", 0x4000));

        assert_eq!(learned.measurements.len(), 1, "one function, one claim");
        assert_eq!(
            learned.measurements[0].region.expect("a write").bytes,
            0x4000,
            "and the newer run measured the newer emulator"
        );
    }

    /// The policy is derived, and says nothing about what was not measured.
    #[test]
    fn the_derived_policy_leaves_unmeasured_functions_alone() {
        let mut learned = Learned::default();
        learned.record(measurement("sceFoo", 0x2000));
        let policy = learned.policy();

        assert_eq!(policy.for_symbol("sceFoo"), StubReturn::Ok);
        assert_eq!(
            policy.for_symbol("sceSomethingElse"),
            StubReturn::Unimplemented,
            "a file of measurements has nothing to say about functions it never saw"
        );
    }

    /// Agreement is silence; disagreement is named.
    #[test]
    fn a_submission_is_checked_by_re_deriving_it() {
        let mut here = Learned::default();
        here.record(measurement("sceFoo", 0x2000));

        let mut agrees = Learned::default();
        agrees.record(measurement("sceFoo", 0x2000));
        assert!(
            here.disagreements(&agrees).is_empty(),
            "two machines that measured the same thing agree"
        );

        let mut differs = Learned::default();
        differs.record(measurement("sceFoo", 0x9000));
        assert!(matches!(
            here.disagreements(&differs).as_slice(),
            [super::Disagreement::Differs { .. }]
        ));

        let mut unknown = Learned::default();
        unknown.record(measurement("sceBar", 0x2000));
        // Not a refutation: "not looked at" and "wrong" are different facts.
        assert!(matches!(
            here.disagreements(&unknown).as_slice(),
            [super::Disagreement::NotMeasuredHere { .. }]
        ));
    }

    /// The provenance of a measurement reaches the policy it implies.
    ///
    /// Whether the label is true is the measurement's own account; what is checked is that the
    /// account is kept (D557).
    #[test]
    fn a_measurements_provenance_reaches_the_policy_it_implies() {
        let mut learned = Learned::default();
        learned.record(measurement("sceFoo", 0x1000));
        let policy = learned.policy();

        assert_eq!(
            policy.provenance("sceFoo"),
            Oracle::GuestObserved,
            "the sweep said how it knew, and the policy forgot"
        );
        assert!(
            !policy.provenance("sceFoo").is_evidence(),
            "a guest proceeding is consistency, not measurement - so it still props a run up"
        );
    }

    /// A region counts as an entry even when nothing is answered, so a policy that only writes
    /// memory is not reported as an unassisted run.
    #[test]
    fn a_policy_that_only_writes_memory_is_still_a_policy_that_helps() {
        let mut learned = Learned::default();
        let mut writes_only = measurement("sceFoo", 0x1000);
        writes_only.answers = None;
        learned.record(writes_only);
        let policy = learned.policy();

        assert!(
            policy.overrides.is_empty(),
            "nothing is answered - which is the shape that used to report zero"
        );
        assert_eq!(policy.specific(), 1, "but one symbol is decided for");
        assert_eq!(
            policy.propping(),
            1,
            "and it rests on a byte count nothing measured"
        );
    }

    /// A symbol decided for two ways is one entry, not two, so counts stay comparable with
    /// `imports`.
    #[test]
    fn one_symbol_answered_and_written_is_counted_once() {
        let mut learned = Learned::default();
        learned.record(measurement("sceFoo", 0x1000));
        let policy = learned.policy();

        assert_eq!(policy.overrides.len(), 1);
        assert_eq!(policy.regions.len(), 1);
        assert_eq!(policy.specific(), 1, "one symbol, decided for in two ways");
        assert_eq!(policy.propping(), 1);
    }

    /// An entry with no label reads as a guess, so drift between the maps can only make a run
    /// look less honest, never more.
    #[test]
    fn an_unlabelled_answer_is_a_guess_rather_than_evidence() {
        let mut policy = crate::StubPolicy::default();
        policy
            .overrides
            .insert("sceTyped".to_owned(), StubReturn::Ok);

        assert_eq!(
            policy.provenance("sceTyped"),
            Oracle::Assumed,
            "somebody typed an answer into a file and said nothing about where it came from"
        );
        assert_eq!(
            policy.propping(),
            1,
            "so it props the run up, and losing the label cannot make a run look honest"
        );
    }

    /// A measured answer is knowledge rather than a prop.
    #[test]
    fn an_answer_measured_on_hardware_is_knowledge_rather_than_a_prop() {
        let mut learned = Learned::default();
        let mut from_hardware = measurement("sceFoo", 0x1000);
        from_hardware.known = Oracle::Measured;
        learned.record(from_hardware);
        let policy = learned.policy();

        assert_eq!(policy.specific(), 1, "one symbol is still decided for");
        assert_eq!(
            policy.propping(),
            0,
            "but it was measured on the target, so the run measures the emulator as it stands"
        );
    }
}
