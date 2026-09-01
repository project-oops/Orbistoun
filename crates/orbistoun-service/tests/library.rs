//! Settings, the title library, and what the service can say about a module.
//!
//! # Why the service and not a shim
//!
//! Principle 13: the CLI, the window and worker mode are interaction shims, and shared
//! orchestration lives here. So this is where the behaviour actually is - which means a
//! shim's tests would be testing a pass-through, and a bug here would be a bug in all three
//! at once.
//!
//! # What is covered and what is not
//!
//! Everything that answers a question about bytes or about the filesystem. Not the parts
//! that place an image, spawn threads or write a run report: those change process-wide state
//! or need an address space, and a test that reserved one would be testing the host's
//! allocator as much as anything here.
//!
//! The module fixtures are assembled byte by byte for the same reason as
//! `orbistoun-elf`'s: no title can be in this repository, and a hand-built file can be wrong
//! in exactly one way at a time.

use orbistoun_service::{
    FileConfig, LibrarySettings, Service, ServiceConfig, TitleEntry, read_title_metadata,
};
use std::path::{Path, PathBuf};

const EHDR: usize = 64;
const PHDR: usize = 56;

/// A bare ELF64 with the given `(p_type, offset, vaddr, filesz)` program headers.
fn elf_with(headers: &[(u32, u64, u64, u64)], size: usize) -> Vec<u8> {
    let phoff = EHDR;
    let mut elf = vec![0_u8; size.max(phoff + PHDR * headers.len())];
    elf[0..4].copy_from_slice(b"\x7fELF");
    elf[4] = 2; // 64-bit
    elf[5] = 1; // little endian
    elf[6] = 1; // version
    elf[7] = 9; // FreeBSD, as a target module carries
    elf[16..18].copy_from_slice(&0xfe18_u16.to_le_bytes()); // a vendor `e_type`
    elf[18..20].copy_from_slice(&62_u16.to_le_bytes()); // x86-64
    elf[24..32].copy_from_slice(&0x4000_u64.to_le_bytes()); // entry
    elf[32..40].copy_from_slice(&(phoff as u64).to_le_bytes());
    elf[54..56].copy_from_slice(&(PHDR as u16).to_le_bytes());
    elf[56..58].copy_from_slice(&(headers.len() as u16).to_le_bytes());

    for (index, (p_type, offset, vaddr, filesz)) in headers.iter().enumerate() {
        let at = phoff + PHDR * index;
        elf[at..at + 4].copy_from_slice(&p_type.to_le_bytes());
        elf[at + 8..at + 16].copy_from_slice(&offset.to_le_bytes());
        elf[at + 16..at + 24].copy_from_slice(&vaddr.to_le_bytes());
        elf[at + 32..at + 40].copy_from_slice(&filesz.to_le_bytes());
        elf[at + 40..at + 48].copy_from_slice(&filesz.to_le_bytes());
    }
    elf
}

/// A module with one load, one vendor segment and one ordinary GNU one.
fn shaped_module() -> Vec<u8> {
    elf_with(
        &[
            (1, 0, 0, 0x800),               // PT_LOAD covering the file
            (0x6100_0000, 0x400, 0x400, 8), // vendor data
            (0x6474_e550, 0x100, 0x100, 8), // GNU: in the OS range, not vendor data
        ],
        0x800,
    )
}

/// A service with nothing configured beyond the defaults.
///
/// `paths: None` disables reporting, which is what an inspection wants: nothing here should
/// write a run artifact as a side effect of being asked a question.
fn service() -> Service {
    Service::new(ServiceConfig::default())
}

/// A directory this test owns, named after what it is for.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("orbistoun-service-test-{name}"));
    // Removed first, so a previous run's leftovers cannot make a test pass or fail for
    // reasons this run knows nothing about.
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    dir
}

/// Writes a title directory with an entry file and, optionally, metadata.
fn title_at(root: &Path, name: &str, param_json: Option<&str>) {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).expect("a title directory");
    std::fs::write(dir.join("eboot.bin"), shaped_module()).expect("an entry file");
    if let Some(json) = param_json {
        let system = dir.join("sce_sys");
        std::fs::create_dir_all(&system).expect("a metadata directory");
        std::fs::write(system.join("param.json"), json).expect("metadata");
    }
}

// --- settings -------------------------------------------------------------------------------

/// A relative library root is resolved against the data root, not the working directory.
///
/// **The working directory is not a property of the installation** - it is a property of how
/// somebody happened to start the program. Same binary, same settings file, three different
/// libraries, and the window reports the difference as "no titles here".
#[test]
fn a_relative_library_root_is_resolved_against_the_data_root() {
    let settings = LibrarySettings::default();
    assert!(
        !Path::new(&settings.root).is_absolute(),
        "the default is relative, which is what makes this test meaningful"
    );

    let data_root = Path::new("/somewhere/orbistoun");
    assert_eq!(
        settings.resolve(data_root),
        data_root.join(&settings.root),
        "a relative root hangs off the data root"
    );
}

