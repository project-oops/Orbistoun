//! ABI constants, harvested from FreeBSD headers.
//!
//! Implementing needs numbers: `socket(AF_INET, SOCK_STREAM, 0)` cannot be mapped onto a
//! host socket without them, and a wrong value creates the wrong kind of socket (D352).
//!
//! Only `#define NAME <number>` and its trailing comment are taken, from named headers: a
//! bare number is an interface fact, like a symbol name. No function bodies, structure
//! layouts or expressions; `#define X (Y | Z)` is skipped rather than evaluated. The target
//! is FreeBSD-derived, so each value is `published` about FreeBSD and `assumed` about a
//! guest, as the rendered file says.

use std::path::Path;

use anyhow::{Context as _, Result};

/// One harvested definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Constant {
    /// The name, exactly as the header spells it.
    pub(crate) name: String,
    /// The value as written; hexadecimal stays hexadecimal, so it compares directly with
    /// the header.
    pub(crate) value: String,
    /// The header's own trailing comment, where it had one.
    pub(crate) comment: String,
}

/// A header to read, and what it is for.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Header {
    /// The section name in the rendered file.
    pub(crate) section: &'static str,
    /// Path within a FreeBSD checkout.
    pub(crate) path: &'static str,
    /// One line saying what the section holds.
    pub(crate) purpose: &'static str,
}

/// The headers this harvests, and why each is here.
///
/// A list rather than a directory walk, so every section is one somebody needed.
pub(crate) const HEADERS: &[Header] = &[
    Header {
        section: "errno",
        path: "sys/sys/errno.h",
        purpose: "Error numbers. `errno` is what every failing call reports through.",
    },
    Header {
        section: "signal",
        path: "sys/sys/signal.h",
        purpose: "Signal numbers.",
    },
    Header {
        section: "socket",
        path: "sys/sys/socket.h",
        purpose: "Address families, socket types, and socket-level options.",
    },
    Header {
        section: "in",
        path: "sys/netinet/in.h",
        purpose: "Internet protocol numbers and IPv4/IPv6 socket options.",
    },
    Header {
        section: "fcntl",
        path: "sys/sys/fcntl.h",
        purpose: "File open flags and descriptor commands.",
    },
    Header {
        section: "sysctl",
        path: "sys/sys/sysctl.h",
        purpose: "Kernel MIB identifiers - the numbers a `sysctl` name array is made of.",
    },
    Header {
        section: "clock",
        path: "sys/sys/_clock_id.h",
        purpose: "Clock identifiers - which clock a guest is asking `clock_gettime` about.",
    },
    Header {
        section: "unistd",
        path: "sys/sys/unistd.h",
        purpose: "Access modes and seek origins - what a guest asks `access` and `lseek` about.",
    },
    Header {
        section: "if",
        path: "sys/net/if.h",
        purpose: "Network interface flags - how a guest tells a loopback from something it can be reached on.",
    },
    Header {
        section: "dirent",
        path: "sys/sys/dirent.h",
        purpose: "Directory entry types - how a guest tells a directory from a file while listing one.",
    },
    Header {
        section: "stat",
        path: "sys/sys/stat.h",
        purpose: "File mode bits - the type and permission halves of `st_mode`.",
    },
    Header {
        section: "syslimits",
        path: "sys/sys/syslimits.h",
        purpose: "Size ceilings - how long a path may be, which is what a caller's buffer is sized by.",
    },
    Header {
        section: "event",
        path: "sys/sys/event.h",
        purpose: "Event filters and actions - how a server waits for a descriptor without polling it.",
    },
    Header {
        section: "syscall",
        path: "sys/sys/syscall.h",
        purpose: "System call numbers - what a guest asks the kernel for directly, past every name.",
    },
];

/// Extracts every `#define NAME <number>` from one header's text.
///
/// Pure, so the parsing is testable without a checkout. First definition wins: headers
/// guard alternatives behind `#if`, which this does not evaluate, and taking the first
/// keeps the output deterministic.
#[must_use]
pub(crate) fn extract(text: &str) -> Vec<Constant> {
    let mut out: Vec<Constant> = Vec::new();
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("#define") else {
            continue;
        };
        if !rest.starts_with([' ', '\t']) {
            continue;
        }
        let mut parts = rest.split_whitespace();
        let (Some(name), Some(value)) = (parts.next(), parts.next()) else {
            continue;
        };
        if !is_constant_name(name) || !is_plain_number(value) {
            continue;
        }
        if out.iter().any(|held| held.name == name) {
            continue;
        }
        let tail = rest
            .split_once(value)
            .map(|(_, after)| after)
            .unwrap_or_default();
        out.push(Constant {
            name: name.to_owned(),
            value: as_toml_number(value),
            comment: comment_of(tail),
        });
    }
    out
}

