//! What a title is allowed to do, by the category it declares.
//!
//! Execution is governed by a privilege tier (the authority id in the container header) and by
//! an application category, `applicationCategoryType` in the title's `param.json`. The category
//! decides how much direct memory the kernel grants, whether the title may own the video
//! scanout, and whether it runs alongside other titles (D662). The values come from SELFish's
//! `docs/features/writing.md` and are published, not measured, so they can be promoted when a
//! probe confirms them.

use serde::{Deserialize, Serialize};

/// What kind of application a title declares itself to be.
///
/// The numbers are the values `applicationCategoryType` carries, not an ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Category {
    /// `0` - a full game. Full memory budget, exclusive scanout, foreground only.
    #[default]
    BigApp,
    /// `3` - a background system service. Headless.
    Daemon,
    /// `65536` - a background utility, with no direct memory and no scanout.
    SystemApp,
    /// `131072` - runs alongside a big app, drawing to the compositor rather than the scanout.
    MiniApp,
    /// `262144` - a streaming app, with its own protected video path.
    MediaApp,
    /// A value no published table names.
    ///
    /// Kept apart from the default: an unrecognised number is not granted a big app's permissions.
    Unknown(u32),
}

impl Category {
    /// The category a `param.json` value names.
    #[must_use]
    pub const fn from_declared(value: u32) -> Self {
        match value {
            0 => Self::BigApp,
            3 => Self::Daemon,
            65_536 => Self::SystemApp,
            131_072 => Self::MiniApp,
            262_144 => Self::MediaApp,
            other => Self::Unknown(other),
        }
    }

    /// How to name it in a report.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::BigApp => "big app",
            Self::Daemon => "daemon",
            Self::SystemApp => "system app",
            Self::MiniApp => "mini app",
            Self::MediaApp => "media app",
            Self::Unknown(_) => "an unrecognised category",
        }
    }

    /// Whether this category may own the video scanout.
    ///
    /// A system app that opens the display is refused `0x8029_0001`. An unrecognised category is
    /// refused too, since the permissive direction hides divergence.
    #[must_use]
    pub const fn may_own_scanout(self) -> bool {
        matches!(self, Self::BigApp | Self::MediaApp)
    }

    /// Whether the kernel grants this category any direct memory at all.
    ///
    /// Zero for a system app: every `sceKernelAllocateDirectMemory` fails, and a guest written for
    /// it allocates through `mmap`.
    #[must_use]
    pub const fn has_direct_memory(self) -> bool {
        !matches!(self, Self::SystemApp | Self::Unknown(_))
    }

    /// Whether a title of this category must ship `sce_module/libc.prx`.
    ///
    /// Documented for a big app only. A missing one is reported, not refused, because nothing
    /// measured says what the hardware does about it.
    #[must_use]
    pub const fn requires_shipped_libc(self) -> bool {
        matches!(self, Self::BigApp)
    }
}

/// What opening the display answers a category that may not have it.
///
/// `SCE_VIDEO_OUT_ERROR_INVALID_VALUE`, per SELFish's table; published, not measured.
pub const SCANOUT_DENIED: u32 = 0x8029_0001;

/// The category this run presents, if one was declared.
static PRESENTED: std::sync::OnceLock<Category> = std::sync::OnceLock::new();

/// Records the category a title declared, once, before it is entered.
///
/// A process-wide slot, like the presented machine: a guest call arrives on a frame with no
/// room to thread a context through.
pub fn present(category: Category) {
    let _ = PRESENTED.set(category);
}

/// What this run presents itself as.
///
/// A big app when nothing was declared, which is what every corpus title with a `param.json`
/// declares.
#[must_use]
pub fn presented() -> Category {
    PRESENTED.get().copied().unwrap_or_default()
}

/// The code a category is refused the scanout with, or [`None`] where it may have it.
///
/// Returns the code because the code is what a guest branches on.
#[must_use]
pub const fn scanout_refusal(category: Category) -> Option<u32> {
    if category.may_own_scanout() {
        None
    } else {
        Some(SCANOUT_DENIED)
    }
}

/// How many bytes of direct memory the kernel grants this category.
///
/// The table states only the system app's zero exactly; the other figures are sizing ranges.
/// So this answers zero where zero is documented and [`u64::MAX`] (not limited by category)
/// everywhere else, rather than inventing a budget (D662).
#[must_use]
pub const fn direct_memory_budget(category: Category) -> u64 {
    if category.has_direct_memory() {
        u64::MAX
    } else {
        0
    }
}
#[cfg(test)]
mod tests {

    /// The presented category is what a caller is told.
    #[test]
    fn the_presented_category_is_what_a_caller_is_told() {
        // Not asserting on a fresh slot: this is a process-wide `OnceLock` another test may have set.
        // Whatever is presented comes back, and nothing presented reads as a big app.
        let seen = super::presented();
        assert_eq!(
            seen.may_own_scanout(),
            !matches!(seen, Category::SystemApp | Category::Unknown(_)),
            "the presented category's permissions are its own, not the default's"
        );
    }

    /// A system app is refused the scanout with the documented code, and refused direct memory.
    #[test]
    fn a_system_app_is_refused_both_things_a_big_app_gets() {
        let system = Category::SystemApp;
        assert_eq!(
            super::scanout_refusal(system),
            Some(SCANOUT_DENIED),
            "opening the display answers the documented code"
        );
        assert_eq!(
            super::direct_memory_budget(system),
            0,
            "the kernel grants a system app no direct memory"
        );
    }

    /// A big app is refused neither.
    #[test]
    fn a_big_app_is_refused_neither() {
        assert_eq!(super::scanout_refusal(Category::BigApp), None);
        assert!(super::direct_memory_budget(Category::BigApp) > 0);
    }
    use super::{Category, SCANOUT_DENIED};

    /// A system app has neither scanout nor direct memory.
    #[test]
    fn a_system_app_has_neither_scanout_nor_direct_memory() {
        let system = Category::from_declared(65_536);
        assert_eq!(system, Category::SystemApp);
        assert!(!system.may_own_scanout());
        assert!(!system.has_direct_memory());
    }

    /// A category nobody has named is refused, not given a big app's permissions.
    #[test]
    fn an_unrecognised_category_is_not_treated_as_a_big_app() {
        let odd = Category::from_declared(0xdead);
        assert_eq!(odd, Category::Unknown(0xdead));
        assert!(!odd.may_own_scanout(), "not granted the scanout");
        assert!(!odd.has_direct_memory(), "not granted direct memory");
        assert_ne!(odd, Category::default());
    }

    /// A big app owns the scanout and has a memory budget.
    #[test]
    fn a_big_app_owns_the_scanout_and_has_a_memory_budget() {
        let big = Category::from_declared(0);
        assert_eq!(big, Category::BigApp);
        assert!(big.may_own_scanout());
        assert!(big.has_direct_memory());
        assert!(big.requires_shipped_libc());
    }
}
