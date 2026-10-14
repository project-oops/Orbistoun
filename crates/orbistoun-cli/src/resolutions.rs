//! Whether the bus's resolutions are measurements: `resolutions`.
//!
//! The bus step of the loop (`docs/THE_LOOP.md`): every resolved request orbistoun filed, its
//! outcome checked against the rows of the log it cites. A resolution whose claims no row backs,
//! or whose log cannot be read, is filed again rather than ingested (D840).

use anyhow::{Context, Result};
use orbistoun_probe::resolution::{Unbacked, resolutions, unbacked};
use std::path::Path;

/// Prints, for each resolution `asker` filed from `since` on, what its cited logs do not back.
pub(crate) fn cmd_resolutions(
    worklog: &Path,
    reports: &Path,
    asker: &str,
    since: Option<&str>,
) -> Result<()> {
    let text = std::fs::read_to_string(worklog)
        .with_context(|| format!("reading the worklog {}", worklog.display()))?;
    let found: Vec<_> = resolutions(&text, asker)
        .into_iter()
        .filter(|r| since.is_none_or(|since| r.id.as_str() >= since))
        .collect();
    let mut refile = 0;
    for resolution in &found {
        let mut problems: Vec<String> = Vec::new();
        if resolution.logs.is_empty() {
            problems.push("cites no log".to_owned());
        }
        // A claim is backed by a row in any of the logs the request cites; a set-up is judged
        // within each.
        let mut texts: Vec<String> = Vec::new();
        for log in &resolution.logs {
            match std::fs::read_to_string(reports.join(log)) {
                Ok(text) => texts.push(text),
                Err(_) => problems.push(format!("{log} cannot be read")),
            }
        }
        let mut notes: Vec<String> = Vec::new();
        if !texts.is_empty() {
            let logs: Vec<&str> = texts.iter().map(String::as_str).collect();
            let claims = unbacked(&resolution.outcome, &logs);
            let of = |kind: fn(&Unbacked) -> Option<&str>| {
                claims
                    .iter()
                    .filter_map(kind)
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            let absent = of(|c| match c {
                Unbacked::Absent(claim) => Some(claim),
                _ => None,
            });
            let never = of(|c| match c {
                Unbacked::NeverBound(claim) => Some(claim),
                _ => None,
            });
            let unnamed = of(|c| match c {
                Unbacked::Unnamed(claim) => Some(claim),
                _ => None,
            });
            if !absent.is_empty() {
                problems.push(format!("no row holds {absent}"));
            }
            if !never.is_empty() {
                problems.push(format!("never bound {never}"));
            }
            for claim in &claims {
                match claim {
                    Unbacked::AfterFailedSetUp(what) => {
                        problems.push(format!("measured after a failed set-up - {what}"));
                    }
                    Unbacked::QuotedRowAbsent(row) => {
                        problems.push(format!("quotes a row its log does not hold: {row}"));
                    }
                    _ => {}
                }
            }
            if !unnamed.is_empty() {
                notes.push(format!(
                    "no row names {unnamed} (rows may use a key of their own)"
                ));
            }
        }
        if !problems.is_empty() {
            refile += 1;
        }
        if !problems.is_empty() || !notes.is_empty() {
            println!("{}", resolution.id);
            for line in problems.iter().chain(&notes) {
                println!("  {line}");
            }
        }
    }
    println!(
        "{} resolution(s); {refile} with claims no row backs - file them again (D840)",
        found.len()
    );
    Ok(())
}