/// Whether a token is a name this harvests.
///
/// Starts upper case, so a lower-case internal macro is left alone, and has no parentheses,
/// since a function-like macro is code. The rest may be lower case, as in `SYS_read` from
/// `sys/sys/syscall.h` (D378).
fn is_constant_name(token: &str) -> bool {
    !token.is_empty()
        && token.starts_with(|c: char| c.is_ascii_uppercase())
        && token.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Whether a value is a bare number rather than an expression.
///
/// Anything composed (`(A | B)`, a cast, a reference to another name) is skipped. A sign
/// and one pair of brackets still count as a bare number, because `sys/sys/event.h` writes
/// every filter as `#define EVFILT_READ (-1)`.
fn is_plain_number(token: &str) -> bool {
    let token = strip_brackets(token);
    let token = token.strip_prefix('-').unwrap_or(token);
    let digits = token
        .strip_prefix("0x")
        .or_else(|| token.strip_prefix("0X"));
    match digits {
        Some(hex) => !hex.is_empty() && hex.chars().all(|c| c.is_ascii_hexdigit()),
        None => !token.is_empty() && token.chars().all(|c| c.is_ascii_digit()),
    }
}

/// One pair of wrapping brackets removed, if that is all they are.
///
/// One pair only: nested brackets mean an expression.
fn strip_brackets(token: &str) -> &str {
    token
        .strip_prefix('(')
        .and_then(|inner| inner.strip_suffix(')'))
        .unwrap_or(token)
}

/// A header's number, spelled the way TOML spells it.
///
/// A leading zero is C octal (`S_IFDIR` is `0040000`), which TOML rejects, so it becomes
/// TOML's `0o` form: still octal, so it compares with the header (D374). Hexadecimal and
/// decimal pass through.
fn as_toml_number(token: &str) -> String {
    // The brackets around a negative number are C's; `-1` is `-1` in both.
    let token = strip_brackets(token);
    let (sign, digits) = match token.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", token),
    };
    let is_octal = digits.len() > 1
        && digits.starts_with('0')
        && digits.chars().skip(1).all(|c| c.is_ascii_digit());
    if is_octal {
        format!("{sign}0o{}", digits.trim_start_matches('0'))
    } else {
        format!("{sign}{digits}")
    }
}

/// The header's trailing comment, flattened to one line.
///
/// Kept as the header's own description. A comment that runs onto the next line is marked,
/// not silently cut, because a truncated sentence reads as a complete one.
fn comment_of(tail: &str) -> String {
    let Some(start) = tail.find("/*") else {
        return String::new();
    };
    let body = &tail[start + 2..];
    let (body, closed) = body
        .split_once("*/")
        .map_or((body, false), |(before, _)| (before, true));
    let flattened = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if flattened.is_empty() || closed {
        flattened
    } else {
        format!("{flattened} ...")
    }
}

/// The revision the checkout is at.
///
/// Asked of `git` rather than taken as an argument, so the header states what the harvest
/// ran against and a hand-edited header differs from a regeneration.
///
/// # Errors
///
/// When the checkout is not a git repository or `git` cannot be run, rather than falling
/// back to "unknown".
fn revision_of(source: &Path) -> Result<String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(source)
        .args(["rev-parse", "HEAD"])
        .output()
        .with_context(|| format!("running git in {}", source.display()))?;
    anyhow::ensure!(
        output.status.success(),
        "{} is not a git checkout, so the harvest cannot say which revision it read",
        source.display()
    );
    let commit = String::from_utf8(output.stdout)
        .context("git printed something that is not text")?
        .trim()
        .to_owned();
    anyhow::ensure!(!commit.is_empty(), "git named no commit");
    Ok(format!("commit {commit}"))
}

