//! The title corpus: `corpus list`, `sync` and `run`.

use crate::run::cmd_run;
use anyhow::Result;

/// `compat list` - every recorded title, furthest first.
/// Show the corpus manifest: every source and whether each asset is pinned yet.
pub(crate) fn cmd_corpus_list(manifest: &std::path::Path) -> Result<()> {
    let m = orbistoun_corpus::load(manifest)?;
    if m.source.is_empty() {
        println!("no sources in {}", manifest.display());
        return Ok(());
    }
    for src in &m.source {
        println!("{} ({}) -> {}", src.name, src.kind, src.target.label());
        println!("  cite {}", src.cite);
        if let Some(todo) = &src.todo {
            println!("  TODO {todo}");
        }
        for a in &src.asset {
            let pin = a
                .sha256
                .as_deref()
                .map_or("unpinned", |h| &h[..h.len().min(12)]);
            println!("    {:<40} {pin}", a.file);
        }
    }
    Ok(())
}

/// Fetch every source's assets into `titles/`, pinning or verifying each by hash.
pub(crate) fn cmd_corpus_sync(
    manifest: &std::path::Path,
    titles: &std::path::Path,
    only: Option<&str>,
) -> Result<()> {
    // A source's relative `path` (and thus a local source) resolves from the repo root, which is
    // where the CLI runs. Kept explicit so the corpus crate does not guess a working directory.
    let root = std::path::Path::new(".");
    let mut m = orbistoun_corpus::load(manifest)?;
    let client = orbistoun_corpus::client()?;
    let mut pinned = false;
    let mut mismatches = 0usize;
    let mut unavailable: Vec<String> = Vec::new();
    for src in &mut m.source {
        if only.is_some_and(|s| s != src.name) {
            continue;
        }
        // **The root the manifest asked for, not always `titles`.** A payload mirror under
        // `titles/` made twenty-five one-file ELFs look like installed titles (D661). The
        // `--titles` argument still names the titles root; the siblings are resolved beside it
        // so a caller overriding one overrides all three consistently.
        let into = beside_target(titles, src.target);
        println!("{}: -> {}", src.name, src.target.label());
        // **One source that cannot be fetched does not stop the rest** - an app nobody has built
        // or released yet is listed so it arrives the moment either exists. It is said, counted and
        // fails the sync at the end, after every source that could come has come.
        let outcomes = match src.sync(root, &into, &client) {
            Ok(outcomes) => outcomes,
            Err(e) => {
                println!("  unavailable {e:#}");
                unavailable.push(src.name.clone());
                continue;
            }
        };
        for o in outcomes {
            let tag = match &o.state {
                orbistoun_corpus::State::PinnedNew => "pinned",
                orbistoun_corpus::State::Verified => "verified",
                orbistoun_corpus::State::Reused => "cached",
                orbistoun_corpus::State::LocalSnapshot => "local",
                orbistoun_corpus::State::Mismatch { .. } => "MISMATCH",
            };
            println!(
                "  {tag:<9} {:<40} {:>9} bytes  {}",
                o.file,
                o.bytes,
                &o.sha256[..o.sha256.len().min(12)]
            );
            if matches!(
                o.state,
                orbistoun_corpus::State::PinnedNew | orbistoun_corpus::State::LocalSnapshot
            ) {
                pinned = true;
            }
            if let orbistoun_corpus::State::Mismatch { expected } = &o.state {
                mismatches += 1;
                println!("    expected {expected}");
            }
        }
    }
    if pinned {
        orbistoun_corpus::save(manifest, &m)?;
        println!("wrote pins into {}", manifest.display());
    }
    if mismatches > 0 {
        anyhow::bail!(
            concat!(
                "{} asset(s) did not match their pin - a fixed tag's bytes changed; ",
                "review before re-pinning"
            ),
            mismatches
        );
    }
    if !unavailable.is_empty() {
        anyhow::bail!(
            "{} source(s) had no origin that answered: {}",
            unavailable.len(),
            unavailable.join(", ")
        );
    }
    Ok(())
}

/// A sibling of the titles root, by name.
///
/// **Derived from the titles root rather than resolved separately**, so `--titles` keeps meaning
/// what it says: point it at a scratch directory and payloads and packages follow it there,
/// instead of one of the three quietly staying in the real data directory (D661).
fn beside(titles: &std::path::Path, name: &str) -> std::path::PathBuf {
    titles
        .parent()
        .map_or_else(|| std::path::PathBuf::from(name), |p| p.join(name))
}

/// The root a source's target names, given the titles root.
///
/// The same mapping `cmd_corpus_sync` uses, in one place so `run` cannot look for a guest
/// somewhere `sync` did not put it.
fn beside_target(titles: &std::path::Path, target: orbistoun_corpus::Target) -> std::path::PathBuf {
    match target {
        orbistoun_corpus::Target::Titles => titles.to_path_buf(),
        orbistoun_corpus::Target::Payloads => beside(titles, orbistoun_paths::dirs::PAYLOADS),
        orbistoun_corpus::Target::Packages => beside(titles, orbistoun_paths::dirs::PACKAGES),
        orbistoun_corpus::Target::Staged => orbistoun_paths::staged_under(titles),
    }
}

/// Sync, then run every guest and record what it reached to `compat/`.
pub(crate) fn cmd_corpus_run(
    manifest: &std::path::Path,
    titles: &std::path::Path,
    only: Option<&str>,
    limit: u64,
    calls: u64,
    profile: Option<&str>,
) -> Result<()> {
    cmd_corpus_sync(manifest, titles, only)?;
    let m = orbistoun_corpus::load(manifest)?;
    for src in &m.source {
        if only.is_some_and(|s| s != src.name) {
            continue;
        }
        for a in &src.asset {
            let path = src.path_for(&beside_target(titles, src.target), &a.file);
            println!();
            println!("=== {} / {} ===", src.name, a.file);
            // The ordinary run path, which records to compat/ on its own. Deliberately no
            // handoff diagnostic: an intervened run is not recorded (D227), so this measures
            // the honest default-entry baseline. When the elfldr handoff becomes the default
            // entry for payload-shaped guests, these records reflect the progress a diagnostic
            // handoff run shows today (see D411).
            cmd_run(&path, limit, calls, profile, (None, None), false)?;
        }
    }
    Ok(())
}
