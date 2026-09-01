//! The test corpus: a manifest of sources, fetched into `titles/`, ready to run and record.
//!
//! # What this is for
//!
//! orbistoun's one measure of progress is whether a real guest gets further than it did last
//! week (D042). That needs guests, and the honest ones are third-party homebrew nobody here
//! wrote. This crate turns a **tracked manifest** - `corpus/sources.toml`, metadata only - into
//! **gitignored guest bytes** under `titles/`, so the corpus is reproducible from a checkout
//! without ever committing somebody else's binary.
//!
//! # The provenance line this holds
//!
//! The manifest is metadata: a name, where the bytes come from, a licence, a citation, and a
//! per-asset hash. The bytes themselves are never tracked - `titles/` is gitignored and the
//! provenance guard fails CI if anything of that shape is committed (D042). Downloading is not
//! redistributing; pinning by hash is what makes "reproducible on any machine" true past a month
//! (a moving branch is not). A `github-release` asset is verified against its pin every fetch; a
//! `local` asset is a dev artifact snapshotted from a sibling checkout until it has a release of
//! its own.
//!
//! # What lives here, and what does not
//!
//! This crate holds the manifest and the fetch/pin logic and nothing else - no run, no record.
//! The CLI runs each fetched guest through the ordinary `run` path, which records to `compat/`
//! on its own. Keeping the two apart is D034: the crate is the logic, the shim is the shim.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// A `github-release` source: assets pinned by hash and verified every fetch.
pub const KIND_GITHUB_RELEASE: &str = "github-release";
/// A `local` source: bytes copied from a sibling checkout, re-snapshotted each fetch. For a
/// project of ours that has no published release yet; carries a `todo` to migrate it.
pub const KIND_LOCAL: &str = "local";

/// The whole manifest - every source the corpus knows about.
#[derive(Debug, Default, Deserialize, Serialize)]
pub struct Manifest {
    /// Every source, in the order they appear in the file.
    #[serde(default)]
    pub source: Vec<Source>,
}

/// One source: where a set of guests comes from, and the terms under which they were obtained.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Source {
    /// The directory name under `titles/` this source's guests land in.
    pub name: String,
    /// `github-release` or `local`; see the `KIND_*` constants.
    pub kind: String,
    /// `github-release`: `owner/repo`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    /// `github-release`: the pinned release tag (a tag, never a branch - D042).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    /// `local`: a path to the source's build output, relative to the orbistoun repo root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// The licence the assets are obtained under. Recorded per D042; downloading is not
    /// redistributing, so this is a note, not a gate.
    pub licence: String,
    /// Where this source is, for a person to check.
    pub cite: String,
    /// A standing note, e.g. "migrate to a github-release once one is published".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub todo: Option<String>,
    /// The assets to fetch. A `github-release` asset carries a pinned `sha256`; a `local` one
    /// carries the hash of the last snapshot.
    #[serde(default)]
    pub asset: Vec<Asset>,
}

/// One asset within a source.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Asset {
    /// The asset's filename, as the release names it or as it sits in the local directory.
    pub file: String,
    /// The pinned hash. `None` until the first fetch pins it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

/// What became of one asset in a sync.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// Downloaded and pinned for the first time; the manifest gained a hash.
    PinnedNew,
    /// Downloaded and matched its existing pin.
    Verified,
    /// Already present on disk and matching its pin; nothing fetched.
    Reused,
    /// A `local` dev artifact, copied and its hash refreshed (never a failure on change).
    LocalSnapshot,
    /// Downloaded but did not match its pin - the release moved under a fixed tag. An error.
    Mismatch {
        /// The hash the manifest pinned, which the fetched bytes failed to match.
        expected: String,
    },
}

impl State {
    /// A mismatch is the one state a sync must stop on: a pinned tag whose bytes changed.
    pub fn is_mismatch(&self) -> bool {
        matches!(self, State::Mismatch { .. })
    }
}

/// The result of syncing one asset.
#[derive(Debug, Clone)]
pub struct Outcome {
    /// The source this asset belongs to.
    pub source: String,
    /// The asset's filename.
    pub file: String,
    /// The title id this guest records under - its file stem, which becomes the parent
    /// directory so `run`'s automatic recording keys `compat/<stem>.toml` correctly.
    pub stem: String,
    /// Where the bytes now live: `titles/<source>/<stem>/<file>`.
    pub path: PathBuf,
    /// The SHA-256 the fetched bytes actually hashed to.
    pub sha256: String,
    /// How many bytes were fetched.
    pub bytes: u64,
    /// What the fetch amounted to - pinned, verified, cached, snapshotted, or a mismatch.
    pub state: State,
}

