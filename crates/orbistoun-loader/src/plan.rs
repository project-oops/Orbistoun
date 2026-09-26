//! The link plan: everything linking decided about a title, as data (D724).
//!
//! Every base orbistoun places at is fixed, so one executable linked by one loader build writes
//! the same values to the same slots every time. The plan records those decisions - where each
//! module sits, what access each segment has, and every value relocation wrote - so two links
//! compare as data, and a verdict between runs that linked differently says so.

use std::fmt::Write as _;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::Image;

/// One value relocation wrote into guest memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SlotWrite {
    /// The guest address written.
    pub at: u64,
    /// The eight bytes written there.
    pub value: u64,
}

/// One segment as placed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SegmentPlan {
    /// Where the segment starts.
    pub address: u64,
    /// Bytes it occupies, `.bss` included.
    pub size: u64,
    /// Its `p_flags`, from which its protection follows.
    pub flags: u32,
}

/// What linking decided for one module.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModulePlan {
    /// The library name, or empty for the executable.
    pub library: String,
    /// The placement base.
    pub base: u64,
    /// The reserved span as `(start, length)`.
    pub span: (u64, u64),
    /// Every segment, in program-header order.
    pub segments: Vec<SegmentPlan>,
    /// Every relocation write, ordered by address.
    pub writes: Vec<SlotWrite>,
}

impl ModulePlan {
    /// The plan for a placed image and the writes relocating it made.
    #[must_use]
    pub fn of(library: &str, image: &Image, mut writes: Vec<SlotWrite>) -> Self {
        // By address, so two links that applied the same tables in a different order agree.
        writes.sort_unstable();
        Self {
            library: library.to_owned(),
            base: image.base(),
            span: image.span(),
            segments: image
                .segments()
                .iter()
                .map(|s| SegmentPlan {
                    address: s.address,
                    size: s.memsz(),
                    flags: s.flags,
                })
                .collect(),
            writes,
        }
    }
}

/// What linking decided for a whole title: the executable first, then its own modules.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkPlan {
    /// One entry per module.
    pub modules: Vec<ModulePlan>,
}

impl LinkPlan {
    /// A short, stable name for the plan's content.
    ///
    /// Computed over a fixed binary encoding rather than a serialisation, so a change of
    /// serialisation format never reads as a change of plan.
    #[must_use]
    pub fn digest(&self) -> String {
        let mut hash = Sha256::new();
        for module in &self.modules {
            hash.update((module.library.len() as u64).to_le_bytes());
            hash.update(module.library.as_bytes());
            for word in [module.base, module.span.0, module.span.1] {
                hash.update(word.to_le_bytes());
            }
            hash.update((module.segments.len() as u64).to_le_bytes());
            for segment in &module.segments {
                hash.update(segment.address.to_le_bytes());
                hash.update(segment.size.to_le_bytes());
                hash.update(segment.flags.to_le_bytes());
            }
            hash.update((module.writes.len() as u64).to_le_bytes());
            for write in &module.writes {
                hash.update(write.at.to_le_bytes());
                hash.update(write.value.to_le_bytes());
            }
        }
        let digest = hash.finalize();
        let mut name = String::with_capacity(16);
        for byte in digest.iter().take(8) {
            let _ = write!(name, "{byte:02x}");
        }
        name
    }

    /// Total relocation writes across every module.
    #[must_use]
    pub fn write_count(&self) -> usize {
        self.modules.iter().map(|m| m.writes.len()).sum()
    }
}

/// What a stored plan was computed from (D724).
///
/// One executable linked by one loader build on a host with the same instructions makes the
/// same decisions, so a stored plan under an equal key is the plan a fresh link must reproduce.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanKey {
    /// The executable's SHA-256, in hex.
    pub executable: String,
    /// The orbistoun build that linked it.
    pub build: String,
    /// The host CPU features linking may depend on.
    pub host: String,
}

