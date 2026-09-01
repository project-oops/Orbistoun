//! The provenance accounting, exercised through its public face.
//!
//! # Why these are here rather than beside the code
//!
//! `knowledge.rs` is the machinery principle 1 rests on: every recorded behaviour carries a
//! `known_by`, every claim of outside support has to cite something, and CI refuses a tree
//! that breaks either rule. It was at **15% coverage** - the lowest of any pure module in
//! the workspace - which meant most of the rules it enforces had never been watched enforce
//! anything.
//!
//! That is the shape `CLAUDE.md` calls out directly: *"a guard is not finished until
//! somebody has made it fail. A guard nobody has watched reject something is a guard nobody
//! knows anything about."* Every test below makes one of them reject something.
//!
//! # Built from values, never from the shipped files
//!
//! Nothing here asserts on `Knowledge::builtin()` being clean. The shipped files change as
//! functions are implemented, so a test that read them would fail for reasons belonging to
//! whoever last edited a TOML - and would then be *disabled*, which is worse than not having
//! it. The rules are tested against constructed entries; the shipped files are checked
//! separately, by the guard that exists for them.

use orbistoun_hle::knowledge::{FunctionKnowledge, Knowledge, KnowledgeFile, Oracle};

/// An entry that records nothing beyond having seen the function.
fn bare(name: &str) -> FunctionKnowledge {
    FunctionKnowledge {
        name: name.to_owned(),
        ..FunctionKnowledge::default()
    }
}

/// An entry that claims something about behaviour, so provenance has to account for it.
fn claiming(name: &str, known: Oracle) -> FunctionKnowledge {
    FunctionKnowledge {
        name: name.to_owned(),
        arity: Some(2),
        purpose: "does a thing".to_owned(),
        known_by: Some(known),
        ..FunctionKnowledge::default()
    }
}

/// **A bare entry needs no source, and one that claims behaviour does.**
///
/// The distinction the whole accounting rests on: recording that a function exists is not a
/// claim about what it does, and demanding a citation for it would make the rule noise that
/// people learn to silence.
#[test]
fn only_an_entry_claiming_behaviour_needs_to_say_how_it_is_known() {
    let seen = bare("sceSomethingSeen");
    assert!(seen.is_bare());
    assert!(!seen.claims_behaviour());
    assert!(
        seen.provenance_faults().is_empty(),
        "seeing a function is not a claim about it"
    );

    let mut claims = bare("sceSomethingClaimed");
    claims.purpose = "does a thing".to_owned();
    assert!(!claims.is_bare());
    assert!(claims.claims_behaviour());
    let faults = claims.provenance_faults();
    assert_eq!(faults.len(), 1, "{faults:?}");
    assert!(
        faults[0].contains("does not say how it is known"),
        "{faults:?}"
    );
}

/// Any of the four ways of being non-bare is enough to require provenance.
///
/// Checked one at a time because `is_bare` is a conjunction, and a conjunction with one
/// clause wrong still reads correctly for every input that trips a different clause.
#[test]
fn each_kind_of_content_on_its_own_makes_an_entry_non_bare() {
    let with_arity = FunctionKnowledge {
        arity: Some(0),
        ..bare("a")
    };
    let with_purpose = FunctionKnowledge {
        purpose: "p".to_owned(),
        ..bare("b")
    };
    let with_arguments = FunctionKnowledge {
        arguments: vec![orbistoun_hle::knowledge::Argument::default()],
        ..bare("c")
    };
    let with_edges = FunctionKnowledge {
        edge_cases: vec!["e".to_owned()],
        ..bare("d")
    };

    for entry in [with_arity, with_purpose, with_arguments, with_edges] {
        assert!(!entry.is_bare(), "{} read as bare", entry.name);
    }
}

/// **A claim of outside support with nothing to check is refused.**
///
/// `published` and `measured` both assert something a reader could go and verify. Without a
/// citation there is nothing to verify, and the entry *looks* like evidence while being
/// worth less than an honest `assumed`.
#[test]
fn claiming_an_outside_source_without_citing_one_is_a_fault() {
    for oracle in [Oracle::Published, Oracle::Measured] {
        assert!(oracle.needs_citation(), "{oracle:?}");

        let faults = claiming("sceUncited", oracle).provenance_faults();
        assert_eq!(faults.len(), 1, "{oracle:?}: {faults:?}");
        assert!(
            faults[0].contains("claims an outside source but cites none"),
            "{faults:?}"
        );

        let cited = FunctionKnowledge {
            cites: "ISO C 7.21.6.5".to_owned(),
            ..claiming("sceCited", oracle)
        };
        assert!(
            cited.provenance_faults().is_empty(),
            "a named document settles it: {:?}",
            cited.provenance_faults()
        );
    }
}

