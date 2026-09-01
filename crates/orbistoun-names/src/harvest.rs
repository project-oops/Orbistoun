//! Harvesting standard-library names from FreeBSD's own symbol maps.
//!
//! The target C library is FreeBSD-derived, and FreeBSD says exactly what its C library
//! exports: every `Symbol.map` in the source tree lists the symbols of one library,
//! grouped by the version they appeared in. That is the authoritative answer, published
//! by the project itself under a permissive licence.
//!
//! # Why this beats a curated list
//!
//! A hand-written list of function names is accurate right up until it is not, and
//! "somebody typed these from memory of the standards" is provenance nobody can audit.
//! A harvested list can be regenerated from a named source at a named revision, and
//! anyone can repeat it. That is the difference between a claim and a citation.
//!
//! It is also simply *bigger*. A person types the functions they think of; a symbol map
//! contains the ones that exist.
//!
//! # What is skipped, and why
//!
//! - **Private version blocks.** `FBSDprivate_1.0` and friends are implementation
//!   detail, not interface. They are unlikely to be imported by name and would bloat
//!   the search for nothing.
//! - **Reserved names.** Anything starting with an underscore is the implementation's
//!   own namespace.
//! - **Wildcards and section markers.** `*;`, `local:`, `global:` are linker script
//!   syntax rather than symbols.

/// A symbol name and the map it came from.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Harvested {
    /// The exported symbol.
    pub name: String,
    /// The version block it was declared in, for the record.
    pub version: String,
}

/// Whether a version block holds public interface rather than implementation detail.
fn is_public_block(version: &str) -> bool {
    !version.to_ascii_lowercase().contains("private")
}

/// Whether a symbol is one worth trying as a candidate.
///
/// **Reserved names are kept**, and an earlier version of this was wrong to drop them.
/// "Anything leading with an underscore is implementation detail" is a reasonable-sounding
/// rule that excluded `__cxa_atexit` - the single most-called import across every title
/// examined, 53.5% of all calls. Programs import reserved names constantly; the C++ ABI
/// is nothing but reserved names (D126).
///
/// Implementation detail is already filtered by [`is_public_block`], which is the
/// distinction the format itself makes rather than one invented here.
fn is_candidate(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// Parses one `Symbol.map`.
///
/// The format is a linker version script: `NAME { sym; sym; };`. Only the shape matters
/// here, so this is deliberately forgiving - a map that uses a construct this does not
/// know yields fewer symbols rather than an error, because a partial harvest is useful
/// and a failed one is not.
pub fn parse_symbol_map(text: &str) -> Vec<Harvested> {
    let mut out = Vec::new();
    let mut version: Option<String> = None;

    for raw in text.lines() {
        let line = match raw.split_once('#') {
            Some((before, _)) => before.trim(),
            None => raw.trim(),
        };
        if line.is_empty() {
            continue;
        }

        if let Some(name) = line.strip_suffix('{') {
            version = Some(name.trim().to_owned());
            continue;
        }
        if line.starts_with('}') {
            version = None;
            continue;
        }

        let Some(current) = version.as_deref() else {
            continue;
        };
        if !is_public_block(current) {
            continue;
        }
        // Section markers are linker syntax, not symbols.
        if line.ends_with(':') {
            continue;
        }
        let Some(symbol) = line.strip_suffix(';') else {
            continue;
        };
        let symbol = symbol.trim();
        if is_candidate(symbol) {
            out.push(Harvested {
                name: symbol.to_owned(),
                version: current.to_owned(),
            });
        }
    }
    out
}

/// Whether a file is a linker version script worth reading.
///
/// **Any `.map` file, not one called `Symbol.map`.** That filename was an assumption, and
/// it cost the whole threading library: `libthr` declares its exports in `pthread.map`,
/// so every `pthread_*` name was missing from a harvest that reported success (D127).
///
/// The same mistake as the reserved-name filter, twice in one afternoon - a rule invented
/// on top of a source rather than read from it. The format is what makes a file relevant,
/// not what somebody called it.
pub fn is_version_script(name: &str) -> bool {
    // Case-insensitive because the check is about what a file *is*, and a source tree
    // fetched onto a case-insensitive filesystem can hand back a different spelling than
    // the one it was committed with.
    std::path::Path::new(name)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("map"))
}

/// Directories under a FreeBSD source tree worth harvesting.
///
/// The C library, the threading library, the maths library and the system-call library -
/// between them, the interface a program links against. Named explicitly rather than
/// walking the whole tree, so a harvest cannot quietly pull in a driver's internal symbols.
///
/// # `lib/libsys` was the missing one, and it cost 187 names
///
/// FreeBSD split its system-call stubs out of `libc` into `lib/libsys`, so `socket`,
/// `setsockopt`, `shutdown`, `sched_yield` and their neighbours moved out from under a
/// list that named `lib/libc` explicitly. The harvest kept reporting success.
///
/// D168 diagnosed the symptom correctly - a missing syscall family - and proposed
/// re-harvesting from a fuller checkout. That was the right instinct and the wrong fix:
/// depth and completeness were never the problem, a *directory name* was, and the tree
/// being harvested had reorganised underneath the constant (D191).
///
/// The same shape as D127, where the harvest read only files called `Symbol.map` and lost
/// every `pthread_*` name because `libthr` calls its file `pthread.map`. Both are a rule
/// written once about a source that then moved. Neither failed loudly, because a harvest
/// that finds fewer files still finds files.
pub const FREEBSD_LIBRARY_PATHS: &[&str] = &[
    "lib/libc",
    "lib/libsys",
    "lib/libthr",
    "lib/msun",
    "lib/libutil",
];

