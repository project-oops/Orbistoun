//! ABI constants, harvested from FreeBSD headers.
//!
//! # Why these are read rather than remembered
//!
//! The naming loop needs *names*, and the harvest was scoped to them - `Symbol.map` files,
//! no constants, deliberately (`docs/REFERENCES.md`). That was right while the work was
//! naming. Implementing needs numbers, and the gap cost three answers in one session before
//! anyone noticed: `SIGPIPE` was recovered from a guest's own call argument, a `sysctl` MIB
//! was recorded with its meaning left open, and `errno` was left unset because `ENOENT`'s
//! value was not derivable from anything lawful here (D350, D352).
//!
//! Sockets make it acute. `socket(AF_INET, SOCK_STREAM, 0)` cannot be mapped onto a host
//! socket without knowing what those are, and a wrong value creates the wrong kind of
//! socket: a silent, late failure of exactly the shape principle 3 exists to stop.
//!
//! # What is taken, and what is not
//!
//! `#define NAME <number>` and the trailing comment, from named headers. **No function
//! bodies, no structure layouts, no expressions.** A `#define` of a bare number is an
//! interface fact - it *is* the ABI - which is the same category as a symbol name and the
//! reason this is within the provenance boundary rather than an exception to it.
//!
//! A definition whose value is itself an expression - `#define X (Y | Z)` - is deliberately
//! skipped rather than evaluated. Evaluating one means reproducing a decision somebody made
//! about how to compose it, which is the line this does not cross.
//!
//! # They are FreeBSD's numbers, not the target's
//!
//! The target platform is FreeBSD-derived, which is why these are worth having and also why
//! they are not facts about it. The rendered file says so at the top. Each is `published`
//! about FreeBSD and `assumed` about a guest, and a guest passing a value that disagrees is
//! what would show it.

use std::path::Path;

use anyhow::{Context as _, Result};

