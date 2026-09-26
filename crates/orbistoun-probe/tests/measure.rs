//! Reading `measure` records, and the export table one section of them carries.
//!
//! Written against a hand-built transcript that pins the record shape (seven fields, a
//! section name, a hash and an address), not against a sibling project's reports (D207).
//! Each test asserts on a refusal or an absence, which a lenient reader would fail.

use orbistoun_hle::knowledge::Oracle;
use orbistoun_probe::{KEXPORT_SECTION, Origin, Transcript, export_aliases};

/// A transcript carrying measurements, including a small export table.
///
/// The two hashes at `0x8000006f0` are `getpeername` and `_getpeername`: one function under
/// two names, which the alias grouping surfaces (D606).
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
    // An unparseable value is refused, never read as zero.
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
    // A number measured on a stand-in describes the stand-in (D246), so the grade is demoted.
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
    // Any eleven characters decode to a plausible hash, so a differently shaped record is
    // refused, not salvaged.
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
    // Skipped records still count as measurements: unreadable by this decoder is not absent.
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
    // The empty case, which a grouping returning every address would fail.
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