/// **A path is not a citation, in every shape a path arrives in.**
///
/// A citation exists so somebody else can check a claim, and a location only one machine has
/// defeats that entirely. Each form is checked because the test is a disjunction, and one
/// missing arm passes for every input that trips another (D239).
#[test]
fn a_citation_naming_a_location_rather_than_a_document_is_refused() {
    for path in [
        "D:\\scratch\\relay.txt",
        "D:/scratch/relay.txt",
        "/home/someone/notes",
        "./relative.txt",
        "../up-one.txt",
    ] {
        let entry = FunctionKnowledge {
            cites: path.to_owned(),
            ..claiming("scePathCited", Oracle::Published)
        };
        let faults = entry.provenance_faults();
        assert!(
            faults.iter().any(|f| f.contains("must name a document")),
            "{path} was accepted as a citation: {faults:?}"
        );
    }
}

/// A document reference that merely contains a dot is still a document.
///
/// The converse of the test above, and the one that stops the path check being tightened
/// into something that rejects ordinary citations.
#[test]
fn an_ordinary_document_reference_is_not_mistaken_for_a_path() {
    for citation in [
        "ISO C 7.21.6.5",
        "POSIX.1-2017 fcntl",
        "FreeBSD 14.0 sys/kern/kern_descrip.c",
    ] {
        let entry = FunctionKnowledge {
            cites: citation.to_owned(),
            ..claiming("sceProperlyCited", Oracle::Published)
        };
        assert!(
            entry.provenance_faults().is_empty(),
            "{citation} was refused: {:?}",
            entry.provenance_faults()
        );
    }
}

/// **Citing a source for a guess is refused, because it reads as evidence.**
///
/// `assumed` means nobody established it. An entry that says so *and* points at a document
/// is the precise confusion the field exists to prevent.
#[test]
fn an_assumption_that_cites_something_is_a_fault() {
    assert!(Oracle::Assumed.is_guess());

    let entry = FunctionKnowledge {
        cites: "ISO C 7.21.6.5".to_owned(),
        ..claiming("sceGuessWithCitation", Oracle::Assumed)
    };
    let faults = entry.provenance_faults();
    assert!(
        faults.iter().any(|f| f.contains("nothing to cite")),
        "{faults:?}"
    );

    let honest = claiming("sceHonestGuess", Oracle::Assumed);
    assert!(
        honest.provenance_faults().is_empty(),
        "an uncited assumption is the honest case, not a fault"
    );
}

/// A `found_by` outside the vocabulary is named, and stops further name checking.
#[test]
fn an_unknown_found_by_label_is_refused_by_name() {
    let entry = FunctionKnowledge {
        found_by: "vibes".to_owned(),
        ..claiming("sceInventedProvenance", Oracle::Assumed)
    };
    let faults = entry.name_provenance_faults();
    assert_eq!(faults.len(), 1, "{faults:?}");
    assert!(faults[0].contains("is not one of"), "{faults:?}");
}

/// An empty `found_by` says nothing about the name, which is not a fault.
#[test]
fn saying_nothing_about_where_a_name_came_from_is_not_a_claim() {
    let entry = claiming("sceNoNameClaim", Oracle::Assumed);
    assert!(entry.found_by.is_empty());
    assert!(entry.name_provenance_faults().is_empty());
}

/// **An entry that itemises its guesses is not charged again for being candid.**
///
/// Two definitions of this count once disagreed by ten across the same knowledge base, and
/// neither report said which it meant. The rule kept was: itemised assumptions count as
/// themselves, and an entry resting on a guess while listing nothing counts as one (D239).
#[test]
fn open_questions_count_the_items_and_never_add_a_penalty_on_top() {
    let itemised = FunctionKnowledge {
        assumptions: vec!["one".to_owned(), "two".to_owned(), "three".to_owned()],
        ..claiming("sceCandid", Oracle::Assumed)
    };
    assert_eq!(
        itemised.open_questions(),
        3,
        "three items is three questions, not four"
    );
    assert_eq!(itemised.open_questions_asked(), vec!["one", "two", "three"]);

    let unitemised = claiming("sceVague", Oracle::Assumed);
    assert_eq!(
        unitemised.open_questions(),
        1,
        "resting on a guess and saying nothing is still one question"
    );

    let established = claiming("sceEstablished", Oracle::Measured);
    assert_eq!(
        established.open_questions(),
        0,
        "something measured asks nothing"
    );
}

