//! The test corpus: a manifest of sources, fetched into `titles/`, ready to run and record.
//!
//! A tracked manifest, `corpus/sources.toml`, holds metadata only: a name, where the bytes come
//! from, a licence, a citation and a per-asset hash. The guest bytes are fetched into gitignored
//! roots, so the corpus is reproducible from a checkout without committing anybody else's binary
//! (D042). A `github-release` asset is verified against its pin on every fetch; a `local` asset
//! is a dev artifact snapshotted from a sibling checkout. This crate holds the manifest and the
//! fetch logic only; the CLI runs each guest through the ordinary `run` path, which records to
//! `compat/`.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// A `github-release` source: assets pinned by hash and verified every fetch.
pub const KIND_GITHUB_RELEASE: &str = "github-release";
/// A `local` source: bytes copied from a sibling checkout, re-snapshotted each fetch, for a
/// project of ours with no published release; carries a `todo` to migrate it.
pub const KIND_LOCAL: &str = "local";

/// The whole manifest: every source the corpus knows about.
#[derive(Debug, Default, Deserialize, Serialize)]
pub struct Manifest {
    /// Every source, in the order they appear in the file.
    #[serde(default)]
    pub source: Vec<Source>,
}

/// Which of orbistoun's roots a source's assets belong under.
///
/// A title is a directory with a `param.json`, an `eboot.bin` and its own filesystem; a payload
/// is one executable; a package is not installed yet. Each kind has its own root, and the
/// manifest says which (D661).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Target {
    /// Installed titles, a directory each with their own material.
    #[default]
    Titles,
    /// Raw executables, run directly rather than installed.
    Payloads,
    /// Installable packages, before anything installs them.
    Packages,
    /// Titles staged on the user partition, as `pros restore` stages homebrew on the hardware: the
    /// library's `data/homebrew` tree. A title there runs with a writable `/app0` (D722).
    Staged,
}

impl Target {
    /// How to name it in a report.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Titles => "titles",
            Self::Payloads => "payloads",
            Self::Packages => "packages",
            Self::Staged => "staged",
        }
    }
}
/// One source: where a set of guests comes from, and the terms under which they were obtained.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Source {
    /// The directory name, under whichever root [`Self::target`] names, this source's guests
    /// land in.
    pub name: String,
    /// Which root those guests belong under. Defaults to `titles`.
    #[serde(default)]
    pub target: Target,
    /// `github-release` or `local`; see the `KIND_*` constants.
    pub kind: String,
    /// `github-release`: `owner/repo`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    /// `github-release`: the pinned release tag, a tag and never a branch (D042).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    /// `local`: a path to the source's build output, relative to the orbistoun repo root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Origins to try in order, each a local path or a URL; the first that answers wins.
    ///
    /// An alternative to `repo`/`tag`/`path`, not an addition: a sibling checkout is the fast path
    /// where one exists and a published release the fallback. Every origin that failed is reported,
    /// so a broken fast path does not hide behind a working one (D664).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<String>,
    /// The [`Self::sources`] each name a `.zip` of a whole title directory, which is unpacked into
    /// `<root>/<name>/` in the layout `run` reads.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub archive: bool,
    /// The licence the assets are obtained under. Downloading is not redistributing, so this is a
    /// note, not a gate.
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
    /// Downloaded but did not match its pin: the release moved under a fixed tag. An error.
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
    /// The title id this guest records under: its file stem, which becomes the parent directory so
    /// `run`'s automatic recording keys `compat/<stem>.toml`.
    pub stem: String,
    /// Where the bytes now live: `titles/<source>/<stem>/<file>`.
    pub path: PathBuf,
    /// The SHA-256 the fetched bytes actually hashed to.
    pub sha256: String,
    /// How many bytes were fetched.
    pub bytes: u64,
    /// What the fetch amounted to: pinned, verified, cached, snapshotted, or a mismatch.
    pub state: State,
}

impl Source {
    /// The download URL for one asset of a `github-release` source.
    ///
    /// # Errors
    ///
    /// When the source has no `repo` or no `tag`.
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

