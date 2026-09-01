//! Named console profiles: a `Machine`, measured, under a name.
//!
//! A run against a specific console needs its firmware, its release string, its generation and
//! kind - and typing those on every invocation is both tedious and a place for a transcription
//! error to creep in. A profile bundles them under a name (`ps5-cex-12.40`), so `--profile` sets
//! the whole machine at once from a value this project measured and cited.
//!
//! The profiles live in `data/machine-profiles.toml`, keyed by name, with fields matching
//! `Machine`'s own kebab-case serialisation - so a profile is deserialised straight into a
//! `Machine` with no separate mapping to drift.

use orbistoun_core::machine::Machine;

/// The profiles file, parsed once.
fn table() -> &'static toml::Table {
    use std::sync::OnceLock;
    static TABLE: OnceLock<toml::Table> = OnceLock::new();
    TABLE.get_or_init(|| {
        include_str!("../data/machine-profiles.toml")
            .parse::<toml::Table>()
            .expect("the machine-profiles table must parse")
    })
}

/// The machine a named profile presents, or `None` if there is no such profile.
///
/// The name is the exact table key - `ps5-cex-12.40`. A profile deserialises into a `Machine`
/// directly; a key that is present but malformed is a bug in the data file, so it panics rather
/// than being silently skipped.
#[must_use]
pub fn machine(name: &str) -> Option<Machine> {
    let value = table().get(name)?.clone();
    Some(
        value
            .try_into()
            .unwrap_or_else(|e| panic!("machine profile {name} is malformed: {e}")),
    )
}

/// Every profile name, sorted, for listing them and for an error that names the alternatives.
#[must_use]
pub fn names() -> Vec<String> {
    let mut names: Vec<String> = table().keys().cloned().collect();
    names.sort();
    names
}

#[cfg(test)]
mod tests {
    /// The reference profile loads and carries the measured 12.40 values.
    #[test]
    fn the_reference_profile_carries_the_measured_values() {
        let m = super::machine("ps5-cex-12.40").expect("the reference profile exists");
        assert_eq!(
            m.firmware, 0x1240,
            "12.40 in the packed form call 649 answers"
        );
        assert_eq!(m.kernel_release, "0.0-prototype", "what sysctl returned");
        // The software version is a *different* number from the 12.40 firmware (D420).
        let sw = m
            .software_version
            .as_ref()
            .expect("the reference profile states a software version");
        assert_eq!(
            sw.display, "13.090.001",
            "what sceKernelGetSystemSwVersion reports"
        );
        assert_eq!(sw.packed, 0x1309_0001, "and its packed integer");
        assert_ne!(
            u32::from(m.firmware) << 16,
            sw.packed,
            "the software version is not the firmware repackaged - they genuinely differ"
        );
        assert!(m.is_retail(), "cex is retail");
        assert!(!m.is_faster_revision(), "base, not pro");
    }

    /// An unknown name is `None`, so a caller can list the alternatives rather than guess.
    #[test]
    fn an_unknown_profile_is_none() {
        assert!(super::machine("ps5-cex-99.99").is_none());
    }

    /// The names list is non-empty and includes the reference profile.
    #[test]
    fn names_lists_the_reference_profile() {
        assert!(super::names().iter().any(|n| n == "ps5-cex-12.40"));
    }
}
