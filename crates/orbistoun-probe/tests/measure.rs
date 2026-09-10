//! Reading `measure` records, and the export table one section of them carries.
//!
//! # Why these are written against a hand-built transcript
//!
//! The real reports are 300KB each and live in the sibling project. Copying one in for this
//! would be copying evidence, not a specification - and a test that read a sibling checkout
//! fails for anyone without one, which is the coupling D207 exists to prevent. What is
//! pinned here is the *shape*: seven fields, one section name, a hash and an address.
//!
//! # What each of these is guarding against
//!
//! Every one asserts on a refusal or an absence, not on a count of passes. A reader that
//! decoded anything eleven characters long, or that grade-stamped a measurement as though
//! somebody had asserted a device, would pass any test that only counted what came back.

use orbistoun_hle::knowledge::Oracle;
use orbistoun_probe::{KEXPORT_SECTION, Origin, Transcript, export_aliases};

/// A transcript carrying measurements, including a small export table.
///
/// The two hashes at `0x8000006f0` are `getpeername` and `_getpeername` as a real console
/// spells them - one function under two names, which is the fact the alias grouping exists
/// to surface (D606).
fn transcript() -> String {
    [
        "OBS|build|test-build|payload",
        "OBS|measure|120-measure/cache-topology|cache|level|0x2|level",
        "OBS|measure|120-measure/cache-topology|cache|line|0x40|bytes",
        "OBS|measure|135-sysctl/names|kern.version|length|0x2c|bytes",
        &format!("OBS|measure|{KEXPORT_SECTION}|TXFFFiNldU8|vaddr|0x8000006f0|offset"),
        &format!("OBS|measure|{KEXPORT_SECTION}|rTNM7adVYog|vaddr|0x8000006f0|offset"),
        &format!("OBS|measure|{KEXPORT_SECTION}|+L22kkFiXok|vaddr|0x800000690|offset"),
    ]
    .join("\n")
}

fn read(text: &str) -> Transcript {
    Transcript::read(text).expect("the transcript must parse")
}

#[test]
fn measurements_are_read_rather_than_carried_uninterpreted() {
    let transcript = read(&transcript());
    let measurements = transcript.measurements(&Origin::unasserted());
    assert_eq!(
        measurements.len(),
        6,
        "every measure record, kexport included"
    );
    let cache = measurements
        .iter()
        .find(|m| m.field == "line")
        .expect("the cache line measurement");
    assert_eq!(cache.section, "120-measure/cache-topology");
    assert_eq!(cache.subject, "cache");
    assert_eq!(cache.unit, "bytes");
    assert_eq!(cache.number(), Some(0x40));
}

#[test]
fn a_value_that_is_not_a_number_comes_back_as_none_rather_than_zero() {
    // The failure this guards: a reader that parsed with `unwrap_or(0)` would turn "the
    // probe wrote a word here" into "the probe measured zero", and nothing downstream could
    // tell the two apart (principle 3).
    let text = [
        "OBS|build|test-build|payload",
        "OBS|measure|135-sysctl/names|kern.ostype|value|FreeBSD|text",
        "OBS|measure|135-sysctl/names|kern.maxproc|value|0x1e0|count",
        "OBS|measure|135-sysctl/names|kern.ncpu|value|8|count",
    ]
    .join("\n");
    let transcript = read(&text);
    let measurements = transcript.measurements(&Origin::unasserted());
    assert_eq!(measurements[0].number(), None, "a word is not a number");
    assert_eq!(measurements[1].number(), Some(0x1e0), "hex says so");
    assert_eq!(
        measurements[2].number(),
        Some(8),
        "and a bare 8 is eight, not fourteen"
    );
}

#[test]
fn the_section_tally_names_every_section_including_ones_nothing_interprets() {
    let transcript = read(&transcript());
    let sections = transcript.measured_sections();
    assert_eq!(sections.len(), 3);
    assert_eq!(sections.get("120-measure/cache-topology"), Some(&2));
    assert_eq!(sections.get("135-sysctl/names"), Some(&1));
    assert_eq!(sections.get(KEXPORT_SECTION), Some(&3));
}

