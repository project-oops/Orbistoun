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
    /// A function no row names. Weaker than the other two: a probe may label a call's rows by a
    /// short key of its own (`ime-dialog|abort-rc`) rather than by the function.
    Unnamed(String),
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
    let mut logs: Vec<String> = Vec::new();
    for line in block {
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
    logs
}

/// The claims of `outcome` the rows of `log` do not back, in the order the outcome makes them.
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
        .collect()
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