impl Source {
    /// The download URL for one asset of a `github-release` source.
    pub fn asset_url(&self, file: &str) -> Result<String> {
        let repo = self
            .repo
            .as_deref()
            .with_context(|| format!("source {:?} is {} but has no repo", self.name, self.kind))?;
        let tag = self
            .tag
            .as_deref()
            .with_context(|| format!("source {:?} is {} but has no tag", self.name, self.kind))?;
        Ok(format!(
            "https://github.com/{repo}/releases/download/{tag}/{file}"
        ))
    }

    /// The title id an asset records under: its file stem. `elfldr_v0.26.elf` -> `elfldr_v0.26`.
    pub fn stem(file: &str) -> String {
        Path::new(file)
            .file_stem()
            .map_or_else(|| file.to_owned(), |s| s.to_string_lossy().into_owned())
    }

    /// The bare filename an asset is stored under (a local `path` asset may carry subdirs).
    fn base(file: &str) -> String {
        Path::new(file)
            .file_name()
            .map_or_else(|| file.to_owned(), |n| n.to_string_lossy().into_owned())
    }

    /// Where one asset lands under the corpus root: `titles/<source>/<stem>/<file>`.
    ///
    /// Each guest gets its own directory because `run` keys a compatibility record by the
    /// containing directory's name (a title is a directory holding its material), so a flat
    /// layout would make every guest overwrite one record.
    pub fn target(&self, titles_root: &Path, file: &str) -> PathBuf {
        titles_root
            .join(&self.name)
            .join(Self::stem(file))
            .join(Self::base(file))
    }

    /// Fetch every asset of this source into `titles_root`, verifying or pinning each.
    ///
    /// `repo_root` is where a `local` source's relative `path` is resolved from. Mutates each
    /// asset's `sha256` in place when a fetch pins or refreshes it, so the caller can persist the
    /// manifest afterwards. Returns one [`Outcome`] per asset. A [`State::Mismatch`] is returned,
    /// not raised - the caller decides whether one bad pin should stop the whole sync.
    pub fn sync(
        &mut self,
        repo_root: &Path,
        titles_root: &Path,
        client: &reqwest::blocking::Client,
    ) -> Result<Vec<Outcome>> {
        let name = self.name.clone();
        let kind = self.kind.clone();
        let path = self.path.clone();
        let repo = self.repo.clone();
        let tag = self.tag.clone();
        let mut outcomes = Vec::with_capacity(self.asset.len());

        for asset in &mut self.asset {
            let stem = Self::stem(&asset.file);
            let target = titles_root
                .join(&name)
                .join(&stem)
                .join(Self::base(&asset.file));

            // A github asset already on disk and matching its pin needs no network at all.
            if kind == KIND_GITHUB_RELEASE {
                if let (Some(pin), true) = (asset.sha256.as_deref(), target.exists()) {
                    let existing = std::fs::read(&target)
                        .with_context(|| format!("reading {}", target.display()))?;
                    if hash_hex(&existing) == pin {
                        outcomes.push(Outcome {
                            source: name.clone(),
                            file: asset.file.clone(),
                            stem,
                            path: target,
                            sha256: pin.to_owned(),
                            bytes: existing.len() as u64,
                            state: State::Reused,
                        });
                        continue;
                    }
                }
            }

            let bytes = match kind.as_str() {
                KIND_GITHUB_RELEASE => {
                    let repo = repo
                        .as_deref()
                        .with_context(|| format!("source {name:?} is {kind} but has no repo"))?;
                    let tag = tag
                        .as_deref()
                        .with_context(|| format!("source {name:?} is {kind} but has no tag"))?;
                    let url = format!(
                        "https://github.com/{repo}/releases/download/{tag}/{}",
                        asset.file
                    );
                    let resp = client
                        .get(&url)
                        .send()
                        .with_context(|| format!("fetching {url}"))?
                        .error_for_status()
                        .with_context(|| format!("fetching {url}"))?;
                    resp.bytes()
                        .with_context(|| format!("reading the body of {url}"))?
                        .to_vec()
                }
                KIND_LOCAL => {
                    let rel = path
                        .as_deref()
                        .with_context(|| format!("source {name:?} is local but has no path"))?;
                    let src = repo_root.join(rel).join(&asset.file);
                    std::fs::read(&src).with_context(|| {
                        format!("reading local asset {} - is {name} built?", src.display())
                    })?
                }
                other => bail!("source {name:?} has unknown kind {other:?}"),
            };

            let sha = hash_hex(&bytes);
            write_atomic(&target, &bytes)
                .with_context(|| format!("writing {}", target.display()))?;

            let state = if kind == KIND_LOCAL {
                State::LocalSnapshot
            } else {
                match asset.sha256.as_deref() {
                    Some(pin) if pin == sha => State::Verified,
                    Some(pin) => State::Mismatch {
                        expected: pin.to_owned(),
                    },
                    None => State::PinnedNew,
                }
            };

            // Pin a new hash and refresh a local snapshot; leave a verified pin; never overwrite
            // a pin the bytes failed to match - that is the caller's to resolve.
            if matches!(state, State::PinnedNew | State::LocalSnapshot) {
                asset.sha256 = Some(sha.clone());
            }

            outcomes.push(Outcome {
                source: name.clone(),
                file: asset.file.clone(),
                stem,
                path: target,
                sha256: sha,
                bytes: bytes.len() as u64,
                state,
            });
        }
        Ok(outcomes)
    }
}