/// The count is the length of the list, so the two cannot disagree.
#[test]
fn the_question_count_is_derived_from_the_question_list() {
    for entry in [
        claiming("a", Oracle::Assumed),
        claiming("b", Oracle::Measured),
        FunctionKnowledge {
            assumptions: vec!["x".to_owned(), "y".to_owned()],
            ..claiming("c", Oracle::GuestObserved)
        },
    ] {
        assert_eq!(entry.open_questions(), entry.open_questions_asked().len());
    }
}

/// **The vocabulary has no value meaning "I already knew it", and each says what it is.**
///
/// The property that makes the field enforcement rather than decoration: every option names
/// something that could contradict it.
#[test]
fn every_oracle_is_falsifiable_and_labelled() {
    let all = [
        Oracle::Published,
        Oracle::Measured,
        Oracle::GuestObserved,
        Oracle::Assumed,
    ];
    let mut labels = std::collections::BTreeSet::new();
    for oracle in all {
        assert!(labels.insert(oracle.label()), "{oracle:?} shares a label");
        assert!(!oracle.label().is_empty());
    }

    // Only the two claiming outside support need a citation; the two resting on our own
    // observation do not, because the observation is the evidence.
    assert!(Oracle::Published.needs_citation());
    assert!(Oracle::Measured.needs_citation());
    assert!(!Oracle::GuestObserved.needs_citation());
    assert!(!Oracle::Assumed.needs_citation());

    // Only what nobody has settled, or settled with one bit, is worth pointing hardware at.
    assert!(Oracle::Assumed.is_probeable());
    assert!(Oracle::GuestObserved.is_probeable());
    assert!(!Oracle::Published.is_probeable());
    assert!(!Oracle::Measured.is_probeable());
}

/// A file survives being written and read back.
#[test]
fn a_knowledge_file_round_trips_through_its_own_format() {
    let file = KnowledgeFile {
        library: "libSceExample".to_owned(),
        functions: vec![FunctionKnowledge {
            cites: "ISO C 7.21.6.5".to_owned(),
            assumptions: vec!["the size is a guess".to_owned()],
            edge_cases: vec!["refuses a null destination".to_owned()],
            found_on: "2026-08-27".to_owned(),
            ..claiming("sceExample", Oracle::Published)
        }],
    };

    let text = file.render().expect("renders");
    let back = KnowledgeFile::parse(&text).expect("parses");

    assert_eq!(back.library, file.library);
    assert_eq!(back.functions, file.functions);
    assert!(
        !back.functions[0].assumptions.is_empty(),
        "assumptions must survive, or a claim arrives stronger than it left"
    );
}

/// Malformed text is refused rather than read as an empty file.
///
/// An empty fall-back would look exactly like a library nobody has learned anything about,
/// which is the failure `Container::imports` is written to avoid one layer down.
#[test]
fn a_malformed_file_is_an_error_rather_than_an_empty_one() {
    assert!(KnowledgeFile::parse("this is not toml {{{").is_err());
    assert!(KnowledgeFile::parse("library = 12").is_err());
}

/// Absorbing a file makes its functions findable, and says which library they came from.
#[test]
fn an_absorbed_file_is_searchable_by_function_and_by_library() {
    let mut knowledge = Knowledge::default();
    assert!(knowledge.is_empty());
    assert_eq!(knowledge.len(), 0);

    let file = KnowledgeFile {
        library: "libSceExample".to_owned(),
        functions: vec![
            claiming("sceFirst", Oracle::GuestObserved),
            bare("sceSecond"),
        ],
    };
    knowledge.absorb("libSceExample", &file);

    assert_eq!(knowledge.len(), 2);
    assert!(!knowledge.is_empty());
    assert_eq!(
        knowledge.get("sceFirst").map(|f| f.name.as_str()),
        Some("sceFirst")
    );
    assert_eq!(knowledge.library_of("sceSecond"), Some("libSceExample"));
    assert!(
        knowledge.get("sceNeverHeardOf").is_none(),
        "a function nobody recorded is absent, not defaulted"
    );
    assert!(knowledge.library_of("sceNeverHeardOf").is_none());
}

