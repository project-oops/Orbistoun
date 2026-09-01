//! What this machine is.
//!
//! # This module is not about models, and that is deliberate
//!
//! Nothing here mentions inference. It answers "what is this machine", which the
//! selector happens to need and which a run report needs for an unrelated reason:
//! D046 requires a report to embed its own inputs so that a difference between two
//! runs can be attributed to the change rather than to drift, and *the machine* is an
//! input nothing currently records. Two runs compared across two machines are not
//! comparable today, and nothing says so.
//!
//! So this file is written to be **lifted whole** into a shared home once there is
//! one - it has no dependency on the rest of this crate beyond the error type, and no
//! knowledge of what it is being asked for. See the note in the crate README.
//!
//! # Absent is a real answer
//!
//! Every field that has to be measured is an [`Option`], and `None` means *nobody
//! measured it*, never zero and never a guess. A machine that cannot report its VRAM
//! is a normal machine, not a broken one, and the selector has a defined behaviour for
//! it. Principle 3: an explicit "not known" beats a plausible number, and the number
//! here would drive a multi-gigabyte download.
//!
//! # What is deliberately missing
//!
//! Only NVIDIA accelerators report memory, via `nvidia-smi`. AMD and Intel report
//! nothing and are therefore recorded as nothing. The better answer is already
//! sitting in this repository: `orbistoun-gpu-vulkan` loads Vulkan at runtime and can
//! enumerate device-local heaps on any vendor. Doing that here would couple this
//! crate to `ash` for a single number, so it is left as the known improvement rather
//! than a hidden limitation.

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
/// Cheap to construct and cheap to copy around, so callers keep one rather than
/// re-probing - probing shells out, and doing that on a hot path would be silly.
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
    /// Never fails. Everything unmeasurable comes back `None`, because there is no
    /// useful distinction between "the tool is absent", "the tool failed" and "there
    /// is no such device" - all three mean the selector has to decide without it.
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
    /// Says `unknown` where a value is missing rather than omitting the field: a line
    /// that silently drops what it could not measure reads as a complete description.
    #[must_use]
    pub fn summary(&self) -> String {
        let cores = self
            .cpu_cores
            .map_or_else(|| "unknown".to_owned(), |c| c.to_string());
        let ram = self
            .ram_mb
            .map_or_else(|| "unknown".to_owned(), |m| format!("{m} MB"));
        // An accelerator this build cannot address is worth saying twice as loudly as
        // one that is absent: the second is a machine, the first is a surprise.
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
/// Shelling out rather than taking a dependency or writing `unsafe`: the value is
/// wanted once per process, an `Option` is a correct answer, and principle 4 says
/// `unsafe` should be rare and confined to guest memory - a RAM figure is not worth
/// spending any.
fn probe_ram_mb() -> Option<u32> {
    #[cfg(target_os = "linux")]
    {
        // MemTotal is in kB and is the first line, but do not rely on the position.
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
        // `wmic` is deprecated and absent on recent images, so ask PowerShell's CIM
        // layer, which is the supported route and present everywhere `wmic` was.
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

/// The first accelerator that will say how much memory it has.
///
/// NVIDIA only, and the module documentation says so rather than leaving the reader
/// to infer it from a missing branch.
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
/// A non-zero exit is `None` rather than an error: every caller here treats "the tool
/// said no" and "there is no tool" identically, and inventing a distinction the
/// callers do not use would be structure for its own sake.
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

    /// Probing never panics and never fails, on any machine.
    ///
    /// This runs in CI, on a laptop, and inside a container with no accelerator and no
    /// `powershell`. All three are supported and all three must return.
    #[test]
    fn probing_always_returns() {
        let host = Host::probe();
        assert!(!host.os.is_empty());
        assert!(!host.arch.is_empty());
    }

    /// A summary names every field even when nothing was measurable.
    ///
    /// The failure this guards is a line that reads as a full description of the
    /// machine while silently omitting what it could not find.
    #[test]
    fn a_summary_says_unknown_rather_than_omitting() {
        let summary = Host::unmeasured().summary();
        assert!(summary.contains("unknown cores"), "{summary}");
        assert!(summary.contains("unknown RAM"), "{summary}");
        assert!(summary.contains("none reported"), "{summary}");
    }

    /// Cores, if reported at all, are at least one.
    ///
    /// Zero would pass every `>=` comparison in the selector's CPU tier and quietly
    /// choose the smallest model on a large machine.
    #[test]
    fn a_reported_core_count_is_never_zero() {
        if let Some(cores) = Host::probe().cpu_cores {
            assert!(cores >= 1);
        }
    }
}
