//! Checking a resolution on the bus against the log it cites (D840).
//!
//! A request is resolved by an outcome written in prose, and ingested only for what the rows of
//! the log it cites show. Every status code and every function the outcome names is looked for
//! among the `OBS|` rows: a claim with no row, or a function whose only rows say it never bound,
//! is not a measurement, and the request is filed again.

/// A resolved request: its id, its outcome text, and the logs it cites.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    /// `REQ-...`.
    pub id: String,
    /// What the resolver wrote under `outcome`.
    pub outcome: String,
    /// The report paths the request cites, relative to the resolver's repository.
    pub logs: Vec<String>,
}

/// A claim of an outcome that no row backs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unbacked {
    /// A status code no `OBS|` row holds.
    Absent(String),
    /// A function whose only rows say it never bound.
    NeverBound(String),
    /// A function no row names. Weaker than the others: a probe may label a call's rows by a short
    /// key of its own (`ime-dialog|abort-rc`) rather than by the function.
    Unnamed(String),
    /// A section the outcome cites that reports measurements after its own set-up failed, as
    /// `section: the failing row, then the first measurement after it`.
    AfterFailedSetUp(String),
    /// An `OBS|` row the outcome quotes that no cited log holds, word for word.
    QuotedRowAbsent(String),
}

impl Unbacked {
    /// Whether the claim alone shows the resolution is not a measurement: a code with no row, or
    /// a function that never bound.
    #[must_use]
    pub fn is_decisive(&self) -> bool {
        !matches!(self, Self::Unnamed(_))
    }
}

/// Where a bus entry's fields begin.
const FIELD: &str = "- **";

/// The directories a resolver's report lives under, as a request cites it.
const REPORT_DIRS: [&str; 2] = ["reports/hardware/", "build/reports/"];

/// Every resolved request `asker` filed, in the order the worklog holds them.
#[must_use]
pub fn resolutions(worklog: &str, asker: &str) -> Vec<Resolution> {
    let mut found = Vec::new();
    let mut block: Vec<&str> = Vec::new();
    for line in worklog.lines().chain(std::iter::once("### ")) {
        if line.starts_with("### ") {
            if let Some(resolution) = read_request(&block, asker) {
                found.push(resolution);
            }
            block.clear();
        }
        block.push(line);
    }
    found
}

/// The resolution one request block holds, if it is resolved and `asker` filed it.
fn read_request(block: &[&str], asker: &str) -> Option<Resolution> {
    let id = block
        .first()?
        .strip_prefix("### ")?
        .split_whitespace()
        .next()?;
    if !id.starts_with("REQ-") {
        return None;
    }
    let field = |name: &str| {
        let marker = format!("{FIELD}{name}:**");
        block
            .iter()
            .find_map(|line| line.strip_prefix(marker.as_str()).map(str::trim))
    };
    if !field("status")?.starts_with("RESOLVED") || !field("from")?.starts_with(asker) {
        return None;
    }
    Some(Resolution {
        id: id.to_owned(),
        outcome: outcome(block),
        logs: cited_logs(block),
    })
}

/// The outcome's text: its own line and what follows it, up to the next field that is the
/// request's rather than the resolver's.
fn outcome(block: &[&str]) -> String {
    /// The fields a resolver writes beside its outcome.
    const RESOLVER: [&str; 4] = ["outcome", "logs", "sweep", "resolved"];
    let mut text = String::new();
    let mut inside = false;
    for line in block {
        if let Some(rest) = line.strip_prefix(FIELD) {
            let name = rest.split(':').next().unwrap_or_default();
            inside = if name == "outcome" {
                true
            } else {
                inside && RESOLVER.contains(&name)
            };
        }
        if inside {
            text.push_str(line);
            text.push('\n');
        }
    }
    text
}

/// The report paths a request block names, once each.
fn cited_logs(block: &[&str]) -> Vec<String> {
    /// The fields a resolver writes beside its outcome.
    const RESOLVER: [&str; 4] = ["outcome", "logs", "sweep", "resolved"];
    let mut logs: Vec<String> = Vec::new();
    let mut inside = false;
    for line in block {
        if let Some(rest) = line.strip_prefix(FIELD) {
            let name = rest.split(':').next().unwrap_or_default();
            inside = RESOLVER.contains(&name);
        }
        if inside {
            for dir in REPORT_DIRS {
                for (at, _) in line.match_indices(dir) {
                    let path: String = line[at..]
                        .chars()
                        .take_while(|c| c.is_ascii_alphanumeric() || "/._-".contains(*c))
                        .collect();
                    let path = path.trim_end_matches('.');
                    let is_report = std::path::Path::new(path).extension().is_some_and(|e| {
                        e.eq_ignore_ascii_case("log") || e.eq_ignore_ascii_case("txt")
                    });
                    if is_report && !logs.iter().any(|known| known == path) {
                        logs.push(path.to_owned());
                    }
                }
            }
        }
    }
    logs
}