/// An absolute root is used exactly as given.
///
/// The ordinary case once somebody has pointed the window at their own folder: set once,
/// saved, and found afterwards by every build from every launcher.
#[test]
fn an_absolute_library_root_is_used_as_given() {
    let absolute = if cfg!(windows) {
        r"C:\titles"
    } else {
        "/titles"
    };
    let settings = LibrarySettings {
        root: absolute.to_owned(),
        ..LibrarySettings::default()
    };
    assert_eq!(
        settings.resolve(Path::new("/ignored/data/root")),
        PathBuf::from(absolute)
    );
}

/// A missing settings file is the defaults, and a malformed one is an error.
///
/// **Deliberately not silent.** A malformed file that quietly fell back to defaults would
/// look exactly like a setting that had no effect - and observing an effect is the entire
/// point of these being in a file at all.
#[test]
fn a_missing_settings_file_is_the_defaults_and_a_broken_one_is_not() {
    let dir = scratch("config");

    let absent = dir.join("nothing-here.toml");
    let defaults = FileConfig::load(&absent).expect("a missing file is not an error");
    assert_eq!(
        defaults.library.root,
        LibrarySettings::default().root,
        "and it is the defaults, not an empty configuration"
    );

    let broken = dir.join("broken.toml");
    std::fs::write(&broken, "library = [this is not toml").expect("write");
    assert!(
        FileConfig::load(&broken).is_err(),
        "a file that exists and cannot be parsed must be reported"
    );
}

/// A file naming one setting is valid, and the rest default.
///
/// A configuration that must be complete to be valid is one nobody edits.
#[test]
fn a_settings_file_naming_one_thing_leaves_the_rest_alone() {
    let dir = scratch("partial");
    let path = dir.join("config.toml");
    std::fs::write(&path, "[library]\nrun-call-budget = 12345\n").expect("write");

    let config = FileConfig::load(&path).expect("parses");
    assert_eq!(
        config.library.run_call_budget, 12345,
        "the named setting lands"
    );
    assert_eq!(
        config.library.root,
        LibrarySettings::default().root,
        "and everything else is the default"
    );
    assert_eq!(
        config.library.run_limit_seconds,
        LibrarySettings::default().run_limit_seconds
    );
}

/// Settings survive a round trip through the file they are written to.
///
/// The property that makes a starting file worth writing: a shim emits the defaults, a
/// person edits one line, and what comes back is what they meant.
#[test]
fn settings_survive_being_written_and_read_back() {
    let dir = scratch("roundtrip");
    let path = dir.join("config.toml");

    let mut original = FileConfig::default();
    original.library.run_call_budget = 999;
    original.library.run_limit_seconds = 7;
    original.library.root = "my-titles".to_owned();

    let text = original.to_toml().expect("serialises");
    assert!(!text.is_empty());
    std::fs::write(&path, &text).expect("write");

    let read_back = FileConfig::load(&path).expect("parses");
    assert_eq!(read_back.library.run_call_budget, 999);
    assert_eq!(read_back.library.run_limit_seconds, 7);
    assert_eq!(read_back.library.root, "my-titles");
}

// --- the title library ------------------------------------------------------------------------

/// A title is a directory with an entry file in it, and nothing else counts.
#[test]
fn a_title_is_a_directory_holding_an_entry_file() {
    let root = scratch("library");
    title_at(&root, "beta-title", None);
    title_at(&root, "alpha-title", None);
    // A directory with no entry file is not a title.
    std::fs::create_dir_all(root.join("not-a-title")).expect("a bare directory");
    // Nor is a loose file.
    std::fs::write(root.join("stray.bin"), b"x").expect("a loose file");

    let found = service().discover_titles(&root).expect("scans");
    assert_eq!(
        found.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
        ["alpha-title", "beta-title"],
        "sorted, so a library list does not reorder itself between runs"
    );
    assert!(found[0].module.is_file());
}

/// A library that cannot be read says which path it was, not just that it failed.
///
/// `io::Error` carries no path, so the bare message is "the system cannot find the path
/// specified" - which is not an answer to the only question the reader has.
#[test]
fn an_unreadable_library_names_the_path_in_the_error() {
    let missing = scratch("missing").join("definitely-not-here");
    let error = service()
        .discover_titles(&missing)
        .expect_err("a library that is not there cannot be scanned");
    let message = error.to_string();
    assert!(
        message.contains("definitely-not-here"),
        "the error should name the path it tried: {message}"
    );
}

