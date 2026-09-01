//! A title's sandbox, established as one thing.
//!
//! # What this centralises, and why it is not just `filesystem::install`
//!
//! The overlay *engine* lives in [`crate::mount`] and [`crate::filesystem`]: a base tree
//! materialised from a knowledge file, and a per-title writable overlay stacked over it (D250,
//! D251). But assembling that for a *running title* is three steps that must happen in one order -
//! empty the overlay if this run is not to inherit the last one's files, install the base tree with
//! its writable device overlays, then layer the title's own files over `/app0` - and that order is
//! exactly what gets remembered wrong when it is spread across whoever happens to set a run up (the
//! textures-lost regression, D269).
//!
//! So it is one function, [`establish`], and every consumer calls it. A consumer supplies *where*
//! the bytes live and *how long* they last; nothing here reads the environment, so the fs crate
//! stays a mechanism its callers configure rather than one that configures itself (principle 5).

use std::path::Path;

/// Whether a title's sandbox keeps what it wrote between runs.
///
/// The console's own answer is presumably [`Ephemeral`](Self::Ephemeral); the default here is the
/// opposite on purpose, because a proof of concept wants the saves and the reports a run produced
/// to survive it. The choice is the consumer's, passed in - a run reads it from
/// `ORBISTOUN_SANDBOX`, a test states it outright - so the policy has one meaning and many callers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Retention {
    /// Keep it: saves and a probe's reports persist past the run that wrote them.
    #[default]
    Retain,
    /// Empty it at the start of each run, closer to a console sandbox that carries no state.
    Ephemeral,
}

/// Establishes a title's sandboxed filesystem: the accountable base tree, its writable device
/// overlays, and the title's own files over `/app0` - the whole guest-visible namespace.
///
/// - `base` is where the console's base tree is materialised (read-only to the guest).
/// - `overlay` is the per-title directory a guest's writes land in.
/// - `title_module` is the guest that was loaded; its directory becomes `/app0`.
/// - `retention` decides whether `overlay` is emptied first.
///
/// The order is the one [`crate::mount`] requires and is not the caller's to get right: the base is
/// installed first, each writable entry's overlay is stacked over it, and the title is layered over
/// `/app0` last so its own files answer before anything the console provides.
pub fn establish(base: &Path, overlay: &Path, title_module: &Path, retention: Retention) {
    if retention == Retention::Ephemeral {
        // **At the start of a run, not at a teardown.** A process guest is jumped to and leaves by
        // calling exit, so nothing after the entry point reliably runs; "empty at the start" is the
        // only point that always executes, and it is the observable property anyway (D422).
        let _ = std::fs::remove_dir_all(overlay);
    }
    crate::filesystem::install(base, overlay);
    crate::mount::mount_title(title_module);
}

#[cfg(test)]
mod tests {
    use super::{Retention, establish};

    /// A scratch root unique to this test, cleaned at both ends.
    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("orbistoun-sandbox-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// **Retain keeps what a previous run wrote; ephemeral does not.**
    ///
    /// This is the whole reason the policy is a setting: a save must survive the run that made it,
    /// and a run that must start clean must be able to say so. Asserted on a file placed in the
    /// overlay, because that is what a guest's write becomes.
    #[test]
    fn retention_keeps_or_empties_the_overlay_as_asked() {
        let _guard = crate::exclusively();
        crate::mount::clear();

        let root = scratch("retain");
        let base = root.join("base");
        let overlay = root.join("overlay");
        let title = root.join("title");
        std::fs::create_dir_all(&title).unwrap();
        std::fs::write(title.join("eboot.bin"), b"guest").unwrap();

        // A file a "previous run" left in the writable /data overlay.
        let left = overlay.join("data").join("save.bin");
        std::fs::create_dir_all(left.parent().unwrap()).unwrap();
        std::fs::write(&left, b"a save").unwrap();

        establish(&base, &overlay, &title.join("eboot.bin"), Retention::Retain);
        assert!(left.exists(), "retain keeps a previous run's file");

        establish(
            &base,
            &overlay,
            &title.join("eboot.bin"),
            Retention::Ephemeral,
        );
        assert!(
            !left.exists(),
            "ephemeral empties the overlay before the run"
        );

        crate::mount::clear();
        let _ = std::fs::remove_dir_all(&root);
    }

    /// **After establishing, a console device path is mounted and writable.**
    ///
    /// The point of the manifest entries: `/mnt/usb0` is not a special case in code, it is a
    /// writable overlay like `/data`, so a guest's `mkdir` and write land there rather than
    /// faulting. If this regresses, obSCEne's report sink is back to crashing (D422).
    #[test]
    fn a_device_path_is_writable_after_establishing() {
        let _guard = crate::exclusively();
        crate::mount::clear();

        let root = scratch("device");
        let base = root.join("base");
        let overlay = root.join("overlay");
        let title = root.join("title");
        std::fs::create_dir_all(&title).unwrap();
        std::fs::write(title.join("eboot.bin"), b"guest").unwrap();

        establish(&base, &overlay, &title.join("eboot.bin"), Retention::Retain);
        assert!(
            crate::mount::is_writable("/mnt/usb0/obscene/report.txt"),
            "a USB device path is a writable sandbox after establishing"
        );

        crate::mount::clear();
        let _ = std::fs::remove_dir_all(&root);
    }
}