    /// Fetches the source's one asset from the first of [`Self::sources`] that answers, pins it,
    /// and places it: an [`Self::archive`] unpacked into `<root>/<name>/`, anything else written as
    /// `<root>/<name>/<file>`.
    ///
    /// A `local` source refreshes its pin whichever origin answered, since its fallback is a
    /// rolling build, a dev artifact like the sibling checkout. Any other source's URL is checked
    /// against the pin, and a mismatch is reported, not placed.
    fn sync_from_sources(
        &mut self,
        repo_root: &Path,
        root: &Path,
        client: &reqwest::blocking::Client,
    ) -> Result<Vec<Outcome>> {
        let attempt = first_that_answers(&self.sources, |origin| match Origin::classify(origin) {
            Origin::Path(path) => std::fs::read(repo_root.join(path)).map_err(|e| e.to_string()),
            Origin::Url(url) => client
                .get(&url)
                .send()
                .and_then(reqwest::blocking::Response::error_for_status)
                .and_then(reqwest::blocking::Response::bytes)
                .map(|bytes| bytes.to_vec())
                .map_err(|e| e.to_string()),
        })
        .map_err(|failed| {
            anyhow::anyhow!(
                "no origin of {:?} answered: {}",
                self.name,
                failed
                    .iter()
                    .map(|(origin, why)| format!("{origin} ({why})"))
                    .collect::<Vec<_>>()
                    .join("; ")
            )
        })?;
        for (origin, why) in &attempt.failed {
            println!("  skipped   {origin}: {why}");
        }
        let bytes = attempt.value;
        let local =
            matches!(Origin::classify(&attempt.used), Origin::Path(_)) || self.kind == KIND_LOCAL;
        let file = Self::base(&attempt.used);
        let sha = hash_hex(&bytes);
        if self.asset.is_empty() {
            self.asset.push(Asset {
                file: file.clone(),
                sha256: None,
            });
        }
        let pin = self.asset[0].sha256.clone();
        let state = if local {
            State::LocalSnapshot
        } else {
            match pin.as_deref() {
                Some(p) if p == sha => State::Verified,
                Some(p) => State::Mismatch {
                    expected: p.to_owned(),
                },
                None => State::PinnedNew,
            }
        };
        let into = root.join(&self.name);
        if !matches!(state, State::Mismatch { .. }) {
            if self.archive {
                unpack_title(&bytes, &into)
                    .with_context(|| format!("unpacking {file} into {}", into.display()))?;
            } else {
                write_atomic(&into.join(&file), &bytes)?;
            }
            self.asset[0] = Asset {
                file: file.clone(),
                sha256: Some(sha.clone()),
            };
        }
        Ok(vec![Outcome {
            source: self.name.clone(),
            file,
            stem: self.name.clone(),
            path: into,
            sha256: sha,
            bytes: bytes.len() as u64,
            state,
        }])
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
    /// containing directory's name, so a flat layout would make every guest overwrite one record.
    pub fn path_for(&self, root: &Path, file: &str) -> PathBuf {
        root.join(&self.name)
            .join(Self::stem(file))
            .join(Self::base(file))
    }

    /// Fetches every asset of this source into `titles_root`, verifying or pinning each.
    ///
    /// `repo_root` is where a `local` source's relative `path` is resolved from. Each asset's
    /// `sha256` is updated in place when a fetch pins or refreshes it, so the caller can persist
    /// the manifest. Returns one [`Outcome`] per asset; a [`State::Mismatch`] is returned, not
    /// raised, so the caller decides whether it stops the sync.
    ///
    /// # Errors
    ///
    /// When a source is malformed, an asset cannot be fetched or read, or a file cannot be written.
    pub fn sync(
        &mut self,
        repo_root: &Path,
        titles_root: &Path,
        client: &reqwest::blocking::Client,
    ) -> Result<Vec<Outcome>> {
        // A source that lists origins is fetched from them; the `asset` walk below is for `path`
        // and `repo` sources, which name their assets.
        if self.archive || !self.sources.is_empty() {
            return self.sync_from_sources(repo_root, titles_root, client);
        }
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

            // Pin a new hash and refresh a local snapshot; leave a verified pin; never overwrite a
            // pin the bytes failed to match, which is the caller's to resolve.
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

/// An HTTP client for fetching release assets, built here so `reqwest` is a dependency of this
/// crate and not of the binary. It carries a user-agent, which release downloads expect.
///
/// # Errors
///
/// When the client cannot be built.
pub fn client() -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .user_agent(concat!("orbistoun-corpus/", env!("CARGO_PKG_VERSION")))
        .build()
        .context("building an HTTP client")
}

/// SHA-256 of some bytes, lowercase hex: the pin format.
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

/// Reads a manifest from `corpus/sources.toml`.
///
/// # Errors
///
/// When the file cannot be read or parsed.
pub fn load(path: &Path) -> Result<Manifest> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}

/// Writes a manifest back, so freshly pinned hashes persist.
///
/// # Errors
///
/// When the manifest cannot be serialised or written.
pub fn save(path: &Path, manifest: &Manifest) -> Result<()> {
    let text = toml::to_string_pretty(manifest).context("serialising the manifest")?;
    std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))
}

/// Writes bytes to `target`, creating parent directories, through a temporary file and a rename,
/// so a killed fetch never leaves a truncated guest a later run would treat as real.
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