/// Reads every header and renders the data file.
///
/// # Errors
///
/// When the checkout is not a directory, or a header named above is missing. A missing
/// header is named rather than skipped, so a sparse checkout cannot yield a silently
/// smaller table.
pub(crate) fn run(source: &Path) -> Result<String> {
    use std::fmt::Write as _;

    anyhow::ensure!(
        source.is_dir(),
        "{} is not a directory - point this at a FreeBSD source checkout",
        source.display()
    );

    let revision = revision_of(source)?;
    let mut out = String::new();
    out.push_str(&header(&revision));
    let mut total = 0_usize;
    for header in HEADERS {
        let path = source.join(header.path);
        let text = std::fs::read_to_string(&path).with_context(|| {
            format!(
                concat!(
                    "reading {} - if the checkout is sparse it may not include this. ",
                    "`git sparse-checkout add sys/sys sys/netinet include` adds what this needs"
                ),
                path.display()
            )
        })?;
        let found = extract(&text);
        total += found.len();
        report_skipped(header.section, &skipped(&text));

        let _ = write!(out, "\n[{}]\n", header.section);
        let _ = write!(out, "# {}\n# From {}.\n", header.purpose, header.path);
        let mut sorted = found;
        sorted.sort_by(|a, b| a.name.cmp(&b.name));
        for constant in sorted {
            let _ = write!(out, "{} = {}", constant.name, constant.value);
            if !constant.comment.is_empty() {
                let _ = write!(out, "  # {}", constant.comment);
            }
            out.push('\n');
        }
    }
    eprintln!("{total} constants from {} headers", HEADERS.len());
    Ok(out)
}

/// How many skipped names to print before saying only how many are left.
///
/// Enough to show what kind of thing was skipped without burying the counts.
const SKIPPED_SHOWN: usize = 8;

/// Says what a section did not take, and names some of it.
///
/// A `#define` whose name qualifies and whose value does not is skipped by a spelling rule,
/// so it is reported rather than dropped silently (D385).
fn report_skipped(section: &str, names: &[String]) {
    if names.is_empty() {
        return;
    }
    let shown: Vec<&str> = names
        .iter()
        .take(SKIPPED_SHOWN)
        .map(String::as_str)
        .collect();
    let rest = names.len().saturating_sub(shown.len());
    let tail = if rest > 0 {
        format!(", and {rest} more")
    } else {
        String::new()
    };
    eprintln!(
        "  {section}: {} skipped - {}{tail}",
        names.len(),
        shown.join(", ")
    );
}

/// Every name this could have taken and did not, in the order the header states them.
///
/// Names that qualify with values that do not. Names that do not qualify, such as
/// lower-case internal macros, are not listed.
#[must_use]
pub(crate) fn skipped(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("#define") else {
            continue;
        };
        if !rest.starts_with([' ', '\t']) {
            continue;
        }
        let mut parts = rest.split_whitespace();
        let (Some(name), Some(value)) = (parts.next(), parts.next()) else {
            continue;
        };
        if is_constant_name(name) && !is_plain_number(value) && !out.iter().any(|held| held == name)
        {
            out.push(name.to_owned());
        }
    }
    out
}

/// The file's own preamble, saying what it is and what it is not.
fn header(revision: &str) -> String {
    format!(
        concat!(
            "# ABI constants harvested from FreeBSD headers.\n",
            "#\n",
            "# GENERATED by `orbistoun-gen constants` - see docs/REFERENCES.md for what was\n",
            "# taken and how it was checked. Do not hand-edit: a value typed in by a person\n",
            "# is one nobody can trace back to a header.\n",
            "#\n",
            "# Source: github.com/freebsd/freebsd-src, BSD-2-Clause, {}.\n",
            "#\n",
            "# **These are FreeBSD's numbers, not the target's.** The target platform is\n",
            "# FreeBSD-derived, which is why they are worth having and also why they are not\n",
            "# facts about it. Each is `published` about FreeBSD and `assumed` about a guest,\n",
            "# and a guest passing a value that disagrees is what would show it.\n"
        ),
        revision
    )
}

#[cfg(test)]
mod tests {
    use super::{extract, is_plain_number};