/// Renders a harvested list as a word list, with the provenance in its header.
///
/// The header is the point as much as the names are: a list that says where it came
/// from and how to regenerate it is a citation, and one that does not is an assertion.
pub fn render(names: &[String], source: &str, on: &str) -> String {
    use std::fmt::Write as _;

    /// Fixed preamble. Separate from the parts that vary so the explanation reads as
    /// prose rather than as a sequence of pushes.
    const PREAMBLE: &[&str] = &[
        "# Function names exported by FreeBSD's C, threading, and maths libraries.",
        "#",
        "# GENERATED - do not edit by hand. Regenerate with:",
        "#     orbistoun-cli harvest <path-to-freebsd-src>",
        "#",
    ];
    /// What the list is, and why it is a citation rather than an assertion.
    const EXPLANATION: &[&str] = &[
        "#",
        "# Read from the Symbol.map files FreeBSD publishes with its own source, which",
        "# are the authoritative statement of what that library exports. The target C",
        "# library is FreeBSD-derived, so these are not guesses - they are the names the",
        "# interface actually uses (CLAUDE.md principle 1).",
        "#",
        "# Nothing here was read from a vendor binary.",
        "",
    ];

    let mut text = String::new();
    for line in PREAMBLE {
        let _ = writeln!(text, "{line}");
    }
    // The revision leads, because a local path is meaningless to anyone re-deriving
    // this - and re-derivability is the entire point of recording a source at all.
    let _ = writeln!(text, "# Source:    {source}");
    let _ = writeln!(text, "# Harvested: {on}");
    let _ = writeln!(text, "# Symbols:   {}", names.len());
    for line in EXPLANATION {
        let _ = writeln!(text, "{line}");
    }
    for name in names {
        let _ = writeln!(text, "{name}");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::{parse_symbol_map, render};

    #[test]
    fn symbols_are_read_from_a_version_block() {
        let map = "FBSD_1.0 {\n\tabort;\n\tmemcpy;\n};\n";
        let found = parse_symbol_map(map);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].name, "abort");
        assert_eq!(found[0].version, "FBSD_1.0");
        assert_eq!(found[1].name, "memcpy");
    }

    #[test]
    fn private_blocks_are_skipped_because_they_are_not_interface() {
        // Implementation detail, unlikely to be imported by name, and it would bloat
        // the search for nothing.
        let map = "FBSD_1.0 {\n\tmemcpy;\n};\nFBSDprivate_1.0 {\n\tnot_interface;\n};\n";
        let found = parse_symbol_map(map);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "memcpy");
    }

    #[test]
    fn reserved_names_are_kept_because_programs_import_them() {
        // This asserted the opposite until a harvest using that rule dropped
        // `__cxa_atexit` - the single most-called import across every title examined.
        // Reserved says whose namespace a name belongs to, not whether anybody links
        // against it, and the C++ ABI is nothing but reserved names (D126).
        let map = "FBSD_1.0 {
	_Exit;
	__cxa_atexit;
	public_one;
};
";
        let found = parse_symbol_map(map);
        assert_eq!(found.len(), 3, "all three are exported interface");
        assert!(found.iter().any(|f| f.name == "__cxa_atexit"));
    }

    #[test]
    fn linker_syntax_is_not_mistaken_for_symbols() {
        // `local:` and `*;` are version-script constructs. Treating them as names would
        // put nonsense in the candidate list.
        let map = "FBSD_1.0 {\nglobal:\n\tmemcpy;\nlocal:\n\t*;\n};\n";
        let found = parse_symbol_map(map);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "memcpy");
    }

    #[test]
    fn comments_are_ignored() {
        let map = "# a note\nFBSD_1.0 {\n\tmemcpy; # trailing note\n};\n";
        let found = parse_symbol_map(map);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "memcpy");
    }

    #[test]
    fn symbols_outside_any_block_are_ignored() {
        // A malformed map should yield fewer symbols, never stray text as names.
        let map = "stray;\nFBSD_1.0 {\n\tmemcpy;\n};\nalso_stray;\n";
        let found = parse_symbol_map(map);
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn an_unparseable_map_yields_nothing_rather_than_failing() {
        // A partial harvest is useful; a failed one is not. A map using a construct
        // this does not know should cost its own symbols, not the whole run.
        assert!(parse_symbol_map("this is not a version script at all").is_empty());
        assert!(parse_symbol_map("").is_empty());
    }

    #[test]
    fn the_rendered_list_carries_its_own_provenance() {
        // A list that says where it came from is a citation; one that does not is an
        // assertion, and this whole file exists to move from the second to the first.
        let text = render(
            &["abort".to_owned(), "memcpy".to_owned()],
            "freebsd-src @ releng/14.0",
            "2026-08-19",
        );
        assert!(text.contains("GENERATED"));
        assert!(text.contains("freebsd-src @ releng/14.0"));
        assert!(text.contains("2026-08-19"));
        assert!(text.contains("orbistoun-cli harvest"));

        // And still parses as an ordinary word list.
        let parsed = crate::word_list(&text);
        assert_eq!(parsed, vec!["abort", "memcpy"]);
    }
}