/// Unpacks a packaged title directory into `into`: every file at its path in the archive, with a
/// single top-level folder the whole archive sits in stripped, since the corpus names the
/// directory itself.
///
/// A zip or a tar, told apart by their bytes rather than the file name. Only an entry's enclosed
/// name is used, so an entry naming `..` or an absolute path is refused rather than written
/// outside `into`.
///
/// # Errors
///
/// When the bytes are neither, an entry cannot be read or has an unsafe name, or a file cannot be
/// written.
pub fn unpack_title(bytes: &[u8], into: &Path) -> Result<usize> {
    let entries = if bytes.starts_with(b"PK") {
        zip_entries(bytes)?
    } else if bytes.get(257..262) == Some(b"ustar") {
        tar_entries(bytes)?
    } else {
        anyhow::bail!("not a zip or a tar archive");
    };
    // The one folder every entry sits in, when there is exactly one.
    let first = |path: &PathBuf| path.components().next().map(|c| c.as_os_str().to_owned());
    let top = entries.first().and_then(|entry| first(&entry.path));
    let shared = top.filter(|top| {
        entries.iter().all(|entry| {
            first(&entry.path).as_ref() == Some(top)
                && (entry.path.components().count() > 1 || entry.contents.is_none())
        })
    });
    let mut written = 0;
    for entry in entries {
        let relative = match &shared {
            Some(_) => entry.path.components().skip(1).collect::<PathBuf>(),
            None => entry.path,
        };
        if relative.as_os_str().is_empty() {
            continue;
        }
        let target = into.join(&relative);
        match entry.contents {
            None => std::fs::create_dir_all(&target)
                .with_context(|| format!("creating {}", target.display()))?,
            Some(contents) => {
                write_atomic(&target, &contents)?;
                written += 1;
            }
        }
    }
    Ok(written)
}

/// One entry of a packaged title: its enclosed path, and its bytes (`None` for a directory).
struct PackedEntry {
    path: PathBuf,
    contents: Option<Vec<u8>>,
}

/// A zip's entries, each by its enclosed name.
fn zip_entries(bytes: &[u8]) -> Result<Vec<PackedEntry>> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).context("not a zip")?;
    let mut entries = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let path = entry
            .enclosed_name()
            .with_context(|| format!("entry {:?} names a path outside the title", entry.name()))?;
        let contents = if entry.is_dir() {
            None
        } else {
            let mut contents = Vec::with_capacity(usize::try_from(entry.size()).unwrap_or(0));
            std::io::Read::read_to_end(&mut entry, &mut contents)?;
            Some(contents)
        };
        entries.push(PackedEntry { path, contents });
    }
    Ok(entries)
}

/// A tar's regular files and directories. A path is refused unless every component is an
/// ordinary name (no root, no `..`, no drive), so nothing lands outside the title. Links and
/// other special entries are skipped.
fn tar_entries(bytes: &[u8]) -> Result<Vec<PackedEntry>> {
    let mut archive = tar::Archive::new(std::io::Cursor::new(bytes));
    let mut entries = Vec::new();
    for entry in archive.entries().context("not a tar")? {
        let mut entry = entry.context("reading a tar entry")?;
        let path = entry.path().context("a tar entry's path")?.into_owned();
        let enclosed = path.components().all(|component| {
            matches!(
                component,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )
        });
        if !enclosed {
            anyhow::bail!("entry {} names a path outside the title", path.display());
        }
        let path: PathBuf = path
            .components()
            .filter(|component| matches!(component, std::path::Component::Normal(_)))
            .collect();
        let contents = match entry.header().entry_type() {
            tar::EntryType::Directory => None,
            tar::EntryType::Regular | tar::EntryType::Continuous => {
                let mut contents = Vec::with_capacity(usize::try_from(entry.size()).unwrap_or(0));
                std::io::Read::read_to_end(&mut entry, &mut contents)?;
                Some(contents)
            }
            _ => continue,
        };
        entries.push(PackedEntry { path, contents });
    }
    Ok(entries)
}

/// Where one attempt to obtain an asset points.
///
/// Classified from the string rather than declared, so a manifest lists a sibling checkout and
/// a release URL side by side as bare strings (D664).
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
    /// Only `http://` and `https://` are URLs: a Windows path such as `C:\` contains a colon and is
    /// not a scheme.
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
    /// An absent sibling checkout and a release that answers 404 have different fixes, so a
    /// fallback reports what failed as well as what succeeded.
    pub failed: Vec<(String, String)>,
}