/// **The tallies count what they say they count.**
///
/// `understood` counts entries that record something; `resting_on` counts by oracle; and
/// `open_questions` sums the itemised guesses. Built from a known set so each number has a
/// hand-checkable answer rather than being whatever the shipped files happen to hold.
#[test]
fn the_tallies_are_each_computed_over_the_right_thing() {
    let mut knowledge = Knowledge::default();
    knowledge.absorb(
        "libSceExample",
        &KnowledgeFile {
            library: "libSceExample".to_owned(),
            functions: vec![
                bare("sceOnlySeen"),
                claiming("sceGuessed", Oracle::Assumed),
                FunctionKnowledge {
                    assumptions: vec!["one".to_owned(), "two".to_owned()],
                    ..claiming("sceWatched", Oracle::GuestObserved)
                },
                FunctionKnowledge {
                    cites: "ISO C 7.21.6.5".to_owned(),
                    ..claiming("sceRead", Oracle::Published)
                },
            ],
        },
    );

    assert_eq!(knowledge.len(), 4);
    assert_eq!(
        knowledge.understood(),
        3,
        "the bare entry records nothing beyond having been seen"
    );
    assert_eq!(knowledge.resting_on(Oracle::Assumed), 1);
    assert_eq!(knowledge.resting_on(Oracle::GuestObserved), 1);
    assert_eq!(knowledge.resting_on(Oracle::Published), 1);
    assert_eq!(knowledge.resting_on(Oracle::Measured), 0);
    assert_eq!(
        knowledge.open_questions(),
        3,
        "one for the unitemised guess, two for the itemised ones"
    );
    assert_eq!(knowledge.functions().count(), 4);
}

/// **A fault in one entry is reported against the whole base, and names the function.**
///
/// The property CI depends on: a report that said only "something is wrong" would send
/// somebody reading ninety-five entries.
#[test]
fn a_faulty_entry_is_reported_by_name_across_the_whole_base() {
    let mut knowledge = Knowledge::default();
    knowledge.absorb(
        "libSceExample",
        &KnowledgeFile {
            library: "libSceExample".to_owned(),
            functions: vec![
                claiming("sceFine", Oracle::Assumed),
                claiming("sceUncited", Oracle::Published),
            ],
        },
    );

    let faults = knowledge.provenance_faults();
    assert_eq!(faults.len(), 1, "{faults:?}");
    assert!(faults[0].contains("sceUncited"), "{faults:?}");
    assert!(
        !faults[0].contains("sceFine"),
        "the clean entry is not implicated: {faults:?}"
    );
}

/// The shipped knowledge parses and holds something.
///
/// **Deliberately not an assertion that it is clean.** The shipped files change as functions
/// are implemented, so a cleanliness assertion here would fail for reasons belonging to
/// whoever last edited a TOML - and a test that fails for somebody else's reason is one that
/// gets disabled. That check has its own home; this one only says the embedded files load.
#[test]
fn the_shipped_knowledge_loads_and_is_not_empty() {
    let builtin = Knowledge::builtin();
    assert!(!builtin.is_empty());
    assert!(builtin.len() > 20, "only {} entries loaded", builtin.len());
    assert_eq!(builtin.functions().count(), builtin.len());
}

/// A record carrying only what one session established.
fn recording(function: &str) -> orbistoun_hle::knowledge::Record {
    orbistoun_hle::knowledge::Record {
        function: function.to_owned(),
        ..orbistoun_hle::knowledge::Record::default()
    }
}

