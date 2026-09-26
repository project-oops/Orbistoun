//! Harvesting standard-library names from FreeBSD's own symbol maps.
//!
//! The target C library is FreeBSD-derived, and FreeBSD's version scripts list what each of its
//! libraries exports, grouped by version, under a permissive licence. A harvested list is
//! regenerated from a named source at a named revision, which a hand-typed list cannot be
//! (D126). Private version blocks (`FBSDprivate_1.0`) are skipped as implementation detail, and
//! linker syntax (`*;`, `local:`, `global:`) is not read as symbols.

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
/// Reserved names are kept: programs import them constantly (`__cxa_atexit`), and the C++ ABI
/// is nothing but reserved names (D126). Implementation detail is filtered by
/// [`is_public_block`], the distinction the format itself makes.
fn is_candidate(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// Parses one `Symbol.map`.
///
/// The format is a linker version script, `NAME { sym; sym; };`. Parsing is forgiving: an
/// unknown construct yields fewer symbols rather than an error, since a partial harvest is
/// useful.
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
/// Any `.map` file, not only `Symbol.map`: `libthr` declares its exports in `pthread.map`. The
/// format makes a file relevant, not its name.
pub fn is_version_script(name: &str) -> bool {
    // Case-insensitive: a tree fetched onto a case-insensitive filesystem can return a different
    // spelling than the one committed.
    std::path::Path::new(name)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("map"))
}

/// Directories under a FreeBSD source tree worth harvesting.
///
/// The C, threading, maths and system-call libraries: the interface a program links against.
/// Named explicitly so a harvest cannot pull in a driver's internal symbols. `lib/libsys` holds
/// the system-call stubs (`socket`, `setsockopt`, `sched_yield`) that FreeBSD split out of
/// `libc`; a missing directory only makes the harvest smaller, never fail.
pub const FREEBSD_LIBRARY_PATHS: &[&str] = &[
    "lib/libc",
    "lib/libsys",
    "lib/libthr",
    "lib/msun",
    "lib/libutil",
];

/// Renders a harvested list as a word list, with its provenance and regeneration command in
/// the header.
pub fn render(names: &[String], source: &str, on: &str) -> String {
    use std::fmt::Write as _;

    /// Fixed preamble, separate from the varying parts so it reads as prose.
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
    // The revision leads: a local path means nothing to anyone re-deriving this.
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

    /// Symbols are read from a version block.
    #[test]
    fn symbols_are_read_from_a_version_block() {
        let map = "FBSD_1.0 {\n\tabort;\n\tmemcpy;\n};\n";
        let found = parse_symbol_map(map);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].name, "abort");
        assert_eq!(found[0].version, "FBSD_1.0");
        assert_eq!(found[1].name, "memcpy");
    }

    /// Private version blocks are skipped.
    #[test]
    fn private_blocks_are_skipped_because_they_are_not_interface() {
        // Implementation detail, unlikely to be imported by name.
        let map = "FBSD_1.0 {\n\tmemcpy;\n};\nFBSDprivate_1.0 {\n\tnot_interface;\n};\n";
        let found = parse_symbol_map(map);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "memcpy");
    }

    /// Reserved names are kept.
    #[test]
    fn reserved_names_are_kept_because_programs_import_them() {
        // Reserved says whose namespace a name belongs to, not whether anybody links against it
        // (D126).
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

    /// Linker script syntax is not read as symbols.
    #[test]
    fn linker_syntax_is_not_mistaken_for_symbols() {
        // `local:` and `*;` are version-script constructs, not names.
        let map = "FBSD_1.0 {\nglobal:\n\tmemcpy;\nlocal:\n\t*;\n};\n";
        let found = parse_symbol_map(map);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "memcpy");
    }

    /// Comments are ignored.
    #[test]
    fn comments_are_ignored() {
        let map = "# a note\nFBSD_1.0 {\n\tmemcpy; # trailing note\n};\n";
        let found = parse_symbol_map(map);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "memcpy");
    }

    /// Symbols outside any block are ignored.
    #[test]
    fn symbols_outside_any_block_are_ignored() {
        // A malformed map should yield fewer symbols, never stray text as names.
        let map = "stray;\nFBSD_1.0 {\n\tmemcpy;\n};\nalso_stray;\n";
        let found = parse_symbol_map(map);
        assert_eq!(found.len(), 1);
    }

    /// An unparseable map yields nothing rather than failing.
    #[test]
    fn an_unparseable_map_yields_nothing_rather_than_failing() {
        // A map using an unknown construct costs its own symbols, not the run.
        assert!(parse_symbol_map("this is not a version script at all").is_empty());
        assert!(parse_symbol_map("").is_empty());
    }

    /// The rendered list carries its own provenance and still parses as a word list.
    #[test]
    fn the_rendered_list_carries_its_own_provenance() {
        // A list that says where it came from is a citation.
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