/// An HTTP client for fetching release assets. Built here rather than in the CLI so `reqwest`
/// stays a dependency of this crate and not of the binary (the crate boundary is the point).
/// Carries a user-agent because GitHub prefers one on release-asset downloads.
pub fn client() -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .user_agent(concat!("orbistoun-corpus/", env!("CARGO_PKG_VERSION")))
        .build()
        .context("building an HTTP client")
}

/// SHA-256 of some bytes, lowercase hex - the pin format.
pub fn hash_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut s = String::with_capacity(digest.len() * 2);
    for b in digest {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0x0f) as usize] as char);
    }
    s
}

/// Read a manifest from `corpus/sources.toml`.
pub fn load(path: &Path) -> Result<Manifest> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}

/// Write a manifest back, so freshly pinned hashes persist.
pub fn save(path: &Path, manifest: &Manifest) -> Result<()> {
    let text = toml::to_string_pretty(manifest).context("serialising the manifest")?;
    std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))
}

/// Write bytes to `target`, creating parent directories, via a temp file and rename so a killed
/// fetch never leaves a truncated guest that a later run would treat as real.
fn write_atomic(target: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let tmp = target.with_extension("partial");
    std::fs::write(&tmp, bytes).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, target).with_context(|| format!("renaming into {}", target.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(kind: &str) -> Source {
        Source {
            name: "src".into(),
            kind: kind.into(),
            repo: Some("owner/repo".into()),
            tag: Some("t".into()),
            path: None,
            licence: "x".into(),
            cite: "y".into(),
            todo: None,
            asset: vec![],
        }
    }

    #[test]
    fn a_stem_becomes_the_title_directory() {
        assert_eq!(Source::stem("elfldr_v0.26.elf"), "elfldr_v0.26");
        assert_eq!(Source::stem("etaHEN_2.5B.bin"), "etaHEN_2.5B");
    }

    #[test]
    fn a_github_asset_url_is_the_release_download_path() {
        assert_eq!(
            source(KIND_GITHUB_RELEASE).asset_url("a.elf").unwrap(),
            "https://github.com/owner/repo/releases/download/t/a.elf"
        );
    }

    #[test]
    fn the_target_is_one_directory_per_guest() {
        let t = source(KIND_GITHUB_RELEASE).target(Path::new("titles"), "elfldr_v0.26.elf");
        let got: PathBuf = t.components().collect();
        let want: PathBuf = ["titles", "src", "elfldr_v0.26", "elfldr_v0.26.elf"]
            .iter()
            .collect();
        assert_eq!(got, want);
    }

    #[test]
    fn a_manifest_round_trips_through_toml() {
        let mut m = Manifest::default();
        m.source.push(source(KIND_GITHUB_RELEASE));
        let text = toml::to_string_pretty(&m).unwrap();
        let back: Manifest = toml::from_str(&text).unwrap();
        assert_eq!(back.source.len(), 1);
        assert_eq!(back.source[0].name, "src");
    }
}