/// **A merge adds without restating, and never drops what came before.**
///
/// The property the whole accumulate-never-regenerate design rests on: a session recording
/// one edge case must not have to repeat what was established three sessions ago, and must
/// not silently lose it either.
#[test]
fn merging_a_record_keeps_what_earlier_sessions_established() {
    let mut file = KnowledgeFile {
        library: "libSceExample".to_owned(),
        functions: vec![FunctionKnowledge {
            edge_cases: vec!["refuses a null destination".to_owned()],
            found_in: vec!["PPSA02664-app0".to_owned()],
            ..claiming("sceGrowing", Oracle::GuestObserved)
        }],
    };

    let later = orbistoun_hle::knowledge::Record {
        edge_cases: vec!["truncates on a character boundary".to_owned()],
        found_in: vec!["obscene".to_owned()],
        ..recording("sceGrowing")
    };
    file.merge(&later, "2026-08-27");

    assert_eq!(file.functions.len(), 1, "one function, one entry");
    let entry = &file.functions[0];
    assert_eq!(entry.edge_cases.len(), 2, "{:?}", entry.edge_cases);
    assert!(entry.edge_cases.iter().any(|e| e.contains("null")));
    assert!(entry.edge_cases.iter().any(|e| e.contains("truncates")));
    assert_eq!(entry.found_in.len(), 2, "{:?}", entry.found_in);
    assert_eq!(
        entry.arity,
        Some(2),
        "a record carrying no arity leaves the established one alone"
    );
    assert_eq!(entry.purpose, "does a thing", "and the same for purpose");
}

/// **Recording the same thing twice does not record it twice.**
///
/// A loop that re-derives the same finding every run would otherwise grow a file without
/// bound, and a list of a hundred identical edge cases says less than one does.
#[test]
fn merging_the_same_finding_again_does_not_duplicate_it() {
    let mut file = KnowledgeFile {
        library: "libSceExample".to_owned(),
        functions: Vec::new(),
    };
    let record = orbistoun_hle::knowledge::Record {
        edge_cases: vec!["the same finding".to_owned()],
        found_in: vec!["obscene".to_owned()],
        assumptions: vec!["the same guess".to_owned()],
        known_by: Some(Oracle::GuestObserved),
        arity: Some(1),
        ..recording("sceRepeated")
    };

    file.merge(&record, "2026-08-27");
    file.merge(&record, "2026-08-28");

    let entry = &file.functions[0];
    assert_eq!(file.functions.len(), 1);
    assert_eq!(entry.edge_cases.len(), 1, "{:?}", entry.edge_cases);
    assert_eq!(entry.found_in.len(), 1, "{:?}", entry.found_in);
    assert_eq!(entry.assumptions.len(), 1, "{:?}", entry.assumptions);
    assert_eq!(
        entry.found_on, "2026-08-27",
        "the date records when it was first worked out, not when it was last touched"
    );
}

/// A record for a function nobody has recorded creates the entry, dated today.
#[test]
fn merging_an_unknown_function_creates_it_with_todays_date() {
    let mut file = KnowledgeFile {
        library: "libSceExample".to_owned(),
        functions: Vec::new(),
    };
    file.merge(
        &orbistoun_hle::knowledge::Record {
            arity: Some(3),
            purpose: Some("a new thing".to_owned()),
            known_by: Some(Oracle::Assumed),
            ..recording("sceBrandNew")
        },
        "2026-08-27",
    );

    let entry = &file.functions[0];
    assert_eq!(entry.name, "sceBrandNew");
    assert_eq!(entry.arity, Some(3));
    assert_eq!(entry.found_on, "2026-08-27");
}

/// **A merge reports faults rather than refusing, and an empty list means admissible.**
///
/// The distinction the doc comment insists on: only the caller knows whether this is a
/// command rejecting input or a loop declining to record, so the decision is not made here.
#[test]
fn a_merge_reports_what_is_wrong_instead_of_deciding_what_to_do_about_it() {
    let mut file = KnowledgeFile {
        library: "libSceExample".to_owned(),
        functions: Vec::new(),
    };

    let faults = file.merge(
        &orbistoun_hle::knowledge::Record {
            purpose: Some("claims something".to_owned()),
            known_by: Some(Oracle::Published),
            ..recording("sceUncitedMerge")
        },
        "2026-08-27",
    );
    assert!(
        faults.iter().any(|f| f.contains("cites none")),
        "{faults:?}"
    );
    assert_eq!(
        file.functions.len(),
        1,
        "and it is still recorded - reporting is not refusing"
    );

    let clean = file.merge(
        &orbistoun_hle::knowledge::Record {
            cites: Some("ISO C 7.21.6.5".to_owned()),
            ..recording("sceUncitedMerge")
        },
        "2026-08-27",
    );
    assert!(clean.is_empty(), "citing it settles it: {clean:?}");
}