/// A title names itself in its default language.
#[test]
fn a_title_is_named_in_the_language_it_says_is_its_default() {
    let root = scratch("metadata");
    title_at(
        &root,
        "TEST00001",
        Some(
            r#"{
                "titleId": "TEST00001",
                "contentVersion": "01.02",
                "localizedParameters": {
                    "defaultLanguage": "fr-FR",
                    "en-US": { "titleName": "The English One" },
                    "fr-FR": { "titleName": "Le Bon Titre" }
                }
            }"#,
        ),
    );

    let metadata = read_title_metadata(&root.join("TEST00001")).expect("metadata is readable");
    assert_eq!(
        metadata.title, "Le Bon Titre",
        concat!(
            "the default language is what the platform picks; guessing English would be wrong ",
            "for anything that ships without it"
        )
    );
    assert_eq!(metadata.title_id, "TEST00001");
    assert_eq!(metadata.version.as_deref(), Some("01.02"));
}

/// A default language that is not present falls back to any name rather than to none.
#[test]
fn a_missing_default_language_falls_back_to_a_name_that_is_there() {
    let root = scratch("fallback");
    title_at(
        &root,
        "TEST00002",
        Some(
            r#"{
                "titleId": "TEST00002",
                "localizedParameters": {
                    "defaultLanguage": "ja-JP",
                    "en-US": { "titleName": "The Only One" }
                }
            }"#,
        ),
    );

    let metadata = read_title_metadata(&root.join("TEST00002")).expect("metadata is readable");
    assert_eq!(metadata.title, "The Only One");
    assert_eq!(
        metadata.version, None,
        "an absent field is absent rather than an empty string"
    );
}