/// One harvested definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Constant {
    /// The name, exactly as the header spells it.
    pub(crate) name: String,
    /// The value, as written - hexadecimal stays hexadecimal, because a reader comparing
    /// this against the header should not have to convert anything.
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
/// **A list rather than a directory walk.** Every entry is one somebody needed, and a walk
/// would pull in hundreds of headers nobody has looked at - making the file large, the
/// provenance question vaguer, and the answer to "why is this constant here" worse.
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
/// Pure, so the parsing is testable without a checkout - which matters here, because the
/// checkout is sparse and the thing most likely to go wrong is a header being absent rather
/// than malformed.
///
/// **First definition wins.** Headers guard alternatives behind `#if`, and this does not
/// evaluate preprocessor conditions; taking the first keeps the choice deterministic and
/// makes a disagreement visible as a wrong value rather than as an unstable file.
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
/// Starts upper case, so a lower-case internal macro is left alone, and no parentheses - a
/// function-like macro is code, not a constant.
///
/// **The body may be lower case, and that was not always so.** The rule was upper case
/// throughout, which is right for every header harvested until `sys/sys/syscall.h` - where
/// every name is `SYS_read`, `SYS_write`, `SYS_getpid`. It harvested exactly one constant
/// from a header of six hundred, and the count was the only thing that said so (D378).
fn is_constant_name(token: &str) -> bool {
    !token.is_empty()
        && token.starts_with(|c: char| c.is_ascii_uppercase())
        && token.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Whether a value is a bare number rather than an expression.
///
/// Anything composed - `(A | B)`, a cast, a reference to another name - is skipped. Working
/// out what it evaluates to means reproducing a decision, which is the line (see the module
/// documentation).
///
/// # Brackets and a sign are still a bare number
///
/// `sys/sys/event.h` writes every filter as `#define EVFILT_READ (-1)`, which is what a
/// header does with a negative constant so that `EVFILT_READ - 1` cannot mean something
/// else. Requiring bare digits skipped **all fifteen filters** and kept `EVFILT_SYSCOUNT`,
/// which is the one number of the set that is not a filter - so the section looked harvested
/// and named nothing a guest can ask for.
///
/// That is the third time a rule about spelling silently dropped what mattered: the
/// upper-case rule took one constant of six hundred from `syscall.h` (D378), C octal made a
/// whole table unparseable (D374), and this. **The count is the only thing that ever says
/// so**, which is why the harvest prints one (D385).
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
/// **One pair, not any number.** Nested brackets are how an expression is written, and this
/// only exists for the single pair a header puts around a negative number.
fn strip_brackets(token: &str) -> &str {
    token
        .strip_prefix('(')
        .and_then(|inner| inner.strip_suffix(')'))
        .unwrap_or(token)
}

/// A header's number, spelled the way TOML spells it.
///
/// # C octal is not TOML octal
///
/// `S_IFDIR` is `0040000` in `sys/sys/stat.h`, and a **leading zero is how C says octal**.
/// TOML rejects a leading zero outright, so the first file mode harvested made the whole
/// table unparseable - and the failure surfaced as every constant in every section being
/// missing at once, which reads like a build problem rather than like one number (D374).
///
/// So a C octal becomes TOML's `0o` form. The value is the same number in the same base, and
/// a reader comparing it against the header still sees octal - which is why it is not simply
/// converted to decimal.
///
/// Hexadecimal and decimal pass through untouched: both spellings already mean the same in
/// each language.
fn as_toml_number(token: &str) -> String {
    // The brackets a header puts around a negative number are C's, not TOML's, and TOML has
    // no use for them - `-1` is `-1` in both.
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
/// Kept because it is the header's own description and worth far more than anything that
/// could be written here about a number.
///
/// **A comment that runs onto the next line is marked, not silently cut.** Eight
/// definitions in these headers have one, and a truncated sentence reads as a complete one.
/// `AT_EACCESS` would be described as *"Check access using effective user"*, which stops
/// exactly where it stops meaning something. The marker costs one character and makes the
/// difference visible.
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

/// Reads every header and renders the data file.
///
/// # Errors
///
/// When the checkout is not a directory, or a header named above is missing. **Named rather
/// than skipped**, because a sparse checkout lacking one silently yields a smaller table,
/// and a constant absent for that reason looks exactly like one that was never defined.
/// The revision a checkout is actually at.
///
/// # Why this is asked rather than accepted
///
/// The first version took the revision as an argument and stamped it into the header. That
/// makes the header a **claim**, and the gate that re-derives the file could not tell a true
/// claim from a false one: regenerating with whatever the file says produces a file saying
/// the same thing, so editing the header to name a different revision passed (D354).
///
/// Deriving it closes that. The header then says what the harvest actually ran against, and
/// a hand-edited one differs from a regeneration - which is what the gate is looking at.
///
/// # Errors
///
/// When the checkout is not a git repository or `git` cannot be run. **Refused rather than
/// falling back to "unknown"**, because a table that cannot say where it came from is
/// exactly the thing this whole file exists to prevent.
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
/// Enough to see what kind of thing was skipped without burying the counts under a header
/// full of function-like macros.
const SKIPPED_SHOWN: usize = 8;

/// Says what a section did not take, and names some of it.
///
/// # Why this exists
///
/// Three separate times a rule about *spelling* has silently taken the wrong set: the
/// upper-case rule left one constant of six hundred in `syscall.h` (D378), C octal made the
/// whole table unparseable (D374), and requiring bare digits took none of the fifteen event
/// filters while keeping the one number of the set that is not a filter (D385).
///
/// Every time, **the count was the only thing that said so**, and twice it was noticed weeks
/// later by somebody looking for a constant that should have been there. A `#define` whose
/// name qualifies and whose value does not is a decision this makes on its own, and a
/// decision nobody can see is one nobody can check.
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
/// A name that qualifies with a value that does not. Names that do not qualify are not
/// listed: a lower-case internal macro is not a constant anybody was looking for, and
/// listing them would bury the ones that are.
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

    /// **Hexadecimal stays hexadecimal.**
    ///
    /// A reader checking this against the header should not have to convert anything, and
    /// `SOL_SOCKET` being `0xffff` is exactly the value somebody would misremember.
    #[test]
    fn a_hexadecimal_value_is_not_normalised() {
        let found = extract("#define\tSOL_SOCKET\t0xffff\t\t/* options for socket level */\n");
        assert_eq!(found[0].value, "0xffff");
    }

    /// **A negative constant in brackets is still a bare number.**
    ///
    /// Every filter in `sys/sys/event.h` is written this way, and requiring bare digits
    /// harvested none of them while keeping `EVFILT_SYSCOUNT` - so the section existed and
    /// named nothing a guest can ask for (D385).
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

    /// **An expression is skipped, never evaluated.**
    ///
    /// Working out what `(A | B)` comes to means reproducing a decision somebody made about
    /// how to compose it. The number is not the point; where it came from is.
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

    /// **An unterminated comment is marked rather than silently truncated.**
    ///
    /// A cut sentence reads as a whole one, which is the plausible-output problem at the
    /// scale of a comment. Found by comparing two implementations of this harvest against
    /// each other (D353).
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

    /// **First definition wins, and the choice is deterministic.**
    ///
    /// Headers guard alternatives behind `#if`, and this does not evaluate preprocessor
    /// conditions. Taking the first makes a disagreement show up as a wrong value somebody
    /// can find, rather than as a file that changes between runs.
    #[test]
    fn the_first_definition_of_a_name_is_the_one_kept() {
        let found = extract("#define SIGPIPE 13\n#define SIGPIPE 99\n");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].value, "13");
    }

    /// A name must *start* upper case, and may go on however it likes.
    ///
    /// **The body used to have to be upper case too**, and that silently dropped an entire
    /// header: every syscall is `SYS_read`, `SYS_write`, `SYS_getpid`, so the rule harvested
    /// one constant out of six hundred and the only thing that said so was the total going up
    /// by one (D378).
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

    /// **What is skipped is named**, which is the whole of D385's tooling half.
    ///
    /// The failure this protects against is a section that looks harvested and is not: a
    /// header of fifteen filters that yields one number, with nothing anywhere saying so.
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
    /// **`-1` and `(1)` are numbers now and were not.** They were asserted *not* to be, which
    /// is what made every event filter invisible - the assertion was written from the rule
    /// rather than from what a header contains, so it protected the bug (D385).
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