impl PlanKey {
    /// The key for an executable's bytes, linked by `build` on this host.
    #[must_use]
    pub fn for_executable(bytes: &[u8], build: String) -> Self {
        let mut executable = String::with_capacity(64);
        for byte in Sha256::digest(bytes) {
            let _ = write!(executable, "{byte:02x}");
        }
        Self {
            executable,
            build,
            host: host_features(),
        }
    }
}

/// The host CPU features an instruction rewrite may depend on, present ones only, in a fixed
/// order (D725).
#[must_use]
pub fn host_features() -> String {
    #[cfg(target_arch = "x86_64")]
    {
        let known = [
            ("sse4a", std::arch::is_x86_feature_detected!("sse4a")),
            ("sse4.2", std::arch::is_x86_feature_detected!("sse4.2")),
            ("popcnt", std::arch::is_x86_feature_detected!("popcnt")),
            ("lzcnt", std::arch::is_x86_feature_detected!("lzcnt")),
            ("bmi1", std::arch::is_x86_feature_detected!("bmi1")),
            ("bmi2", std::arch::is_x86_feature_detected!("bmi2")),
            ("avx", std::arch::is_x86_feature_detected!("avx")),
            ("avx2", std::arch::is_x86_feature_detected!("avx2")),
            ("fma", std::arch::is_x86_feature_detected!("fma")),
            ("f16c", std::arch::is_x86_feature_detected!("f16c")),
            ("movbe", std::arch::is_x86_feature_detected!("movbe")),
            ("aes", std::arch::is_x86_feature_detected!("aes")),
            (
                "pclmulqdq",
                std::arch::is_x86_feature_detected!("pclmulqdq"),
            ),
            ("sha", std::arch::is_x86_feature_detected!("sha")),
        ];
        known
            .iter()
            .filter(|(_, present)| *present)
            .map(|(name, _)| *name)
            .collect::<Vec<_>>()
            .join(",")
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        std::env::consts::ARCH.to_owned()
    }
}

/// A plan as the title library keeps it, with what it was computed from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredPlan {
    /// What the plan was computed from.
    pub key: PlanKey,
    /// The plan.
    pub plan: LinkPlan,
}

/// How a fresh plan stands against the stored one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    /// Nothing was stored under this key, so the fresh plan now is.
    New,
    /// The stored plan under this key is the fresh plan.
    Match,
    /// The stored plan under this key differs from the fresh one: a loader defect.
    Mismatch,
}

impl Standing {
    /// The word a run report records.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Match => "match",
            Self::Mismatch => "mismatch",
        }
    }
}

/// The stored plan at `path`, if one is there and readable.
#[must_use]
pub fn load(path: &Path) -> Option<StoredPlan> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Writes `plan` under `key` to `path`, replacing whatever is there.
///
/// # Errors
///
/// When the directory cannot be created or the file written.
pub fn store(path: &Path, key: &PlanKey, plan: &LinkPlan) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let stored = StoredPlan {
        key: key.clone(),
        plan: plan.clone(),
    };
    let bytes = serde_json::to_vec(&stored).map_err(std::io::Error::other)?;
    // Written beside and renamed over, so a run stopped mid-write leaves the old plan or none.
    let partial = path.with_extension("partial");
    std::fs::write(&partial, bytes)?;
    std::fs::rename(&partial, path)
}

