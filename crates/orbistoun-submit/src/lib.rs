//! What one machine has to contribute, gathered into one directory.
//!
//! # Why a bundle exists at all
//!
//! Every artifact a turn produces already writes itself somewhere. Nothing said *here is
//! what this machine has to contribute*, and for a checkout that is a convenience - the
//! files are in the tree and a diff shows them. For somebody running a binary it is the
//! difference between contributing and not: their measurements sit in a data directory
//! under their profile, their title results in whatever directory they happened to run
//! from, and no single command collects either.
//!
//! # What a submission is allowed to carry
//!
//! Two kinds of claim, and one thing that is not a claim at all.
//!
//! - **Measurements** - what a function must answer, and what that rests on.
//! - **Title results** - how far one title got, and under which policy.
//! - **Proposals** - a source change, which nothing here can check.
//!
//! The first two are derived from running a binary the submitter owns, reproducible by
//! anyone with the same title, and falsifiable by a command. That is a stronger contribution
//! model than a diff, because a maintainer without the title cannot check a diff either -
//! but they can accept a claim as `assumed` and promote it when somebody who owns the title
//! confirms it (D297).
//!
//! **A [`Proposal`] is carried and reported separately, because it is settled differently.**
//! A measurement is settled by re-deriving it; a patch is settled by a person reading it and
//! running the gate. Listing them together would let a diff inherit the trust the
//! measurements earned (D322).
//!
//! **Traces and run reports are deliberately excluded.** They are inputs rather than claims,
//! they are large, and they carry far more of a title than a result needs to. A submission
//! should be readable by the person receiving it.
//!
//! # How a received bundle is checked
//!
//! By re-deriving it, never by trusting it. [`Bundle::disagreements`] compares a submission
//! against what this machine found and names every difference; agreement is silence. That is
//! the same shape `audit --repair` already has for names, and the reason a claim is a better
//! contribution than a patch.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use orbistoun_hle::knowledge::Oracle;
use orbistoun_hle::learned::Learned;
use orbistoun_overrides::Status;

/// The file a bundle's manifest is written to.
pub const MANIFEST_FILE: &str = "manifest.toml";
/// The file the measurements are written to, named as they are named at rest.
pub const LEARNED_FILE: &str = "learned.toml";
/// The file the title results are written to.
pub const RESULTS_FILE: &str = "results.toml";
/// Where source patches belong.
pub const PATCHES_DIR: &str = "patches";
/// What each patch in [`PATCHES_DIR`] rests on.
pub const PROPOSALS_FILE: &str = "patches.toml";

/// A source change somebody or something is proposing, and what it rests on.
///
/// # Why this is not the thing the project warned against
///
/// `THE_LOOP.md` says a tool *"that produces plausible implementations **with no verification
/// step** makes the codebase worse rather than better"*. The operative clause is the middle
/// one. A patch that arrives here is inert: it is a file, nothing applies it, and it becomes
/// a change only when a person reads it, runs the gate against it and merges it. That is a
/// verification step, and a stronger one than most code in this tree got.
///
/// **The real constraint is provenance, not verification.** Principle 1 says a model in the
/// loop is a third route to the convergence problem: *"this is what the function does"* can be
/// recalled and then dressed as reasoning. Generating an implementation is exactly where that
/// is most likely and least visible - so a proposal carries [`Oracle`] like every other
/// recorded fact, and one that cannot say better than `assumed` is merged by somebody willing
/// to say where the behaviour came from, or not at all.
///
/// The diff itself is a file in [`PATCHES_DIR`] rather than a string in here. A bundle should
/// be readable, and a patch is read with the tools people already read patches with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Proposal {
    /// The file in [`PATCHES_DIR`], by name.
    pub file: String,
    /// What it changes, in a line.
    pub what: String,
    /// Who or what wrote it.
    ///
    /// **Named rather than implied.** A patch written by a person and one produced by a model
    /// need different reading, and a bundle that did not distinguish them would make the
    /// careful reading the exception.
    pub proposed_by: String,
    /// How the behaviour it implements is known.
    pub known: Oracle,
    /// What was observed that motivates it.
    pub evidence: String,
    /// Claims it rests on that nothing measured.
    #[serde(default)]
    pub assumes: Vec<String>,
}

