//! The link plan: everything linking decided about a title, as data (D724).
//!
//! Every base orbistoun places at is fixed, so one executable linked by one loader build writes
//! the same values to the same slots every time. The plan records those decisions - where each
//! module sits, what access each segment has, and every value relocation wrote - so two links
//! compare as data, and a verdict between runs that linked differently says so.

use std::fmt::Write as _;

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

#[cfg(test)]
mod tests {
    use super::{LinkPlan, ModulePlan, SegmentPlan, SlotWrite};

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