/// Compares a fresh plan with the one stored at `path`, storing it when none is stored under
/// its key.
///
/// A stored plan under an equal key is never overwritten here, so a mismatch stays
/// reproducible until a relink replaces it on purpose. The stored plan comes back beside the
/// standing, for naming what differs.
///
/// # Errors
///
/// When a plan that must be stored cannot be written.
pub fn settle(
    path: &Path,
    key: &PlanKey,
    plan: &LinkPlan,
) -> std::io::Result<(Standing, Option<StoredPlan>)> {
    match load(path) {
        Some(stored) if stored.key == *key => {
            let standing = if stored.plan == *plan {
                Standing::Match
            } else {
                Standing::Mismatch
            };
            Ok((standing, Some(stored)))
        }
        _ => {
            store(path, key, plan)?;
            Ok((Standing::New, None))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{LinkPlan, ModulePlan, PlanKey, SegmentPlan, SlotWrite, Standing};

    fn key() -> PlanKey {
        PlanKey::for_executable(b"executable", "build".to_owned())
    }

    /// The first run stores its plan; the next with the same plan matches and leaves it.
    #[test]
    fn a_plan_is_stored_once_then_matched() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("title").join("link-plan.json");
        let plan = LinkPlan {
            modules: vec![module(Vec::new())],
        };
        let (first, _) = super::settle(&path, &key(), &plan).unwrap();
        assert_eq!(first, Standing::New);
        let before = std::fs::read(&path).unwrap();
        let (second, stored) = super::settle(&path, &key(), &plan).unwrap();
        assert_eq!(second, Standing::Match);
        assert_eq!(stored.unwrap().plan, plan);
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    /// Under the same key a different plan is a mismatch, and the stored one is kept.
    #[test]
    fn a_differing_plan_under_the_same_key_is_kept_and_reported() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("link-plan.json");
        let stored = LinkPlan {
            modules: vec![module(vec![SlotWrite { at: 8, value: 1 }])],
        };
        let fresh = LinkPlan {
            modules: vec![module(vec![SlotWrite { at: 8, value: 2 }])],
        };
        super::settle(&path, &key(), &stored).unwrap();
        let (standing, _) = super::settle(&path, &key(), &fresh).unwrap();
        assert_eq!(standing, Standing::Mismatch);
        assert_eq!(super::load(&path).unwrap().plan, stored);
    }

    /// A changed key replaces the stored plan.
    #[test]
    fn a_changed_key_replaces_the_stored_plan() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("link-plan.json");
        let fresh = LinkPlan {
            modules: vec![module(Vec::new())],
        };
        super::settle(&path, &key(), &LinkPlan::default()).unwrap();
        let rebuilt = PlanKey::for_executable(b"executable", "another build".to_owned());
        let (standing, _) = super::settle(&path, &rebuilt, &fresh).unwrap();
        assert_eq!(standing, Standing::New);
        let now = super::load(&path).unwrap();
        assert_eq!((now.key, now.plan), (rebuilt, fresh));
    }

    /// The executable part of the key is its SHA-256.
    #[test]
    fn the_key_names_the_executable_by_its_sha256() {
        assert_eq!(
            PlanKey::for_executable(b"", String::new()).executable,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    fn module(writes: Vec<SlotWrite>) -> ModulePlan {
        ModulePlan {
            library: String::new(),
            base: 0x4000_0000_0000,
            span: (0x4000_0000_0000, 0x10000),
            segments: vec![SegmentPlan {
                address: 0x4000_0000_0000,
                size: 0x1000,
                flags: 5,
            }],
            writes,
        }
    }

    /// The same decisions name the same plan.
    #[test]
    fn equal_plans_share_a_digest() {
        let write = SlotWrite {
            at: 0x4000_0000_0100,
            value: 0x7000_0000_0040,
        };
        let a = LinkPlan {
            modules: vec![module(vec![write])],
        };
        let b = LinkPlan {
            modules: vec![module(vec![write])],
        };
        assert_eq!(a.digest(), b.digest());
        assert_eq!(a.digest().len(), 16);
    }

    /// One slot answered differently is a different plan.
    #[test]
    fn a_changed_value_changes_the_digest() {
        let at = 0x4000_0000_0100;
        let a = LinkPlan {
            modules: vec![module(vec![SlotWrite { at, value: 1 }])],
        };
        let b = LinkPlan {
            modules: vec![module(vec![SlotWrite { at, value: 2 }])],
        };
        assert_ne!(a.digest(), b.digest());
    }

    /// A module's name is part of the plan: the same bytes linked as a different library differ.
    #[test]
    fn the_library_name_is_part_of_the_plan() {
        let a = LinkPlan {
            modules: vec![module(Vec::new())],
        };
        let mut renamed = module(Vec::new());
        renamed.library = "libc".to_owned();
        let b = LinkPlan {
            modules: vec![renamed],
        };
        assert_ne!(a.digest(), b.digest());
    }
}
