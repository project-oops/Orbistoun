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

/// Which of orbistoun's roots a source's assets belong under.
///
/// # Why this is per-source and not one root for everything
///
/// Every asset used to land under `titles/`, because that was the only root a corpus knew
/// about. It put twenty-five one-file homebrew ELFs beside installed titles, where a shell
/// listing the library showed them as titles - which they are not. A title is a directory with
/// a `param.json`, an `eboot.bin` and its own filesystem; a payload is one executable somebody
/// runs; a package is something that has not been installed yet. Three kinds, three roots, and
/// the manifest says which (D661).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Target {
    /// Installed titles - a directory each, with their own material.
    #[default]
    Titles,
    /// Raw executables, run directly rather than installed.
    Payloads,
    /// Installable packages, before anything installs them.
    Packages,
}

impl Target {
    /// How to name it in a report.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Titles => "titles",
            Self::Payloads => "payloads",
            Self::Packages => "packages",
        }
    }
}
/// One source: where a set of guests comes from, and the terms under which they were obtained.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Source {
    /// The directory name, under whichever root [`Self::target`] names, this source's guests
    /// land in.
    pub name: String,
    /// Which root those guests belong under. Defaults to `titles` - what every source was
    /// before there was anywhere else to put one.
    #[serde(default)]
    pub target: Target,
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
    /// Origins to try in order, each a local path or a URL, first one that answers wins.
    ///
    /// **An alternative to `repo`/`tag`/`path`, not an addition to them.** A sibling checkout is
    /// the fast path when somebody has one and absent when they do not; a published release is
    /// slower and always there. Listing both lets one manifest serve both cases without a person
    /// editing it, and every origin that failed is reported so a broken fast path does not hide
    /// behind a working slow one (D664).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<String>,
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

    /// Where one asset lands: `<root>/<source>/<stem>/<file>`, with the root chosen by
    /// [`Self::target`].
    ///
    /// Each guest gets its own directory because `run` keys a compatibility record by the
    /// containing directory's name (a title is a directory holding its material), so a flat
    /// layout would make every guest overwrite one record.
    pub fn path_for(&self, root: &Path, file: &str) -> PathBuf {
        root.join(&self.name)
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

/// Where one attempt to obtain an asset points.
///
/// **Classified from the string rather than declared**, so a manifest can list a sibling checkout
/// and a release URL side by side without a person also learning a keyword for each. Bare strings
/// are what makes the fallback list readable, and reading them is this code's job (D664).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// A path on this machine, resolved relative to the repository root.
    Path(String),
    /// Something to fetch over the network.
    Url(String),
}

impl Origin {
    /// Which kind of origin a manifest entry names.
    ///
    /// Only `http://` and `https://` are URLs. A Windows path begins `C:\`, which contains a
    /// colon and is emphatically not a scheme - checking for `://` rather than `:` is the whole
    /// difference, and it is why this is a function with a test rather than an inline guess.
    #[must_use]
    pub fn classify(origin: &str) -> Self {
        if origin.starts_with("http://") || origin.starts_with("https://") {
            Self::Url(origin.to_owned())
        } else {
            Self::Path(origin.to_owned())
        }
    }
}

/// What happened when a list of origins was walked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attempt<T> {
    /// What the origin that answered produced.
    pub value: T,
    /// Which origin answered.
    pub used: String,
    /// The ones tried before it, and why each did not answer.
    ///
    /// **Kept rather than discarded.** A sibling checkout that is absent and a release that
    /// answers 404 are different problems with different fixes, and a fallback that reports only
    /// its success hides the fact that the fast path is broken.
    pub failed: Vec<(String, String)>,
}

/// Walks `origins` in order and answers from the first that succeeds.
///
/// Pure: `attempt` does the fetching, so the ordering, the reporting and the give-up condition are
/// all testable without a network or a filesystem - the shape principle 8 asks for.
///
/// # Errors
///
/// When every origin failed, carrying each one and its reason so a caller can say what it tried.
pub fn first_that_answers<T, E: ToString>(
    origins: &[String],
    mut attempt: impl FnMut(&str) -> Result<T, E>,
) -> Result<Attempt<T>, Vec<(String, String)>> {
    let mut failed = Vec::new();
    for origin in origins {
        match attempt(origin) {
            Ok(value) => {
                return Ok(Attempt {
                    value,
                    used: origin.clone(),
                    failed,
                });
            }
            Err(why) => failed.push((origin.clone(), why.to_string())),
        }
    }
    Err(failed)
}

#[cfg(test)]
mod tests {

    /// **An origin is a local path or a URL, and nothing has to say which.**
    ///
    /// Written first. The manifest lists origins as bare strings so a person can paste a release
    /// URL beside a sibling checkout without also learning a `kind` keyword - so the classifying
    /// is this code's job, and getting it wrong means trying to open a URL as a file (D664).
    #[test]
    fn an_origin_knows_whether_it_is_a_url_or_a_path() {
        assert_eq!(
            Origin::classify("https://github.com/x/y/releases/download/t/a.zip"),
            Origin::Url("https://github.com/x/y/releases/download/t/a.zip".to_owned())
        );
        assert_eq!(
            Origin::classify("http://example.invalid/a.elf"),
            Origin::Url("http://example.invalid/a.elf".to_owned())
        );
        assert_eq!(
            Origin::classify("../obscene/build/prospero"),
            Origin::Path("../obscene/build/prospero".to_owned())
        );
        // A Windows path is not a URL, and `C:` is not a scheme. The colon is what makes this
        // worth a test rather than a one-liner.
        assert_eq!(
            Origin::classify(r"D:\builds\probe"),
            Origin::Path(r"D:\builds\probe".to_owned())
        );
    }

    /// The first origin that answers wins, and the ones before it are reported as tried.
    #[test]
    fn the_first_origin_that_answers_is_the_one_used() {
        let tried = first_that_answers(&["a".to_owned(), "b".to_owned(), "c".to_owned()], |o| {
            if o == "b" {
                Ok(7)
            } else {
                Err("nope".to_owned())
            }
        });
        let attempt = tried.expect("one of them answered");
        assert_eq!(attempt.value, 7);
        assert_eq!(attempt.used, "b");
        assert_eq!(
            attempt.failed,
            vec![("a".to_owned(), "nope".to_owned())],
            "what was tried and why it did not answer is kept, not discarded"
        );
    }

    /// **All of them failing is an error naming every attempt**, not a silent skip.
    ///
    /// The negative case, and the one the manifest shape exists for: a sibling checkout that is
    /// not there and a release that 404s should say both, so the next person knows which to fix.
    #[test]
    fn every_origin_failing_reports_every_reason() {
        let tried: Result<Attempt<u8>, Vec<(String, String)>> =
            first_that_answers(&["a".to_owned(), "b".to_owned()], |o| {
                Err(format!("{o} was not there"))
            });
        let failures = tried.expect_err("none of them answered");
        assert_eq!(
            failures.len(),
            2,
            "every origin is reported, not just the last"
        );
        assert!(failures.iter().any(|(o, _)| o == "a"));
        assert!(failures.iter().any(|(o, _)| o == "b"));
    }
    use super::*;

    fn source(kind: &str) -> Source {
        Source {
            target: Target::default(),
            sources: Vec::new(),
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
        let t = source(KIND_GITHUB_RELEASE).path_for(Path::new("titles"), "elfldr_v0.26.elf");
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
