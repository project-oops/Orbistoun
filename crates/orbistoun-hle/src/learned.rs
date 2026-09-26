//! What the loop measured, in a form somebody can send you.
//!
//! # Why this is evidence rather than settings
//!
//! `learned.toml` began as a local cache of policy the loop worked out. That is enough to run
//! and not enough to **send**, and sending is the point: someone running orbistoun as a
//! binary, with no repository and no source, produces exactly these measurements - and they
//! are worth more to this project than most of what it can generate itself.
//!
//! The third oracle in `CLAUDE.md` is *"the guest itself - a 1-bit oracle per call site.
//! Expensive per query (a boot)."* Expensive **per person**. A hundred people turning the loop
//! on titles nobody here owns is the same oracle at a hundred times the rate, costing this
//! repository nothing and holding no title data (D297).
//!
//! # What makes one safe to receive
//!
//! It is checkable. A measurement is derived from running a binary the submitter owns, is
//! reproducible by anyone with the same title, and makes a claim that is **falsifiable by a
//! command** - which is the standard this project already sets for writing anything down, and
//! a stronger contribution model than reviewing a diff.
//!
//! A maintainer without the title cannot check one, and does not have to: it is accepted as
//! `assumed` and promoted when somebody who owns the title confirms it. That ladder is what
//! [`Oracle`] is for.

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
    /// **Recorded because the loop already knew it and was dropping it.** A finding names
    /// `libkernel::sceKernelReserveVirtualRange`; the measurement kept only the second half,
    /// and everything downstream then had to work the first half out again. Nothing could:
    /// the only lookup available is built *from* the knowledge files, so it can place a
    /// function that already has an entry and never a new measurement - which is precisely
    /// the case that needs placing (D328).
    #[serde(default)]
    pub library: String,
    /// Which guest demonstrated it.
    ///
    /// **Load-bearing.** `region_bytes` is established against one title and another may index
    /// further, so an entry that did not say which guest it came from would read as a fact
    /// about the platform. It is a fact about a run (D297).
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
    /// **One concept, two deliveries.** Writing a base through an argument and returning one
    /// are the same behaviour arriving differently, which is what lets the loop try both and
    /// compare rather than following whichever rule it happened to have (D300).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<StubRegion>,
    /// Claims this rests on that nothing measured.
    ///
    /// **Never dropped.** These were printed to a terminal and lost before this file existed,
    /// and they are the difference between a measurement and an assertion.
    #[serde(default)]
    pub assumes: Vec<String>,
}

/// What has to be observed before a measurement is worth acting on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Evidence {
    /// The guest reached code it could not reach before.
    ///
    /// Enough only for a change to what a function **answers**. A wrong answer that buys
    /// progress shows up as a wall that moved; a wrong write does not show up at all until
    /// something unrelated breaks.
    Further,
    /// A conformance check covering this function passed.
    ///
    /// Required for anything that touches guest memory - the only oracle here that says
    /// *correct* rather than *proceeded*.
    ConformanceCheck,
}

impl Learned {
    /// Reads a file, or nothing at all when there is none.
    ///
    /// # Errors
    ///
    /// When the file exists and cannot be parsed. **Deliberately not silent**: a malformed file
    /// that fell back to empty would look exactly like a machine that had measured nothing.
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

    /// Adds a measurement, replacing any earlier one for the same function.
    ///
    /// **Replaced rather than appended**, because two entries for one function are two claims
    /// about the same thing and nothing here can say which is current. The newer run measured
    /// the newer emulator.
    pub fn record(&mut self, measurement: Measurement) {
        self.measurements
            .retain(|held| held.function != measurement.function);
        self.measurements.push(measurement);
    }

    /// The policy these measurements imply.
    ///
    /// **Derived, never stored.** A measurement is a claim about a guest and a policy is a
    /// decision about a machine; deriving one from the other keeps the distinction that makes
    /// the file submittable in the first place (D297).
    ///
    /// `default_return` is untouched - it governs every function nothing here measured, so a
    /// file of measurements has nothing to say about it.
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
            // **The half this conversion used to drop.** A measurement records how it was
            // established and the policy it implied did not, so every derived answer arrived
            // downstream indistinguishable from one somebody typed - and the compatibility
            // record, which asks exactly that question, could only answer "an override exists"
            // (D557).
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
    /// **What makes receiving one safe.** A submitted measurement is checked by re-deriving it
    /// locally and comparing, not by trusting it - which is the same shape `audit --repair`
    /// already has for names, and the reason a policy entry is a better contribution than a
    /// diff (D297).
    ///
    /// Only the *claim* is compared - what it answers, what it writes. `on`, `by` and `measured`
    /// differ between any two machines by construction and say nothing about agreement.
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
    /// **Not a refutation.** It usually means the title is absent or the run never reached the
    /// call, and reporting it as a contradiction would turn "we did not look" into "it is
    /// wrong" - the distinction the whole `known_by` vocabulary exists to hold.
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

    /// A file survives being written and read back.
    ///
    /// The property that makes it submittable at all: what leaves one machine is what arrives
    /// at another, including the assumptions - which are the difference between a measurement
    /// and an assertion.
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
        // **Not a refutation.** "We never looked" and "it is wrong" are different facts.
        assert!(matches!(
            here.disagreements(&unknown).as_slice(),
            [super::Disagreement::NotMeasuredHere { .. }]
        ));
    }

    /// **The provenance of a measurement survives the trip into a policy.**
    ///
    /// This conversion is the single place a fact's origin was being dropped: a measurement
    /// says how it was established and the policy it implied did not, so every derived answer
    /// arrived downstream indistinguishable from one somebody typed into a file. The
    /// compatibility record asks exactly that question and could only ever answer "an override
    /// exists" (D557).
    ///
    /// # What this cannot assert
    ///
    /// **That the label is true.** A sweep writes `guest-observed` because that is what a sweep
    /// can see; nothing here checks that claim, and nothing could - it is the measurement's own
    /// account of itself. What is checked is that the account is not thrown away.
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

    /// **A region counts as an entry even when nothing is answered.**
    ///
    /// A region writes a base into guest memory behind a byte count nothing measured - this
    /// file's own entry says so in its `assumes`. The count that decided whether a run was
    /// honest read `overrides.len()` and never looked at regions, so a policy that wrote memory
    /// and answered nothing reported zero and read as an unassisted run (D557).
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

    /// **A symbol decided for two ways is one entry, not two.**
    ///
    /// `specific` and `propping` count symbols so they can be compared with each other and with
    /// `imports`. Counting an answer and a region separately would report two props where one
    /// function was helped, and the number would drift from the thing it is compared against.
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

    /// **An entry nobody labelled reads as a guess, which is the safe direction.**
    ///
    /// `known` is a second map beside the answers, so the two can drift - a name in `overrides`
    /// with no entry in `known`. That case defaults to [`Oracle::Assumed`], so drift can only
    /// ever call a run *less* honest than it was. The opposite default would let a policy become
    /// honest by losing information, which is the failure worth designing against (D557).
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

    /// **A measured answer does not prop a run up.**
    ///
    /// The point of the whole change, stated at the level that decides it. Nothing in the
    /// corpus is here yet - today's only learned entry is `guest-observed` - so this is the
    /// mechanism proved against a measurement the project does not have rather than one it does.
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
