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
        // A claim is backed by a row in any of the logs the request cites.
        let mut rows = String::new();
        for log in &resolution.logs {
            match std::fs::read_to_string(reports.join(log)) {
                Ok(text) => rows.push_str(&text),
                Err(_) => problems.push(format!("{log} cannot be read")),
            }
        }
        if !rows.is_empty() {
            let claims = unbacked(&resolution.outcome, &rows);
            let named = |never: bool| {
                claims
                    .iter()
                    .filter_map(|c| match c {
                        Unbacked::Absent(claim) if !never => Some(claim.as_str()),
                        Unbacked::NeverBound(claim) if never => Some(claim.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            let (absent, never) = (named(false), named(true));
            if !absent.is_empty() {
                problems.push(format!("no row holds {absent}"));
            }
            if !never.is_empty() {
                problems.push(format!("never bound {never}"));
            }
        }
        if !problems.is_empty() {
            refile += 1;
            println!("{}", resolution.id);
            for problem in problems {
                println!("  {problem}");
            }
        }
    }
    println!(
        "{} resolution(s); {refile} with claims no row backs - file them again (D840)",
        found.len()
    );
    Ok(())
}
