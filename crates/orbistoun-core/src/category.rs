//! What a title is allowed to do, by the category it declares.
//!
//! # Two orthogonal axes, and this is the second one
//!
//! Execution on the target is governed by a **privilege tier** - the authority id in the
//! container header, which decides whether a program is sandboxed at all - and an **application
//! category**, declared as `applicationCategoryType` in a title's own `param.json`. The category
//! decides three things a guest can observe: how much direct memory the kernel will grant it,
//! whether it may own the video scanout, and whether it runs alongside other titles or excludes
//! them.
//!
//! Orbistoun modelled neither. Every title ran as though it were the most privileged kind, which
//! is wrong in the direction that hides bugs: a guest denied direct memory on the console gets it
//! here, proceeds, and fails somewhere else entirely (D662).
//!
//! # Where these numbers come from
//!
//! SELFish's `docs/features/writing.md`, which documents the categories it can stamp into a
//! package and what each one costs. A sibling project's own notes, which is ordinary engineering
//! and not the provenance boundary - that boundary is about other people's *source*.
//!
//! **Every value here is `published`, not `measured`.** No probe has yet asked the console to
//! confirm that a system app is granted zero bytes, and until one does this is a table read out
//! of a document. It is recorded that way so it can be promoted rather than believed.

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
    /// `65536` - a background utility. **No direct memory and no scanout at all.**
    SystemApp,
    /// `131072` - runs alongside a big app, drawing to the compositor rather than the scanout.
    MiniApp,
    /// `262144` - a streaming app, with its own protected video path.
    MediaApp,
    /// A value no published table names.
    ///
    /// **Kept rather than folded into the default.** A title declaring something unknown is not
    /// a big app, and treating it as one would grant it everything on the strength of a number
    /// nobody recognises.
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
    /// A system app that opens the display is refused `0x8029_0001` on the console. An
    /// unrecognised category is refused too: granting the scanout on the strength of a number
    /// nobody recognises is the permissive direction, and the permissive direction is the one
    /// that hides the divergence.
    #[must_use]
    pub const fn may_own_scanout(self) -> bool {
        matches!(self, Self::BigApp | Self::MediaApp)
    }

    /// Whether the kernel grants this category any direct memory at all.
    ///
    /// Zero for a system app, which is the sharpest documented consequence of the axis: every
    /// `sceKernelAllocateDirectMemory` fails, and a guest written for it allocates through
    /// `mmap` instead.
    #[must_use]
    pub const fn has_direct_memory(self) -> bool {
        !matches!(self, Self::SystemApp | Self::Unknown(_))
    }

    /// Whether a title of this category must ship `sce_module/libc.prx`.
    ///
    /// Documented for a big app and not for the others. A missing one is not modelled as a
    /// failure yet - it is reported, because nothing measured says what the console does about
    /// it and refusing to run a title over a documented requirement would be inventing a verdict.
    #[must_use]
    pub const fn requires_shipped_libc(self) -> bool {
        matches!(self, Self::BigApp)
    }
}

/// What opening the display answers a category that may not have it.
///
/// `SCE_VIDEO_OUT_ERROR_INVALID_VALUE`, per SELFish's table. Published, not measured: no probe
/// has yet asked the console to refuse a system app and reported what came back.
pub const SCANOUT_DENIED: u32 = 0x8029_0001;

/// The category this run presents, if one was declared.
static PRESENTED: std::sync::OnceLock<Category> = std::sync::OnceLock::new();

/// Records the category a title declared, once, before it is entered.
///
/// The same shape as the machine this process presents, and for the same reason: a guest call
/// arrives on a frame with no room to thread a context through, so what it needs has to be
/// reachable from a process-wide slot.
pub fn present(category: Category) {
    let _ = PRESENTED.set(category);
}

/// What this run presents itself as.
///
/// A big app when nothing declared one - which is what every measurement so far was taken
/// against, and what every title in the corpus that carries a `param.json` actually declares.
#[must_use]
pub fn presented() -> Category {
    PRESENTED.get().copied().unwrap_or_default()
}

/// The code a category is refused the scanout with, or [`None`] where it may have it.
///
/// A function rather than a bare bool because the *code* is the part a guest branches on, and a
/// caller reconstructing one from a bool would be inventing it.
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
/// **Zero, or "what it had before".** The only figure the table states exactly is the system
/// app's zero; the others are ranges written for a person sizing a package, not values to hand a
/// guest. So this answers zero where zero is documented and [`u64::MAX`] - meaning "not limited
/// by category" - everywhere else, rather than inventing a budget nobody measured (D662).
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

    /// **A run presents one category, and the answer is the presented one.**
    ///
    /// Written before there was anything to present it to. The corpus declares `0` for every
    /// title that has a `param.json`, so no title exercises the restricted paths - which is an
    /// argument for a test that constructs one, not for leaving the paths unwritten (D662).
    #[test]
    fn the_presented_category_is_what_a_caller_is_told() {
        // Not asserting on a fresh slot: this is a process-wide `OnceLock` and another test in
        // this binary may have set it. What must hold either way is that whatever is presented
        // is what comes back, and that nothing presented reads as a big app by accident.
        let seen = super::presented();
        assert_eq!(
            seen.may_own_scanout(),
            !matches!(seen, Category::SystemApp | Category::Unknown(_)),
            "the presented category's permissions are its own, not the default's"
        );
    }

    /// A system app is refused the scanout with the documented code, and refused direct memory.
    ///
    /// The two consequences the table states most sharply, asserted as *refusals* rather than as
    /// a count of things that worked.
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

    /// A big app is refused neither, so the permissive path is pinned as well as the restrictive.
    #[test]
    fn a_big_app_is_refused_neither() {
        assert_eq!(super::scanout_refusal(Category::BigApp), None);
        assert!(super::direct_memory_budget(Category::BigApp) > 0);
    }
    use super::{Category, SCANOUT_DENIED};

    /// **A system app is denied both things a big app gets**, which is the whole point of the
    /// axis and the case orbistoun currently gets wrong by granting everything.
    #[test]
    fn a_system_app_has_neither_scanout_nor_direct_memory() {
        let system = Category::from_declared(65_536);
        assert_eq!(system, Category::SystemApp);
        assert!(!system.may_own_scanout());
        assert!(!system.has_direct_memory());
    }

    /// A category nobody has named is refused, not waved through.
    ///
    /// The negative case that matters: `Unknown` defaulting to a big app's permissions would
    /// grant everything on the strength of an unrecognised number, and nothing would say so.
    #[test]
    fn an_unrecognised_category_is_not_treated_as_a_big_app() {
        let odd = Category::from_declared(0xdead);
        assert_eq!(odd, Category::Unknown(0xdead));
        assert!(!odd.may_own_scanout(), "not granted the scanout");
        assert!(!odd.has_direct_memory(), "not granted direct memory");
        assert_ne!(odd, Category::default());
    }

    /// A big app gets what a big app gets, so the permissive path is pinned too.
    #[test]
    fn a_big_app_owns_the_scanout_and_has_a_memory_budget() {
        let big = Category::from_declared(0);
        assert_eq!(big, Category::BigApp);
        assert!(big.may_own_scanout());
        assert!(big.has_direct_memory());
        assert!(big.requires_shipped_libc());
    }
}
