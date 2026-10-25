//! Asks `spirv-val`, the format's own validator, whether a module is valid for a Vulkan 1.3 device.
//!
//! The builder checks what it is placed to check ([`crate::Builder::check`]); validity is the
//! validator's to decide, since a crate that judges its own output proves nothing. A driver handed
//! an invalid module may accept it, miscompile it or fault, so a module that passes on one device
//! is not thereby valid: translation tests and the shader census run every module they make through
//! here.
//!
//! The validator is an external program from the Vulkan SDK. It is found at `ORBISTOUN_SPIRV_VAL`,
//! then in the SDK `VULKAN_SDK` names, then on `PATH`; where it is not found the answer says so
//! rather than passing the module.

use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// The environment variable naming the validator's executable, ahead of every other place.
pub const VALIDATOR_VARIABLE: &str = "ORBISTOUN_SPIRV_VAL";

/// The environment a module is validated for: what the translator's modules are made for.
pub const TARGET_ENVIRONMENT: &str = "vulkan1.3";

/// What the validator said of a module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Validation {
    /// Valid for [`TARGET_ENVIRONMENT`].
    Valid,
    /// Invalid: the validator's message, its first error and the instruction it names.
    Invalid(String),
    /// Not asked: no validator was found, or it could not be run.
    Unavailable(String),
}

impl Validation {
    /// The validator's message, where the module is invalid.
    #[must_use]
    pub fn invalid(&self) -> Option<&str> {
        match self {
            Self::Invalid(message) => Some(message),
            Self::Valid | Self::Unavailable(_) => None,
        }
    }
}

/// The validator's executable: [`VALIDATOR_VARIABLE`], then the `Bin` directory of the SDK
/// `VULKAN_SDK` names, then the bare name for `PATH` to resolve.
#[must_use]
pub fn validator() -> PathBuf {
    let name = if cfg!(windows) {
        "spirv-val.exe"
    } else {
        "spirv-val"
    };
    if let Some(path) = std::env::var_os(VALIDATOR_VARIABLE) {
        return PathBuf::from(path);
    }
    if let Some(sdk) = std::env::var_os("VULKAN_SDK") {
        for bin in ["Bin", "bin"] {
            let path = PathBuf::from(&sdk).join(bin).join(name);
            if path.is_file() {
                return path;
            }
        }
    }
    PathBuf::from(name)
}

/// Whether a validator can be run here.
#[must_use]
pub fn available() -> bool {
    Command::new(validator())
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Validates `module`, as words, for [`TARGET_ENVIRONMENT`].
#[must_use]
pub fn validate(module: &[u32]) -> Validation {
    let bytes: Vec<u8> = module.iter().flat_map(|word| word.to_le_bytes()).collect();
    let child = Command::new(validator())
        .args(["--target-env", TARGET_ENVIRONMENT, "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(error) => {
            return Validation::Unavailable(format!(
                "{} could not be run ({error}); set {VALIDATOR_VARIABLE} or VULKAN_SDK",
                validator().display()
            ));
        }
    };
    // Written from another thread: a validator that answers before reading everything would
    // otherwise leave both sides waiting on a full pipe.
    let input = child.stdin.take();
    let writer = std::thread::spawn(move || input.map(|mut input| input.write_all(&bytes)));
    let output = child.wait_with_output();
    let written = writer.join();
    let output = match output {
        Ok(output) => output,
        Err(error) => return Validation::Unavailable(format!("the validator failed: {error}")),
    };
    if output.status.success() {
        return Validation::Valid;
    }
    let mut message = String::from_utf8_lossy(&output.stderr).into_owned();
    message.push_str(&String::from_utf8_lossy(&output.stdout));
    let message = message.trim().to_owned();
    if message.is_empty() {
        let fed = matches!(written, Ok(Some(Ok(())) | None));
        return Validation::Unavailable(format!(
            "the validator exited {} without a message{}",
            output.status,
            if fed {
                ""
            } else {
                ", the module not fed to it"
            }
        ));
    }
    Validation::Invalid(message)
}

#[cfg(test)]
mod tests {
    use super::{Validation, available, validate};

    /// A module the validator refuses comes back with its message, and one it accepts as valid.
    /// Without a validator the answer is that none was asked, never that the module is valid.
    #[test]
    fn the_validator_tells_a_valid_module_from_an_invalid_one() {
        let valid = crate::Builder::new().finish();
        if !available() {
            assert!(matches!(validate(&valid), Validation::Unavailable(_)));
            println!(
                "[spirv-val] SKIPPED - no validator: {}",
                super::validator().display()
            );
            return;
        }
        // The header alone, with no capability or memory model: refused.
        let invalid = validate(&valid);
        assert!(invalid.invalid().is_some(), "{invalid:?}");
        assert_eq!(
            validate(&crate::minimal_compute_module([1, 1, 1])),
            Validation::Valid
        );
    }
}