impl Proposal {
    /// Whether this may be merged without somebody vouching for where the behaviour came from.
    ///
    /// **Never true for `assumed`, and that is the whole point.** An assumption written down
    /// can be counted, ranked, probed and retired; one merged as though it were a fact never
    /// will be.
    #[must_use]
    pub fn is_promotable(&self) -> bool {
        !matches!(self.known, Oracle::Assumed)
    }
}

/// Everything one machine has to contribute.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Bundle {
    /// What the loop measured.
    pub learned: Learned,
    /// How far each title got.
    pub results: Results,
    /// Source changes being proposed, and what each rests on.
    ///
    /// **Not claims, and reported as not claims.** A measurement is checked by re-deriving
    /// it; a patch is checked by reading it and running the gate. Listing the two together
    /// without saying which is which would let a diff inherit the trust the measurements
    /// earned (D322).
    pub proposals: Vec<Proposal>,
    /// Who produced it and when.
    pub manifest: Manifest,
}

/// The proposals file, as it is written and read.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Proposals {
    /// One per patch file.
    #[serde(default, rename = "proposal")]
    pub proposal: Vec<Proposal>,
}

impl Proposals {
    /// Reads the file that says what each patch rests on.
    ///
    /// Here rather than in a shim so the format has one reader. Two would drift, and this one
    /// carries the provenance field - the last place a drift should be possible.
    ///
    /// # Errors
    ///
    /// When the text is not a proposals file.
    pub fn parse(text: &str) -> Result<Self, String> {
        toml::from_str(text).map_err(|e| e.to_string())
    }

    /// The file as text, for writing it back.
    ///
    /// Beside [`parse`](Self::parse) so the format has one reader and one writer. A shim that
    /// serialised this itself would be the second definition, and the field it would drift on
    /// is the provenance one (principle 13).
    ///
    /// # Errors
    ///
    /// When the proposals cannot be serialised.
    pub fn to_toml(&self) -> Result<String, String> {
        toml::to_string_pretty(self).map_err(|e| e.to_string())
    }
}

/// Title results, in the shape they are written and read.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Results {
    /// What the emulator does as it stands, by title.
    #[serde(default)]
    pub status: BTreeMap<String, Status>,
    /// The furthest each title got while being helped along, by title.
    ///
    /// **Kept apart rather than merged.** The two answer different questions and one
    /// best-ever entry cannot hold both without silently overwriting the other (D312).
    #[serde(default)]
    pub experiment: BTreeMap<String, Status>,
}

/// Who produced a bundle, and what is in it.
///
/// **The build is the load-bearing field.** A claim that cannot name the tree it came from
/// is not checkable, and every run report this project produced for months said `unknown`
/// because nothing ever set it. A submission is where that costs the most.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    /// The build that produced it.
    pub by: String,
    /// The day it was gathered.
    pub on: String,
    /// How many measurements it carries.
    #[serde(default)]
    pub measurements: usize,
    /// How many title results it carries.
    #[serde(default)]
    pub titles: usize,
    /// How many source changes it proposes.
    ///
    /// Counted apart from the claims, because it is the number that decides whether a
    /// receiver has reading to do rather than a command to run.
    #[serde(default)]
    pub proposals: usize,
}

impl Bundle {
    /// Gathers what a machine holds into one bundle.
    ///
    /// Takes the parts rather than reading them, so the shape of a submission is testable
    /// without a data directory, a checkout or a title - the same separation
    /// `orbistoun-mem` makes between deciding and doing (principle 8).
    #[must_use]
    pub fn gather(learned: Learned, results: Results, by: String, on: String) -> Self {
        let manifest = Manifest {
            by,
            on,
            measurements: learned.measurements.len(),
            titles: results.status.len() + results.experiment.len(),
            proposals: 0,
        };
        Self {
            learned,
            results,
            proposals: Vec::new(),
            manifest,
        }
    }

    /// Attaches source changes being proposed.
    ///
    /// Separate from [`gather`](Self::gather) because they are a separate kind of thing, and
    /// a bundle that carried none is the ordinary case.
    #[must_use]
    pub fn proposing(mut self, proposals: Vec<Proposal>) -> Self {
        self.manifest.proposals = proposals.len();
        self.proposals = proposals;
        self
    }

    /// Proposals nobody may merge without vouching for where the behaviour came from.
    ///
    /// **The list a reviewer starts from.** Everything else in a bundle is checked by
    /// re-deriving it; these cannot be, so the only useful thing to say about them is which
    /// ones carry no oracle better than a guess (D322).
    #[must_use]
    pub fn needing_a_voucher(&self) -> Vec<&Proposal> {
        self.proposals
            .iter()
            .filter(|p| !p.is_promotable())
            .collect()
    }