/// Metadata that is missing, malformed, or names nothing is simply absent.
///
/// A title with unreadable metadata is still a title somebody can run, so none of these is
/// an error - they are the ordinary case for anything homebrew.
#[test]
fn unreadable_metadata_is_absent_rather_than_an_error() {
    let root = scratch("nometadata");

    title_at(&root, "no-sce-sys", None);
    assert_eq!(read_title_metadata(&root.join("no-sce-sys")), None);

    title_at(&root, "not-json", Some("this is not json at all"));
    assert_eq!(read_title_metadata(&root.join("not-json")), None);

    title_at(
        &root,
        "no-names",
        Some(r#"{"titleId":"X","localizedParameters":{}}"#),
    );
    assert_eq!(
        read_title_metadata(&root.join("no-names")),
        None,
        "a metadata file that names the title in no language names it in none"
    );

    title_at(&root, "no-parameters", Some(r#"{"titleId":"X"}"#));
    assert_eq!(read_title_metadata(&root.join("no-parameters")), None);
}

/// A title always has something to call it.
///
/// **Never a blank**: a library row with no label is unusable, and the fallback has to be
/// the directory name because that is the one thing always present.
#[test]
fn a_title_always_has_a_name_to_show() {
    let root = scratch("display");
    title_at(&root, "PLAIN-DIRECTORY", None);
    title_at(
        &root,
        "NAMED",
        Some(
            r#"{"localizedParameters":{"defaultLanguage":"en-US","en-US":{"titleName":"Published Name"}}}"#,
        ),
    );

    let found = service().discover_titles(&root).expect("scans");
    for title in &found {
        assert!(
            !title.display_name().is_empty(),
            "{} has no name to show",
            title.name
        );
    }

    let named = found.iter().find(|t| t.name == "NAMED").expect("present");
    assert_eq!(named.display_name(), "Published Name");

    let plain = found
        .iter()
        .find(|t| t.name == "PLAIN-DIRECTORY")
        .expect("present");
    assert_eq!(
        plain.display_name(),
        "PLAIN-DIRECTORY",
        "the directory name is the fallback, and it is never blank"
    );
}

/// An entry with no metadata shows its directory name without touching the filesystem.
#[test]
fn a_title_entry_falls_back_without_reading_anything() {
    let entry = TitleEntry {
        name: "SOMEDIR".to_owned(),
        module: PathBuf::from("SOMEDIR/eboot.bin"),
        metadata: None,
    };
    assert_eq!(entry.display_name(), "SOMEDIR");
}

// --- what the service can say about a module ----------------------------------------------------

/// A module's structure is reported without executing or fully parsing it.
#[test]
fn a_container_is_described_without_being_run() {
    let bytes = shaped_module();
    let info = service().inspect_bytes(&bytes).expect("inspects");

    assert_eq!(info.entry, 0x4000);
    assert_eq!(info.machine, 62, "x86-64");
    assert_eq!(info.osabi, 9, "FreeBSD, as a target module carries");
    assert_eq!(info.program_headers, 3);
    assert_eq!(
        info.vendor_segments, 1,
        concat!(
            "the GNU segment is in the OS-specific range and is not vendor data - counting it ",
            "would overstate how much of a module is unhandled"
        )
    );
    assert_eq!(info.elf_offset, 0, "an unwrapped module starts at zero");
    assert!(
        info.mapped_segments.is_empty(),
        concat!(
            "nothing is wrapper-mapped in a bare container, which is not the same as nothing ",
            "being mapped"
        )
    );
}

/// Bytes that are not a container are refused rather than described.
#[test]
fn something_that_is_not_a_container_is_refused() {
    let service = service();
    assert!(service.inspect_bytes(&[0_u8; 8]).is_err(), "too short");
    assert!(
        service
            .inspect_bytes(b"not an elf at all, just text")
            .is_err(),
        "wrong magic"
    );
    assert!(service.survey_bytes(&[0_u8; 8]).is_err());
}

/// Reading a module from a path that is not there names the path.
#[test]
fn inspecting_a_path_that_is_not_there_names_it() {
    let missing = scratch("inspect").join("absent.bin");
    let error = service()
        .inspect_path(&missing)
        .expect_err("there is nothing to inspect");
    assert!(
        error.to_string().contains("absent.bin"),
        "the error should name the file: {error}"
    );
}

/// A module with no dynamic table has no imports to explain, and says so rather than
/// reporting none.
///
/// An empty import list reads as "needs nothing", which is never true of a real module.
#[test]
fn a_module_with_no_imports_refuses_rather_than_reporting_none() {
    let bytes = shaped_module();
    let service = service();
    assert!(
        service.explain_imports(&bytes).is_err(),
        "there is no dynamic table here, and an empty answer would be a claim"
    );
    assert!(service.unnamed_imports(&bytes).is_err());
    assert!(service.import_labels(&bytes).is_err());
}

// --- what the service declares -------------------------------------------------------------------

/// The service declares functions, and every one of them has a distinct hash.
///
/// A collision would make two functions indistinguishable at resolution time, which is a
/// silent wrong-function-called bug.
#[test]
fn every_declared_symbol_has_its_own_hash() {
    let service = service();
    let declared = service.declared_symbols();

    assert_eq!(
        declared.len(),
        service.declared_count(),
        "the count and the list must be the same fact"
    );
    assert!(
        !declared.is_empty(),
        "a service that declares nothing would make the rest of this vacuous"
    );

    let mut names = std::collections::BTreeSet::new();
    let mut nids = std::collections::BTreeSet::new();
    for symbol in &declared {
        assert!(
            names.insert(symbol.symbol.clone()),
            "{} is declared more than once",
            symbol.symbol
        );
        assert!(
            nids.insert(symbol.nid),
            "hash collision on {}",
            symbol.symbol
        );
        assert!(
            !symbol.library.is_empty(),
            "{} belongs to nothing",
            symbol.symbol
        );
    }

    assert!(
        declared.iter().any(|s| s.implemented),
        "something must be implemented, or the implemented flag is not being read"
    );
}

/// The service hashes a name the same way it resolves one.
///
/// A name hashed with a different suffix from the one the registry resolves against produces
/// a NID that matches nothing - silently, as an unresolved import rather than as an error.
#[test]
fn a_name_hashes_to_the_value_the_registry_knows_it_by() {
    let service = service();
    let declared = service.declared_symbols();
    let known = declared.first().expect("something is declared");

    assert_eq!(
        service.nid_for(&known.symbol).as_raw(),
        known.nid,
        "hashing {} must produce the hash it is declared under",
        known.symbol
    );
    assert!(service.is_named(service.nid_for(&known.symbol)));
    assert!(
        !service.is_named(orbistoun_nid::Nid::from_raw(0x0123_4567_89ab_cdef)),
        concat!(
            "a hash nothing declares is not named, and saying otherwise would put an invented ",
            "name in a trace"
        )
    );
}

/// The default policy is emitted as editable TOML, and says what it defaults to.
#[test]
fn the_default_policy_can_be_written_out_and_is_loud_by_default() {
    let service = service();
    let toml = service.default_policy_toml().expect("serialises");
    assert!(
        toml.contains("unimplemented"),
        "the default must be the loud one, not silent success: {toml}"
    );

    let (summary, count) = service.policy_summary();
    assert!(!summary.is_empty());
    assert_eq!(count, 0, "a default policy overrides nothing");
}

/// With no database configured, nothing claims to know how many names there are.
///
/// `None` and `Some(0)` are different facts - one is "no database", the other is "a database
/// with nothing in it" - and a shim reports them differently.
#[test]
fn an_absent_symbol_database_reports_nothing_rather_than_zero() {
    assert_eq!(service().symbol_db_len(), None);
}
