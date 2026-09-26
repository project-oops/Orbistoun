//! The provenance guards, tested by making each one fail.
//!
//! A guard is not trusted until it has been made to fail (D227). Every case breaks something on
//! purpose and asserts that the guard notices.

use std::io::Write as _;

/// Builds a database file with one name and the derivation given, and audits it.
///
/// Returns the combined output and the exit status: a fault that is reported and then exits zero is
/// the failure mode.
fn audit(json: &str, extra: &[&str]) -> (String, bool) {
    let dir = tempfile::tempdir().expect("a temp dir");
    let db = dir.path().join("db.json");
    let mut file = std::fs::File::create(&db).expect("writing the database");
    file.write_all(json.as_bytes())
        .expect("writing the database");
    drop(file);

    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_orbistoun-cli"));
    command.arg("audit").arg(&db).args(extra);
    // The harvest check resolves each record's path against the working directory, and the records
    // name paths inside this temp dir.
    command.current_dir(dir.path());
    let out = command.output().expect("running the audit");
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    (text, out.status.success())
}

/// An arbitrary suffix; these tests are about records, not hashes.
const SUFFIX: &str = "00112233";

/// A static record naming a module that lacks its string fails the audit.
#[test]
fn a_static_record_naming_a_module_that_lacks_the_string_is_caught() {
    // A static record claims "this exact string is in this exact file", which anyone holding the
    // corpus can settle.
    let dir = tempfile::tempdir().expect("a temp dir");
    let module = dir.path().join("module.bin");
    std::fs::write(
        &module,
        b"some other identifiers in here\0scePresentSomething\0",
    )
    .expect("writing the module");
    let path = module.display().to_string().replace('\\', "/");

    let json = format!(
        r#"{{"suffix_hex":"{SUFFIX}","names":["sceAbsentFromTheModule"],
            "derivations":{{"sceAbsentFromTheModule":
              {{"found":"static","by":"module-strings","from":"{path}","on":"2026-01-01"}}}}}}"#
    );
    let (out, ok) = audit(&json, &["--verify-harvest"]);
    assert!(
        !ok,
        concat!(
            "a record claiming a string the module does not contain must fail the audit, ",
            "not merely be mentioned in it. Output was:\n{}"
        ),
        out
    );
    assert!(
        out.contains("sceAbsentFromTheModule"),
        "the failure must name the record that is wrong:\n{out}"
    );
}

/// A static record whose module is present and contains the string passes.
#[test]
fn a_static_record_whose_module_is_present_and_contains_it_passes() {
    // The converse, so the test above measures the record rather than the absence of a corpus.
    let dir = tempfile::tempdir().expect("a temp dir");
    let module = dir.path().join("module.bin");
    std::fs::write(&module, b"\0sceReallyInTheModule\0").expect("writing the module");
    let path = module.display().to_string().replace('\\', "/");

    let json = format!(
        r#"{{"suffix_hex":"{SUFFIX}","names":["sceReallyInTheModule"],
            "derivations":{{"sceReallyInTheModule":
              {{"found":"static","by":"module-strings","from":"{path}","on":"2026-01-01"}}}}}}"#
    );
    let (out, ok) = audit(&json, &["--verify-harvest"]);
    assert!(ok, "a record that holds must pass:\n{out}");
    assert!(
        out.contains("1 static record(s) re-harvested"),
        "and must say it checked it, rather than passing quietly:\n{out}"
    );
}

/// A module that is absent is reported unchecked, never passed as checked.
#[test]
fn a_module_that_is_not_here_is_reported_unchecked_and_never_passed_as_checked() {
    // CI has no corpus, so the absent case is the normal one.
    let json = format!(
        r#"{{"suffix_hex":"{SUFFIX}","names":["sceSomethingOrOther"],
            "derivations":{{"sceSomethingOrOther":
              {{"found":"static","by":"module-strings",
                "from":"titles/NOT-HERE/eboot.bin","on":"2026-01-01"}}}}}}"#
    );
    let (out, ok) = audit(&json, &["--verify-harvest"]);
    assert!(ok, "an absent corpus is not an error:\n{out}");
    assert!(
        out.contains("not checked") && out.contains("titles/NOT-HERE/eboot.bin"),
        "it must say what it could not check, by name:\n{out}"
    );
    assert!(
        out.contains("0 static record(s) re-harvested"),
        "and must not count an unread module as re-harvested:\n{out}"
    );
}

/// A stale ceiling fails even when every name is accounted for (D213).
#[test]
fn a_stale_ceiling_fails_even_when_nothing_is_unaccounted() {
    // The ceiling comparison runs even when the unaccounted set is empty: an entry that stopped
    // applying must leave the ceiling.
    let dir = tempfile::tempdir().expect("a temp dir");
    let ceiling = dir.path().join("ceiling.txt");
    std::fs::write(
        &ceiling,
        "# a name that no longer applies\nsceLongSinceAccountedFor\n",
    )
    .expect("writing the ceiling");

    let json = format!(
        r#"{{"suffix_hex":"{SUFFIX}","names":["sceKnownStatic"],
            "derivations":{{"sceKnownStatic":
              {{"found":"static","by":"module-strings",
                "from":"titles/NOT-HERE/eboot.bin","on":"2026-01-01"}}}}}}"#
    );
    let (out, ok) = audit(&json, &["--ceiling", &ceiling.display().to_string()]);
    assert!(
        !ok,
        "a ceiling listing a name that is no longer unaccounted must fail:\n{out}"
    );
    assert!(
        out.contains("sceLongSinceAccountedFor"),
        "and must name the entry to remove:\n{out}"
    );
}

/// A generated record at the wrong index does not verify.
#[test]
fn a_generated_record_at_the_wrong_index_does_not_verify() {
    // A forged record fails as loudly as a missing one, because the derivation is re-run rather
    // than read.
    let json = format!(
        r#"{{"suffix_hex":"{SUFFIX}","names":["sceKernelSomething"],
            "derivations":{{"sceKernelSomething":
              {{"found":"generated","pattern":"prefix-module-verb-object",
                "index":7,"on":"2026-01-01"}}}}}}"#
    );
    let (out, ok) = audit(&json, &[]);
    assert!(!ok, "an unaccounted name exits non-zero:\n{out}");
    assert!(
        out.contains("0 of 1 names re-derived"),
        "a record pointing at an index that produces something else is not a derivation:\n{out}"
    );
}

/// The evidence summary counts every class, including the empty ones.
#[test]
fn the_evidence_summary_counts_every_class_including_the_empty_ones() {
    // "0 external" is asserted rather than assumed.
    let json = format!(
        r#"{{"suffix_hex":"{SUFFIX}","names":["sceOne","sceTwo"],
            "derivations":{{
              "sceOne":{{"found":"static","by":"cross-module",
                         "from":"titles/NOT-HERE/eboot.bin","on":"2026-01-01"}},
              "sceTwo":{{"found":"runtime","by":"call-trace",
                         "how":"read from a trace","on":"2026-01-01"}}}}}}"#
    );
    let (out, _) = audit(&json, &[]);
    assert!(
        out.contains("by evidence: 0 derived, 1 static, 1 runtime, 0 external"),
        "the evidence line must break the database down by class:\n{out}"
    );
}