    /// What the bundle actually carries: measurements, then title results.
    ///
    /// **Counted from the contents, never read off the manifest.** A manifest is a claim by
    /// whoever sent it, and a receiver that quotes it back is reporting the sender's
    /// arithmetic as its own measurement. Found by editing a bundle by hand and watching
    /// `submit check` announce the old totals (D315).
    #[must_use]
    pub fn counts(&self) -> (usize, usize) {
        (
            self.learned.measurements.len(),
            self.results.status.len() + self.results.experiment.len(),
        )
    }

    /// Whether the manifest describes what is actually here.
    ///
    /// Not proof of tampering and not treated as such - a hand-edited bundle is the ordinary
    /// cause. It means the manifest cannot be quoted, which is worth one line of output.
    #[must_use]
    pub fn manifest_matches_contents(&self) -> bool {
        let (measurements, titles) = self.counts();
        self.manifest.measurements == measurements
            && self.manifest.titles == titles
            && self.manifest.proposals == self.proposals.len()
    }

    /// Whether there is anything worth sending.
    ///
    /// **Asked before writing, because an empty bundle is worse than no bundle.** It reads
    /// as "this machine found nothing" when it usually means the loop was never turned, and
    /// those are different facts - the distinction `known_by` exists to hold, one level up.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.learned.measurements.is_empty()
            && self.results.status.is_empty()
            && self.results.experiment.is_empty()
    }

    /// The files a bundle is written as, ready for a caller to put on disk.
    ///
    /// Returned rather than written for the reason [`gather`](Self::gather) takes its parts:
    /// producing the content is a judgement about what a submission carries, writing it is a
    /// decision about a machine, and principle 13 puts the second in a shim.
    ///
    /// # Errors
    ///
    /// When any part cannot be serialised.
    pub fn to_files(&self) -> Result<Vec<(&'static str, String)>, String> {
        let manifest = toml::to_string_pretty(&self.manifest).map_err(|e| e.to_string())?;
        let learned = self.learned.to_toml()?;
        let results = toml::to_string_pretty(&self.results).map_err(|e| e.to_string())?;
        let mut files = vec![
            (MANIFEST_FILE, manifest),
            (LEARNED_FILE, learned),
            (RESULTS_FILE, results),
        ];
        // Written only when there are any. A bundle full of empty files makes a reader look
        // for something that is not there, and `patches.toml` with nothing under it reads as
        // a proposal that failed to serialise rather than as a machine with none.
        if !self.proposals.is_empty() {
            let proposals = Proposals {
                proposal: self.proposals.clone(),
            };
            files.push((
                PROPOSALS_FILE,
                toml::to_string_pretty(&proposals).map_err(|e| e.to_string())?,
            ));
        }
        Ok(files)
    }

    /// Reads a bundle back from its files.
    ///
    /// # Errors
    ///
    /// When any part cannot be parsed. Deliberately not lenient: a submission that half
    /// parsed would be accepted as a smaller submission, which is a silent loss of somebody
    /// else's work.
    /// `proposals` is the text of [`PROPOSALS_FILE`], or `None` where the bundle carries no
    /// patches - which is the ordinary case and not an error.
    pub fn from_files(
        manifest: &str,
        learned: &str,
        results: &str,
        proposals: Option<&str>,
    ) -> Result<Self, String> {
        let proposals: Proposals = match proposals {
            Some(text) => toml::from_str(text).map_err(|e| format!("proposals: {e}"))?,
            None => Proposals::default(),
        };
        Ok(Self {
            manifest: toml::from_str(manifest).map_err(|e| format!("manifest: {e}"))?,
            learned: toml::from_str(learned).map_err(|e| format!("measurements: {e}"))?,
            results: toml::from_str(results).map_err(|e| format!("results: {e}"))?,
            proposals: proposals.proposal,
        })
    }

    /// How a submission and what this machine found differ.
    ///
    /// **Checked by re-deriving, never by trusting.** Agreement is silence; every difference
    /// is named, and "we never measured that" is reported as its own kind rather than as a
    /// contradiction.
    #[must_use]
    pub fn disagreements(&self, theirs: &Self) -> Vec<Disagreement> {
        let mut out: Vec<Disagreement> = self
            .learned
            .disagreements(&theirs.learned)
            .into_iter()
            .map(Disagreement::Measurement)
            .collect();

        for (title, claimed) in &theirs.results.status {
            match self.results.status.get(title) {
                None => out.push(Disagreement::TitleNotRunHere {
                    title: title.clone(),
                }),
                // **Only the reach ladder is compared.** Imports and calls vary between two
                // machines running the same title - a different time limit, a different
                // wall-clock - and reporting that as a contradiction would make every
                // honest submission look like a dispute.
                Some(ours) if ours.reach != claimed.reach => {
                    out.push(Disagreement::ReachDiffers {
                        title: title.clone(),
                        here: ours.reach.label().to_owned(),
                        there: claimed.reach.label().to_owned(),
                    });
                }
                Some(_) => {}
            }
        }
        out
    }
}

