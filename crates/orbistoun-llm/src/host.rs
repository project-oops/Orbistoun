//! What this machine is.
//!
//! Nothing here concerns inference. It answers what the machine is, which the selector needs
//! and which a run report needs to record as one of its conditions (D181). It depends on
//! nothing else in this crate but the error type, so it can move to a shared home whole.
//! Every measured field is an [`Option`]: `None` means nobody measured it, never zero or a
//! guess, and the selector has defined behaviour for it. Only accelerators reporting through
//! `nvidia-smi` give memory; enumerating Vulkan device-local heaps would cover every vendor
//! but would couple this crate to `ash`.

use std::process::Command;

/// An accelerator, as far as anything could tell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Accelerator {
    /// What it calls itself.
    pub name: String,
    /// Total device memory in MB.
    pub vram_mb: u32,
}

/// The machine this process is running on.
///
/// Probing shells out, so callers keep one value rather than re-probing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Host {
    /// Target operating system, as the compiler saw it.
    pub os: &'static str,
    /// Target architecture, as the compiler saw it.
    pub arch: &'static str,
    /// Logical cores, when the platform will say.
    pub cpu_cores: Option<u32>,
    /// Total system memory in MB, when the platform will say.
    pub ram_mb: Option<u32>,
    /// The first accelerator found, when one reports itself.
    pub accelerator: Option<Accelerator>,
}

impl Host {
    /// Measures this machine.
    ///
    /// Never fails. Anything unmeasurable is `None`: a missing tool, a failed tool and a missing
    /// device all leave the selector deciding without the value.
    #[must_use]
    pub fn probe() -> Self {
        Self {
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
            cpu_cores: std::thread::available_parallelism()
                .ok()
                .and_then(|n| u32::try_from(n.get()).ok()),
            ram_mb: probe_ram_mb(),
            accelerator: probe_accelerator(),
        }
    }

    /// A machine with nothing measurable, for tests and for reasoning about the
    /// unmeasured path without owning an unmeasurable machine.
    #[must_use]
    pub fn unmeasured() -> Self {
        Self {
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
            ..Self::default()
        }
    }

    /// One line, for a report or a log.
    ///
    /// Says `unknown` where a value is missing rather than omitting the field, so the line never
    /// reads as complete when it is not.
    #[must_use]
    pub fn summary(&self) -> String {
        let cores = self
            .cpu_cores
            .map_or_else(|| "unknown".to_owned(), |c| c.to_string());
        let ram = self
            .ram_mb
            .map_or_else(|| "unknown".to_owned(), |m| format!("{m} MB"));
        // An accelerator this build cannot address is called out more loudly than an absent one.
        let gpu = self.accelerator.as_ref().map_or_else(
            || "none reported".to_owned(),
            |a| {
                let usable = if crate::embedded::accelerator_supported() {
                    ""
                } else {
                    ", which this build cannot use"
                };
                format!("{} ({} MB{usable})", a.name, a.vram_mb)
            },
        );
        format!(
            "{}/{}, {cores} cores, {ram} RAM, accelerator: {gpu}",
            self.os, self.arch
        )
    }
}

/// Total system memory, by whatever this platform offers.
///
/// Shells out rather than taking a dependency or using `unsafe`: the value is wanted once per
/// process and `None` is a correct answer.
fn probe_ram_mb() -> Option<u32> {
    #[cfg(target_os = "linux")]
    {
        // MemTotal is in kB and is the first line, but the position is not relied on.
        let text = std::fs::read_to_string("/proc/meminfo").ok()?;
        let kb: u64 = text
            .lines()
            .find_map(|line| line.strip_prefix("MemTotal:"))?
            .split_whitespace()
            .next()?
            .parse()
            .ok()?;
        u32::try_from(kb / 1024).ok()
    }
    #[cfg(target_os = "macos")]
    {
        let out = run("sysctl", &["-n", "hw.memsize"])?;
        let bytes: u64 = out.trim().parse().ok()?;
        u32::try_from(bytes / (1024 * 1024)).ok()
    }
    #[cfg(target_os = "windows")]
    {
        // `wmic` is deprecated and absent on recent images; PowerShell's CIM layer is the supported
        // route.
        let out = run(
            "powershell",
            &[
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory",
            ],
        )?;
        let bytes: u64 = out.trim().parse().ok()?;
        u32::try_from(bytes / (1024 * 1024)).ok()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        None
    }
}

/// The first accelerator that will say how much memory it has, through `nvidia-smi`.
fn probe_accelerator() -> Option<Accelerator> {
    let out = run(
        "nvidia-smi",
        &[
            "--query-gpu=name,memory.total",
            "--format=csv,noheader,nounits",
        ],
    )?;
    let line = out.lines().next()?;
    let (name, mb) = line.split_once(',')?;
    Some(Accelerator {
        name: name.trim().to_owned(),
        vram_mb: mb.trim().parse().ok()?,
    })
}

/// Runs a probe and returns its stdout, or `None` for any reason at all.
///
/// A non-zero exit is `None`: every caller treats "the tool said no" and "there is no tool"
/// the same.
fn run(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

#[cfg(test)]
mod tests {
    use super::Host;

    /// Probing never panics and always returns, including with no accelerator and no `powershell`.
    #[test]
    fn probing_always_returns() {
        let host = Host::probe();
        assert!(!host.os.is_empty());
        assert!(!host.arch.is_empty());
    }

    /// A summary names every field even when nothing was measurable.
    #[test]
    fn a_summary_says_unknown_rather_than_omitting() {
        let summary = Host::unmeasured().summary();
        assert!(summary.contains("unknown cores"), "{summary}");
        assert!(summary.contains("unknown RAM"), "{summary}");
        assert!(summary.contains("none reported"), "{summary}");
    }

    /// A reported core count is at least one, so the selector's CPU tier never sees zero.
    #[test]
    fn a_reported_core_count_is_never_zero() {
        if let Some(cores) = Host::probe().cpu_cores {
            assert!(cores >= 1);
        }
    }
}