/// The claims of `outcome` the rows of `log` do not back, in the order the outcome makes them,
/// then each section it cites that measured after its own set-up failed (`after_failed_set_up`),
/// then each row it quotes that the log does not hold (`quoted_rows_absent`).
///
/// A claim is a status code (`0x8` and seven more hex digits) or a platform function (`sce`
/// followed by a capital). A code is backed by any row holding it; a function by any row naming
/// it that is not about binding it, or by one saying it bound.
#[must_use]
pub fn unbacked(outcome: &str, log: &str) -> Vec<Unbacked> {
    let rows: Vec<Vec<&str>> = log
        .lines()
        .filter_map(|line| line.find("OBS|").map(|at| line[at..].split('|').collect()))
        .collect();
    let mut claims: Vec<&str> = Vec::new();
    for word in outcome.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
        if (is_code(word) || is_function(word)) && !claims.contains(&word) {
            claims.push(word);
        }
    }
    claims
        .into_iter()
        .filter_map(|claim| {
            if is_code(claim) {
                let held = rows
                    .iter()
                    .any(|row| row.iter().any(|f| f.eq_ignore_ascii_case(claim)));
                return (!held).then(|| Unbacked::Absent(claim.to_owned()));
            }
            let naming: Vec<&Vec<&str>> = rows.iter().filter(|row| row.contains(&claim)).collect();
            if naming.is_empty() {
                Some(Unbacked::Unnamed(claim.to_owned()))
            } else if naming.iter().all(|row| never_bound(row, claim)) {
                Some(Unbacked::NeverBound(claim.to_owned()))
            } else {
                None
            }
        })
        .chain(after_failed_set_up(outcome, &rows))
        .chain(quoted_rows_absent(outcome, log))
        .collect()
}

/// Each `OBS|` row `outcome` quotes, from `OBS|` to the end of its line, that no line of `log`
/// holds: a quotation of a row the log does not have is not evidence of anything.
fn quoted_rows_absent(outcome: &str, log: &str) -> Vec<Unbacked> {
    let held: std::collections::HashSet<&str> = log
        .lines()
        .filter_map(|line| line.find("OBS|").map(|at| line[at..].trim_end()))
        .collect();
    outcome
        .lines()
        .filter_map(|line| line.find("OBS|").map(|at| line[at..].trim_end()))
        // A row quoted inline ends at its closing backtick.
        .map(|row| row.split('`').next().unwrap_or(row))
        .filter(|row| !held.contains(row))
        .map(|row| Unbacked::QuotedRowAbsent(row.to_owned()))
        .collect()
}

/// Each section `outcome` cites (`000-boot/ge-indx-offset`) whose rows, in order, hold a failed
/// set-up - a call answering -1 - followed by a measurement that is not itself set-up. Nothing
/// after a set-up that failed ran, so such a measurement is not one; a section that stops at its
/// failure is a `not-possible`, and is not reported. A function that did not bind is not a failed
/// set-up by itself: ue01's section measured `CreateEvent` beside a `PostEvent` that never bound.
fn after_failed_set_up(outcome: &str, rows: &[Vec<&str>]) -> Vec<Unbacked> {
    let mut sections: Vec<&str> = Vec::new();
    for word in outcome.split(|c: char| !(c.is_ascii_alphanumeric() || "/-_".contains(c))) {
        if is_section(word) && !sections.contains(&word) {
            sections.push(word);
        }
    }
    sections
        .into_iter()
        .filter_map(|section| {
            let mut failed: Option<(&str, &str)> = None;
            for row in rows {
                let [_, "measure", at, scope, key, value, ..] = row.as_slice() else {
                    continue;
                };
                if *at != section {
                    continue;
                }
                match failed {
                    None if failed_set_up(scope, key, value) => failed = Some((key, value)),
                    Some((why, was)) if !is_set_up(key) => {
                        return Some(Unbacked::AfterFailedSetUp(format!(
                            "{section}: {why} {was}, then {key}"
                        )));
                    }
                    _ => {}
                }
            }
            None
        })
        .collect()
}

/// A probe section's name: three digits, a dash, then a path (`000-boot/ge-indx-offset`).
fn is_section(word: &str) -> bool {
    let bytes = word.as_bytes();
    bytes.len() > 5
        && bytes[..3].iter().all(u8::is_ascii_digit)
        && bytes[3] == b'-'
        && word.contains('/')
}

/// Whether a row's key names set-up: binding a function, or a call's return code.
fn is_set_up(key: &str) -> bool {
    ["bound", "resolved", "status", "how", "rc", "errno"]
        .iter()
        .any(|word| key == *word || key.ends_with(&format!("-{word}")))
}

/// Whether a set-up row says it failed: a call answered -1.
fn failed_set_up(scope: &str, key: &str, value: &str) -> bool {
    !scope.starts_with("arm")
        && (key == "rc" || key.ends_with("-rc"))
        && matches!(value, "0xffffffff" | "0xffffffffffffffff")
}

/// A status code as the platform writes one: `0x8` and seven more hex digits.
fn is_code(word: &str) -> bool {
    word.len() == 10 && word.starts_with("0x8") && word[2..].chars().all(|c| c.is_ascii_hexdigit())
}

/// A platform function's name: `sce` and a capital.
fn is_function(word: &str) -> bool {
    word.strip_prefix("sce")
        .and_then(|rest| rest.chars().next())
        .is_some_and(|c| c.is_ascii_uppercase())
}

/// Whether a row naming `function` is about binding it and says it did not bind: `bound 0x0`,
/// `resolved 0x0`, the `status` that follows them, or `unresolvable`.
fn never_bound(row: &[&str], function: &str) -> bool {
    if row.contains(&"unresolvable") {
        return true;
    }
    let after = row
        .iter()
        .position(|f| *f == function)
        .map_or(&[][..], |at| &row[at + 1..]);
    match after {
        [key, value, ..] if *key == "bound" || *key == "resolved" => *value == "0x0",
        [key, ..] => *key == "status",
        [] => false,
    }
}