/// One way a submission fails to match what this machine found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Disagreement {
    /// A measurement this machine reads differently, or has not made.
    Measurement(orbistoun_hle::learned::Disagreement),
    /// This machine has never run that title.
    ///
    /// **Not a refutation**, and the common case for a submission worth having: the whole
    /// argument for accepting these is that other people hold titles this one does not.
    TitleNotRunHere {
        /// The title the submission names.
        title: String,
    },
    /// Both ran it and they disagree about how far it got.
    ReachDiffers {
        /// The title.
        title: String,
        /// What this machine reached.
        here: String,
        /// What the submission claims.
        there: String,
    },
}

impl Disagreement {
    /// One line a person reads.
    #[must_use]
    pub fn say(&self) -> String {
        use orbistoun_hle::learned::Disagreement as Measured;

        match self {
            Self::Measurement(Measured::NotMeasuredHere { function }) => {
                format!("{function}: not measured here - accept as assumed, or run the title")
            }
            Self::Measurement(Measured::Differs {
                function,
                here,
                there,
            }) => format!("{function}: here {here}, submitted {there}"),
            Self::TitleNotRunHere { title } => {
                format!("{title}: never run here - accept as assumed, or obtain the title")
            }
            Self::ReachDiffers { title, here, there } => {
                format!("{title}: here {here}, submitted {there}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Bundle, Disagreement, Results};
    use orbistoun_hle::knowledge::Oracle;
    use orbistoun_hle::learned::{Evidence, Learned, Measurement};
    use orbistoun_hle::{Delivery, StubRegion, StubReturn};
    use orbistoun_overrides::{Reach, Status};

    fn measurement(function: &str) -> Measurement {
        Measurement {
            function: function.to_owned(),
            library: "libkernel".to_owned(),
            measured: "PPSA02664-app0".to_owned(),
            on: "2026-08-27".to_owned(),
            by: "orbistoun 0.1.0 (abc1234)".to_owned(),
            known: Oracle::GuestObserved,
            evidence: Evidence::ConformanceCheck,
            answers: Some(StubReturn::Ok),
            region: Some(StubRegion {
                via: Delivery::Argument(0),
                bytes: 0x20_0000,
            }),
            assumes: vec!["the size is a guess".to_owned()],
        }
    }

    fn status(reach: Reach, imports: usize) -> Status {
        Status {
            reach,
            outcome: "image+0xafc959".to_owned(),
            imports,
            calls: 222,
            standing: 96,
            default_return: "unimplemented".to_owned(),
            overrides: 0,
            limit_seconds: Some(12),
            build: "0.1.0".to_owned(),
            measured_on: "2026-08-27".to_owned(),
            notes: String::new(),
        }
    }

    fn bundle(functions: &[&str], titles: &[(&str, Reach)]) -> Bundle {
        let mut learned = Learned::default();
        for function in functions {
            learned.record(measurement(function));
        }
        let mut results = Results::default();
        for (title, reach) in titles {
            results
                .status
                .insert((*title).to_owned(), status(*reach, 23));
        }
        Bundle::gather(
            learned,
            results,
            "orbistoun 0.1.0 (abc1234)".to_owned(),
            "2026-08-27".to_owned(),
        )
    }

    /// Pulls one file out of what a bundle serialises to.
    fn file(sent: &Bundle, name: &str) -> String {
        sent.to_files()
            .expect("serialises")
            .into_iter()
            .find(|(n, _)| *n == name)
            .map(|(_, text)| text)
            .expect("every file is present")
    }

    /// **What leaves one machine is what arrives at another**, assumptions included.
    ///
    /// The property the whole crate rests on. A claim that arrives stronger than it left,
    /// having quietly shed what it rests on, is worse than one that does not arrive.
    #[test]
    fn a_bundle_survives_a_round_trip_with_its_assumptions() {
        let sent = bundle(
            &["sceKernelReserveVirtualRange"],
            &[("PPSA02664", Reach::Entered)],
        );
        let arrived = Bundle::from_files(
            &file(&sent, super::MANIFEST_FILE),
            &file(&sent, super::LEARNED_FILE),
            &file(&sent, super::RESULTS_FILE),
            None,
        )
        .expect("parses");

        assert_eq!(arrived, sent);
        assert!(
            !arrived.learned.measurements[0].assumes.is_empty(),
            "assumptions must survive, or the claim arrives stronger than it left"
        );
    }

    /// The manifest counts what is there rather than what a caller claims.
    #[test]
    fn the_manifest_is_derived_from_the_contents() {
        let one = bundle(&["sceFoo"], &[("PPSA02664", Reach::Entered)]);

        assert_eq!(one.manifest.measurements, 1);
        assert_eq!(one.manifest.titles, 1);
        assert!(!one.manifest.by.is_empty(), "a claim has to name its build");
    }

    /// **A receiver counts the contents rather than quoting the manifest.**
    ///
    /// A manifest is a claim by whoever sent it. `submit check` announced six title results
    /// from a bundle carrying seven, because it read the number instead of the files - the
    /// same failure as every other one in this log, arriving in the newest code (D315).
    #[test]
    fn the_counts_come_from_the_contents_not_the_manifest() {
        let mut sent = bundle(&["sceFoo"], &[("PPSA02664", Reach::Entered)]);
        assert!(sent.manifest_matches_contents());

        sent.results
            .status
            .insert("PPSA99999".to_owned(), status(Reach::Linked, 5));

        assert_eq!(sent.counts(), (1, 2));
        assert_eq!(sent.manifest.titles, 1, "the manifest is now stale");
        assert!(
            !sent.manifest_matches_contents(),
            "and the receiver has to be able to notice"
        );
    }

    /// One proposal, as a patch directory would describe it.
    fn proposal(file: &str, known: Oracle) -> super::Proposal {
        super::Proposal {
            file: file.to_owned(),
            what: "implement sceKernelReserveVirtualRange".to_owned(),
            proposed_by: "a model".to_owned(),
            known,
            evidence: "the wall at image+0xafc959 moved".to_owned(),
            assumes: vec!["0x200000 bytes is a guess".to_owned()],
        }
    }

    /// **A guess is never merged on its own authority, however good the diff looks.**
    ///
    /// The promotion ladder is the verification step that makes generating a patch safe at
    /// all: it arrives inert, and becomes a change when somebody who can say where the
    /// behaviour came from merges it. `assumed` means nobody can say that yet (D322).
    #[test]
    fn a_proposal_resting_on_a_guess_needs_somebody_to_vouch_for_it() {
        let guessed = proposal("a.patch", Oracle::Assumed);
        let watched = proposal("b.patch", Oracle::GuestObserved);

        assert!(!guessed.is_promotable());
        assert!(watched.is_promotable());

        let bundle = bundle(&[], &[]).proposing(vec![guessed, watched]);
        let vouching = bundle.needing_a_voucher();
        assert_eq!(vouching.len(), 1);
        assert_eq!(vouching[0].file, "a.patch");
    }

    /// **Proposals survive the trip with their provenance and their assumptions.**
    ///
    /// The fields that decide whether a patch may be merged are exactly the fields a
    /// careless round trip would drop, leaving a diff that looks measured.
    #[test]
    fn a_proposal_arrives_with_what_it_rests_on() {
        let sent = bundle(&["sceFoo"], &[]).proposing(vec![proposal("a.patch", Oracle::Assumed)]);
        let arrived = Bundle::from_files(
            &file(&sent, super::MANIFEST_FILE),
            &file(&sent, super::LEARNED_FILE),
            &file(&sent, super::RESULTS_FILE),
            Some(&file(&sent, super::PROPOSALS_FILE)),
        )
        .expect("parses");

        assert_eq!(arrived.proposals, sent.proposals);
        assert!(!arrived.proposals[0].is_promotable());
        assert!(
            !arrived.proposals[0].assumes.is_empty(),
            "an assumption that does not survive makes the patch look measured"
        );
    }

    /// A bundle carrying no patches writes no proposals file, and reads back the same.
    #[test]
    fn a_bundle_with_no_patches_carries_no_proposals_file() {
        let sent = bundle(&["sceFoo"], &[]);

        assert!(
            !sent
                .to_files()
                .expect("serialises")
                .iter()
                .any(|(name, _)| *name == super::PROPOSALS_FILE),
            "an empty proposals file reads as one that failed to serialise"
        );
    }

    /// **An empty bundle is a different fact from a machine that found nothing.**
    #[test]
    fn a_bundle_with_nothing_in_it_says_so() {
        assert!(Bundle::default().is_empty());
        assert!(!bundle(&["sceFoo"], &[]).is_empty());
        assert!(!bundle(&[], &[("PPSA02664", Reach::Entered)]).is_empty());
    }

    /// **A submission is checked by re-deriving it, and agreement is silence.**
    #[test]
    fn an_agreeing_submission_produces_no_complaints() {
        let here = bundle(&["sceFoo"], &[("PPSA02664", Reach::Entered)]);
        let theirs = bundle(&["sceFoo"], &[("PPSA02664", Reach::Entered)]);

        assert!(here.disagreements(&theirs).is_empty());
    }

    /// **"We never looked" is not "it is wrong", and this is the common case.**
    ///
    /// The entire argument for accepting these is that other people hold titles this machine
    /// does not. If that arrived as a contradiction, every submission worth having would
    /// look like a dispute.
    #[test]
    fn a_title_this_machine_never_ran_is_not_a_refutation() {
        let here = bundle(&[], &[]);
        let theirs = bundle(&["sceFoo"], &[("PPSA99999", Reach::Entered)]);

        let said = here.disagreements(&theirs);
        assert!(
            said.iter()
                .any(|d| matches!(d, Disagreement::TitleNotRunHere { .. })),
            "{said:?}"
        );
        assert!(
            said.iter().all(|d| d.say().contains("assumed")),
            "each should say what to do with it: {:?}",
            said.iter().map(Disagreement::say).collect::<Vec<_>>()
        );
    }

    /// A real contradiction about the same title is named as one.
    #[test]
    fn two_machines_disagreeing_about_a_title_is_reported() {
        let here = bundle(&[], &[("PPSA02664", Reach::Entered)]);
        let theirs = bundle(&[], &[("PPSA02664", Reach::Rejected)]);

        assert!(matches!(
            here.disagreements(&theirs).as_slice(),
            [Disagreement::ReachDiffers { .. }]
        ));
    }

    /// **Counts differing is not a disagreement.**
    ///
    /// Two machines running one title report different import and call totals - a different
    /// time limit, a different wall-clock. Comparing those would make every honest
    /// submission look like a dispute, which is how a check stops being read.
    #[test]
    fn differing_counts_on_the_same_reach_are_not_a_disagreement() {
        let mut here = bundle(&[], &[]);
        here.results
            .status
            .insert("PPSA02664".to_owned(), status(Reach::Entered, 23));
        let mut theirs = bundle(&[], &[]);
        theirs
            .results
            .status
            .insert("PPSA02664".to_owned(), status(Reach::Entered, 40));

        assert!(here.disagreements(&theirs).is_empty());
    }

    /// The two slots are carried apart, so an experiment cannot arrive as a fact.
    #[test]
    fn a_helped_result_arrives_in_its_own_slot() {
        let mut results = Results::default();
        results
            .status
            .insert("PPSA02664".to_owned(), status(Reach::Entered, 23));
        let helped = Status {
            overrides: 1,
            ..status(Reach::Entered, 40)
        };
        results
            .experiment
            .insert("PPSA02664".to_owned(), helped.clone());

        let sent = Bundle::gather(
            Learned::default(),
            results,
            "orbistoun 0.1.0 (abc1234)".to_owned(),
            "2026-08-27".to_owned(),
        );
        let back: Results =
            toml::from_str(&file(&sent, super::RESULTS_FILE)).expect("results parse");

        assert_eq!(back.status["PPSA02664"].overrides, 0);
        assert_eq!(back.experiment["PPSA02664"], helped);
        assert_eq!(
            sent.manifest.titles, 2,
            "both slots are results the machine has to contribute"
        );
    }

    /// A bundle that half parsed would silently lose somebody else's work.
    #[test]
    fn a_malformed_part_is_an_error_rather_than_a_smaller_bundle() {
        let error =
            Bundle::from_files("by = 1", "", "", None).expect_err("a number is not a build");

        assert!(error.starts_with("manifest:"), "{error}");
    }
}
