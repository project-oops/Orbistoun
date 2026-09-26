//! Named machine profiles: a measured `Machine` under a name.
//!
//! A profile bundles firmware, release string, generation and kind under a name
//! (`prospero-cex-12.40`), so `--profile` sets the whole machine from measured values. The
//! profiles live in `data/machine-profiles.toml`, with fields matching `Machine`'s own
//! kebab-case serialisation, so a profile deserialises straight into a `Machine`.

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
/// The name is the exact table key, such as `prospero-cex-12.40`.
///
/// # Panics
///
/// If the profile is present but malformed, which is a defect in the data file.
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
        let m = super::machine("prospero-cex-12.40").expect("the reference profile exists");
        assert_eq!(
            m.firmware, 0x1240,
            "12.40 in the packed form call 649 answers"
        );
        assert_eq!(m.kernel_release, "0.0-prototype", "what sysctl returned");
        // The system-software version is a distinct measured number from the 12.40 release
        // and is never derived from it.
        let sw = m
            .software_version
            .as_ref()
            .expect("the reference profile states a software version");
        assert_eq!(
            sw.display, "13.090.001",
            "what sceKernelGetSystemSwVersion reports"
        );
        assert_eq!(sw.packed, 0x1309_0001, "and its packed integer");
        // The knobs `135-sysctl/names` reads back that belong to a machine rather than the
        // platform: the kernel build banner, its SDK number and the hardware model (D675).
        assert_eq!(
            m.kernel_version, "r226974/releases/12.40 Nov 27 2025 02:23:38",
            "kern.version, as the console wrote it"
        );
        assert_eq!(
            m.kernel_version.len(),
            43,
            "the measured extent, one short of the 0x2c length that counts the terminator"
        );
        assert_eq!(
            m.kernel_sdk_version, 0x1240_0009,
            "kern.sdk_version - 12.400.009, the system software the run header names"
        );
        assert_eq!(
            m.hardware_model.trim_end(),
            "100-000000189",
            "hw.model's text"
        );
        assert_eq!(
            m.hardware_model.len(),
            47,
            "trailing spaces kept: the console wrote 47 bytes, and a trimmed string is a different answer"
        );
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
        assert!(super::machine("prospero-cex-99.99").is_none());
    }

    /// The names list is non-empty and includes the reference profile.
    #[test]
    fn names_lists_the_reference_profile() {
        assert!(super::names().iter().any(|n| n == "prospero-cex-12.40"));
    }
}