    /// A plain definition is taken, with the header's own comment.
    #[test]
    fn a_bare_number_is_harvested_with_its_comment() {
        let found = extract("#define\tENOENT\t\t2\t\t/* No such file or directory */\n");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "ENOENT");
        assert_eq!(found[0].value, "2");
        assert_eq!(found[0].comment, "No such file or directory");
    }

    /// Hexadecimal stays hexadecimal, so it compares directly with the header.
    #[test]
    fn a_hexadecimal_value_is_not_normalised() {
        let found = extract("#define\tSOL_SOCKET\t0xffff\t\t/* options for socket level */\n");
        assert_eq!(found[0].value, "0xffff");
    }

    /// A negative constant in brackets is still a bare number, as every filter in
    /// `sys/sys/event.h` is written.
    #[test]
    fn a_bracketed_negative_is_harvested() {
        let found = extract(
            "#define EVFILT_READ		(-1)
#define EVFILT_WRITE	(-2)
",
        );
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].name, "EVFILT_READ");
        assert_eq!(
            found[0].value, "-1",
            "the brackets are C's, and TOML has no use for them"
        );
        assert_eq!(found[1].value, "-2");
    }

    /// A bare negative, which some headers write without the brackets.
    #[test]
    fn a_bare_negative_is_harvested_too() {
        let found = extract(
            "#define SOMETHING -3
",
        );
        assert_eq!(found[0].value, "-3");
    }

    /// An expression is skipped, never evaluated.
    #[test]
    fn a_composed_value_is_left_alone() {
        let text = concat!(
            "#define O_ACCMODE (O_RDONLY|O_WRONLY)\n",
            "#define O_RDONLY 0x0000\n",
            "#define MAP_FAILED ((void *)-1)\n"
        );
        let found = extract(text);
        assert_eq!(found.len(), 1, "only the bare number");
        assert_eq!(found[0].name, "O_RDONLY");
    }

    /// An unterminated comment is marked rather than silently truncated.
    #[test]
    fn a_comment_running_onto_the_next_line_says_that_it_does() {
        let found = extract(
            "#define	AT_EACCESS	0x0100	/* Check access using effective
",
        );
        assert_eq!(found[0].value, "0x0100");
        assert_eq!(found[0].comment, "Check access using effective ...");

        let whole = extract(
            "#define	ENOENT	2	/* No such file or directory */
",
        );
        assert_eq!(
            whole[0].comment, "No such file or directory",
            "no marker when it closes"
        );
    }

    /// A function-like macro is code, not a constant.
    #[test]
    fn a_function_like_macro_is_not_a_constant() {
        assert!(extract("#define IN_CLASSA(i) (((u_int32_t)(i) & 0x80) == 0)\n").is_empty());
    }

    /// First definition wins, so the output is deterministic without evaluating `#if`.
    #[test]
    fn the_first_definition_of_a_name_is_the_one_kept() {
        let found = extract("#define SIGPIPE 13\n#define SIGPIPE 99\n");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].value, "13");
    }

    /// A name must start upper case and may continue in any case, as `SYS_read` does (D378).
    #[test]
    fn a_name_must_start_upper_case_and_may_go_on_however_it_likes() {
        // A leading underscore is an internal macro, still left alone.
        assert!(extract("#define __LIBC_PRIVATE 1\n").is_empty());
        // A leading lower case letter is not a constant name.
        assert!(extract("#define foo 1\n").is_empty());

        // Mixed case, which is what a syscall number looks like.
        let found = extract("#define SYS_read 3\n");
        assert_eq!(found.len(), 1, "a syscall number is a constant");
        assert_eq!(found[0].name, "SYS_read");
        assert_eq!(found[0].value, "3");
    }

    /// What is skipped is named (D385).
    #[test]
    fn what_was_skipped_is_named_rather_than_dropped() {
        let text = concat!(
            "#define TAKEN 3\n",
            "#define COMPOSED (A|B)\n",
            "#define CAST ((void *)-1)\n",
            "#define lowercase 4\n",
        );
        let names = super::skipped(text);
        assert_eq!(
            names,
            vec!["COMPOSED".to_owned(), "CAST".to_owned()],
            "the two that qualify by name and not by value"
        );
        assert!(
            !names.contains(&"TAKEN".to_owned()),
            "what was taken is not also reported as skipped"
        );
        assert!(
            !names.contains(&"lowercase".to_owned()),
            "an internal macro is not a constant anybody was looking for"
        );
    }

    /// A header this takes everything from reports nothing skipped.
    #[test]
    fn a_header_with_nothing_to_skip_says_nothing() {
        assert!(super::skipped("#define A 1\n#define B 0x2\n#define C (-3)\n").is_empty());
    }

    /// The number test itself, at its edges.
    ///
    /// `-1` and `(1)` are numbers; an expression is not.
    #[test]
    fn a_number_is_told_from_everything_else() {
        assert!(is_plain_number("0"));
        assert!(is_plain_number("13"));
        assert!(is_plain_number("0xffff"));
        assert!(is_plain_number("-1"), "a bare negative");
        assert!(is_plain_number("(-1)"), "and the way a header writes one");
        assert!(
            is_plain_number("(1)"),
            "brackets do not make it an expression"
        );
        assert!(!is_plain_number("0x"));
        assert!(!is_plain_number(""));
        assert!(!is_plain_number("-"), "a sign with no digits");
        assert!(!is_plain_number("()"), "brackets with nothing in them");
        assert!(
            !is_plain_number("((1))"),
            "one pair, not any number of them"
        );
        assert!(!is_plain_number("(A|B)"), "still an expression");
        assert!(!is_plain_number("OTHER"));
    }
}