/// Walks `origins` in order and answers from the first that succeeds.
///
/// Pure: `attempt` does the fetching, so the ordering, reporting and give-up condition are
/// testable without a network or a filesystem.
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
    /// A zip holding `entries` (name, contents), stored uncompressed.
    fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (name, contents) in entries {
            writer.start_file(*name, options).expect("an entry starts");
            std::io::Write::write_all(&mut writer, contents).expect("an entry writes");
        }
        writer.finish().expect("the zip finishes").into_inner()
    }

    /// A packaged title unpacks as the title directory with its one top folder stripped,
    /// subdirectories kept.
    #[test]
    fn a_packaged_title_unpacks_with_its_top_folder_stripped() {
        let dir = std::env::temp_dir().join(format!("corpus-unpack-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let bytes = zip_of(&[
            ("SCSH00001/eboot.bin", b"elf"),
            ("SCSH00001/sce_sys/param.json", b"{}"),
        ]);
        let written = unpack_title(&bytes, &dir).expect("unpacks");
        assert_eq!(written, 2);
        assert_eq!(std::fs::read(dir.join("eboot.bin")).expect("eboot"), b"elf");
        assert_eq!(
            std::fs::read(dir.join("sce_sys").join("param.json")).expect("param"),
            b"{}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A zip entry naming a path outside the title is refused, and nothing is written.
    #[test]
    fn an_entry_escaping_the_title_is_refused() {
        let dir = std::env::temp_dir().join(format!("corpus-escape-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let bytes = zip_of(&[("../outside.bin", b"x")]);
        assert!(unpack_title(&bytes, &dir).is_err());
        assert!(!dir.exists());
    }

    /// A tar holding `entries` (name, contents), each name written into its header as given, so a
    /// test can name a path the builder's own checks would refuse.
    fn tar_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        for (name, contents) in entries {
            let mut header = tar::Header::new_gnu();
            header.as_gnu_mut().expect("a gnu header").name[..name.len()]
                .copy_from_slice(name.as_bytes());
            header.set_size(contents.len() as u64);
            header.set_entry_type(tar::EntryType::Regular);
            header.set_cksum();
            builder
                .append(&header, *contents)
                .expect("an entry appends");
        }
        builder.into_inner().expect("the tar finishes")
    }

    /// A title packaged as a tar under a `.zip` name unpacks the same: the bytes, not the name, say
    /// which it is.
    #[test]
    fn a_title_packaged_as_a_tar_unpacks_like_a_zip() {
        let dir = std::env::temp_dir().join(format!("corpus-tar-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let bytes = tar_of(&[
            ("NVRB00001/eboot.bin", b"elf"),
            ("NVRB00001/sce_sys/param.json", b"{}"),
        ]);
        assert_eq!(unpack_title(&bytes, &dir).expect("unpacks"), 2);
        assert_eq!(std::fs::read(dir.join("eboot.bin")).expect("eboot"), b"elf");
        assert!(dir.join("sce_sys").join("param.json").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A tar entry naming a path outside the title is refused, as are bytes that are neither.
    #[test]
    fn a_tar_entry_escaping_the_title_is_refused() {
        let dir = std::env::temp_dir().join(format!("corpus-tar-escape-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let bytes = tar_of(&[("../outside.bin", b"x")]);
        assert!(unpack_title(&bytes, &dir).is_err());
        assert!(!dir.exists());
        assert!(unpack_title(b"neither a zip nor a tar", &dir).is_err());
    }

    /// An origin is classified as a local path or a URL from the bare string.
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
        // A Windows path is not a URL, and `C:` is not a scheme.
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

    /// Every origin failing is an error naming every attempt, not a silent skip.
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
            archive: false,
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

    /// A file's stem becomes its title directory.
    #[test]
    fn a_stem_becomes_the_title_directory() {
        assert_eq!(Source::stem("elfldr_v0.26.elf"), "elfldr_v0.26");
        assert_eq!(Source::stem("etaHEN_2.5B.bin"), "etaHEN_2.5B");
    }

    /// A github asset's URL is the release download path.
    #[test]
    fn a_github_asset_url_is_the_release_download_path() {
        assert_eq!(
            source(KIND_GITHUB_RELEASE).asset_url("a.elf").unwrap(),
            "https://github.com/owner/repo/releases/download/t/a.elf"
        );
    }

    /// Each guest gets a directory of its own under its source.
    #[test]
    fn the_target_is_one_directory_per_guest() {
        let t = source(KIND_GITHUB_RELEASE).path_for(Path::new("titles"), "elfldr_v0.26.elf");
        let got: PathBuf = t.components().collect();
        let want: PathBuf = ["titles", "src", "elfldr_v0.26", "elfldr_v0.26.elf"]
            .iter()
            .collect();
        assert_eq!(got, want);
    }

    /// A manifest survives a round trip through TOML.
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