#[test]
fn a_measurement_is_only_measured_when_the_operator_asserted_the_target() {
    // Same rule as every other fact: a number measured on a stand-in describes the stand-in
    // (D246). Asserting on the demotion, because the promotion is the direction that
    // corrupts a knowledge base.
    let transcript = read(&transcript());
    for measurement in transcript.measurements(&Origin::unasserted()) {
        assert_eq!(measurement.known_by, Oracle::Assumed);
    }
    let deck = Origin::asserted("steam-deck".to_owned(), String::new(), false);
    for measurement in transcript.measurements(&deck) {
        assert_eq!(measurement.known_by, Oracle::Assumed, "a Deck is not it");
    }
    let target = Origin::asserted("console".to_owned(), String::new(), true);
    for measurement in transcript.measurements(&target) {
        assert_eq!(measurement.known_by, Oracle::Measured);
    }
}

#[test]
fn the_export_table_decodes_to_the_hashes_a_name_search_can_use() {
    let transcript = read(&transcript());
    let exports = transcript.kernel_exports(&Origin::unasserted());
    assert_eq!(exports.len(), 3, "and nothing from the other sections");
    let first = &exports[0];
    assert_eq!(first.encoded, "TXFFFiNldU8");
    assert_eq!(first.vaddr, 0x8000_006f0);
    assert_eq!(
        first.nid,
        orbistoun_nid::NidHasher::default().hash("getpeername"),
        "the decoded hash must be the one hashing the name produces (D070)"
    );
}

#[test]
fn a_subject_that_is_not_an_encoded_hash_is_skipped_rather_than_decoded() {
    // The failure this guards: eleven characters of anything decode to a plausible hash, and
    // a plausible hash is the one error nothing downstream can notice. A section can carry a
    // record shaped differently - so the reader has to refuse, not salvage.
    let text = [
        "OBS|build|test-build|payload",
        &format!("OBS|measure|{KEXPORT_SECTION}|too-short|vaddr|0x800000690|offset"),
        &format!("OBS|measure|{KEXPORT_SECTION}|way-way-too-long-for-a-hash|vaddr|0x8|offset"),
        &format!("OBS|measure|{KEXPORT_SECTION}|not#in#alpha|vaddr|0x800000690|offset"),
        &format!("OBS|measure|{KEXPORT_SECTION}|+L22kkFiXok|vaddr|not-hex|offset"),
        &format!("OBS|measure|{KEXPORT_SECTION}|+L22kkFiXok|count|0x800000690|offset"),
        &format!("OBS|measure|{KEXPORT_SECTION}|+L22kkFiXok|vaddr|0x800000690|offset"),
    ]
    .join("\n");
    let transcript = read(&text);
    let exports = transcript.kernel_exports(&Origin::unasserted());
    assert_eq!(
        exports.len(),
        1,
        "only the well-formed record may survive, not five salvaged ones"
    );
    assert_eq!(exports[0].encoded, "+L22kkFiXok");
    // And the records that were skipped are still counted as measurements, because they
    // were measured - being unreadable by this decoder is not being absent.
    assert_eq!(
        transcript.measured_sections().get(KEXPORT_SECTION),
        Some(&6)
    );
}

#[test]
fn two_hashes_at_one_address_group_and_one_hash_does_not() {
    let transcript = read(&transcript());
    let exports = transcript.kernel_exports(&Origin::unasserted());
    let aliases = export_aliases(&exports);
    assert_eq!(aliases.len(), 1, "one address has two, the other has one");
    assert_eq!(aliases[0].vaddr, 0x8000_006f0);
    assert_eq!(aliases[0].exports.len(), 2);

    let hasher = orbistoun_nid::NidHasher::default();
    let spelled: Vec<orbistoun_nid::Nid> = aliases[0].exports.iter().map(|e| e.nid).collect();
    assert!(spelled.contains(&hasher.hash("getpeername")));
    assert!(spelled.contains(&hasher.hash("_getpeername")));
}

#[test]
fn a_table_with_no_repeated_address_yields_no_aliases() {
    // Asserting on the empty case. A grouping that returned every address would look
    // identical on a table where every address really is shared, which is not this one.
    let text = [
        "OBS|build|test-build|payload",
        &format!("OBS|measure|{KEXPORT_SECTION}|TXFFFiNldU8|vaddr|0x800000010|offset"),
        &format!("OBS|measure|{KEXPORT_SECTION}|rTNM7adVYog|vaddr|0x800000020|offset"),
    ]
    .join("\n");
    let exports = read(&text).kernel_exports(&Origin::unasserted());
    assert!(export_aliases(&exports).is_empty());
}

#[test]
fn a_report_carrying_no_measurements_says_so_rather_than_nothing() {
    let transcript = read("OBS|build|test-build|payload");
    assert!(transcript.measured_sections().is_empty());
    assert!(transcript.kernel_exports(&Origin::unasserted()).is_empty());
}
