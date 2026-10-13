//! The Tracy tools: where they are, installing the pinned version, and a headless capture.

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

/// The Tracy release the client in a `tracy` build speaks to (D829): `tracy-client` 0.19 bundles
/// Tracy 0.14.1, and a viewer or capture tool of another version refuses its protocol. A test
/// holds this to the client the lockfile pins.
pub const TRACY_VERSION: &str = "0.14.1";

/// A tool's file name on this host.
fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    }
}

/// A directory holding the Tracy tools: the capture tool and the exporter at least, and the viewer
/// where the release has one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tools {
    dir: PathBuf,
}

impl Tools {
    /// The tools in `dir`, when the capture tool and the exporter are there.
    #[must_use]
    pub fn at(dir: &Path) -> Option<Self> {
        let tools = Self {
            dir: dir.to_path_buf(),
        };
        (tools.capture().is_file() && tools.exporter().is_file()).then_some(tools)
    }

    /// The first of `candidates` that holds the tools, then each directory on `PATH`.
    pub fn find(candidates: impl IntoIterator<Item = PathBuf>) -> Option<Self> {
        let on_path = std::env::var_os("PATH")
            .map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
            .unwrap_or_default();
        candidates
            .into_iter()
            .chain(on_path)
            .find_map(|dir| Self::at(&dir))
    }

    /// Where [`install`] puts the pinned version under a data root: `tools/tracy-<version>`.
    #[must_use]
    pub fn default_dir(data_root: &Path) -> PathBuf {
        data_root
            .join("tools")
            .join(format!("tracy-{TRACY_VERSION}"))
    }

    /// The directory they are in.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// `tracy-capture`: records a run to a file without a window.
    #[must_use]
    pub fn capture(&self) -> PathBuf {
        self.dir.join(exe("tracy-capture"))
    }

    /// `tracy-csvexport`: a recorded run's zones and plots as CSV.
    #[must_use]
    pub fn exporter(&self) -> PathBuf {
        self.dir.join(exe("tracy-csvexport"))
    }

    /// `tracy-profiler`: the viewer.
    #[must_use]
    pub fn viewer(&self) -> PathBuf {
        self.dir.join(exe("tracy-profiler"))
    }

    /// The version the tools say they are - `0.14.1` from `tracy-csvexport 0.14.1 / <commit>` -
    /// or `None` when they do not answer.
    #[must_use]
    pub fn version(&self) -> Option<String> {
        let out = Command::new(self.exporter())
            .arg("--version")
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        text.split_whitespace().nth(1).map(str::to_owned)
    }

    /// Whether they are the version a `tracy` build's client speaks.
    #[must_use]
    pub fn matches_client(&self) -> bool {
        self.version().as_deref() == Some(TRACY_VERSION)
    }

    /// Opens the viewer on a recorded run, or connected to a running one at `127.0.0.1:port`.
    ///
    /// # Errors
    ///
    /// When the viewer is not there or cannot be started.
    pub fn open_viewer(&self, trace: Option<&Path>, port: Option<u16>) -> io::Result<Child> {
        let mut command = Command::new(self.viewer());
        if let Some(trace) = trace {
            command.arg(trace);
        } else if let Some(port) = port {
            command.args(["-a", "127.0.0.1", "-p", &port.to_string()]);
        }
        command.spawn()
    }
}

/// Where this host's build of the pinned release is published, or `None` for a host Tracy
/// publishes none for.
#[must_use]
pub fn release_url() -> Option<String> {
    let platform = if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        return None;
    };
    Some(format!(
        "https://github.com/wolfpld/tracy/releases/download/v{TRACY_VERSION}/{platform}-{TRACY_VERSION}.zip"
    ))
}

/// Downloads the pinned release into `dir` and unpacks it there, with the host's own `curl` and
/// `tar` - both ship with Windows 10 and later and with every desktop Linux - so nothing is
/// compiled in for a step taken once.
///
/// # Errors
///
/// Naming the step that failed: no release for this host, the download, or the unpacking, or a
/// release that turned out not to hold the tools.
pub fn install(dir: &Path) -> Result<Tools, String> {
    let url = release_url().ok_or("Tracy publishes no build for this host")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    let archive = dir.join(format!("tracy-{TRACY_VERSION}.zip"));
    let fetched = Command::new("curl")
        .args(["-fsSL", "-o"])
        .arg(&archive)
        .arg(&url)
        .status()
        .map_err(|e| format!("running curl: {e}"))?;
    if !fetched.success() {
        return Err(format!("downloading {url} failed ({fetched})"));
    }
    let unpacked = Command::new("tar")
        .arg("-xf")
        .arg(&archive)
        .arg("-C")
        .arg(dir)
        .status()
        .map_err(|e| format!("running tar: {e}"))?;
    let _ = std::fs::remove_file(&archive);
    if !unpacked.success() {
        return Err(format!(
            "unpacking {} failed ({unpacked})",
            archive.display()
        ));
    }
    Tools::at(dir).ok_or_else(|| {
        format!(
            "the release unpacked into {} holds no tracy-capture and tracy-csvexport",
            dir.display()
        )
    })
}

/// A local TCP port nothing is listening on now, for a run's client and its capture to meet on -
/// so two runs profiled at once do not both reach for 8086.
///
/// # Errors
///
/// When the host gives no port.
pub fn free_port() -> io::Result<u16> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    Ok(listener.local_addr()?.port())
}

/// A headless capture under way: `tracy-capture` waiting for, then recording, a client.
#[derive(Debug)]
pub struct Capture {
    child: Child,
    output: PathBuf,
}

/// What a finished capture recorded, as the capture tool reports it.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CaptureSummary {
    /// Frames - guest flips.
    pub frames: u64,
    /// Seconds from its first event to its last.
    pub seconds: f64,
    /// Zones recorded.
    pub zones: u64,
}

impl CaptureSummary {
    /// Its frames over its seconds.
    #[must_use]
    pub fn fps(&self) -> f64 {
        if self.seconds > 0.0 {
            crate::as_f64(self.frames) / self.seconds
        } else {
            0.0
        }
    }

    /// The summary in the capture tool's own words: the lines `Frames: 907`, `Time span: 11.09 s`
    /// and `Zones: 606,322`, among its progress lines, which end in carriage returns.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let mut summary = Self::default();
        for line in text.split(['\r', '\n']) {
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let number = || -> Option<f64> {
                value
                    .split_whitespace()
                    .next()?
                    .replace(',', "")
                    .parse()
                    .ok()
            };
            match key.trim() {
                "Frames" => summary.frames = number().map_or(0, |n| n as u64),
                "Time span" => summary.seconds = number().unwrap_or(0.0),
                "Zones" => summary.zones = number().map_or(0, |n| n as u64),
                _ => {}
            }
        }
        summary
    }
}

impl Capture {
    /// Starts recording whatever client listens on `127.0.0.1:port` into `output`, for `seconds`
    /// from when it connects; it waits for the client to start.
    ///
    /// # Errors
    ///
    /// When the capture tool cannot be started.
    pub fn start(tools: &Tools, output: &Path, port: u16, seconds: u64) -> io::Result<Self> {
        if let Some(parent) = output.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let child = Command::new(tools.capture())
            .arg("-o")
            .arg(output)
            .args(["-f", "-a", "127.0.0.1", "-p", &port.to_string()])
            .args(["-s", &seconds.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        Ok(Self {
            child,
            output: output.to_path_buf(),
        })
    }

    /// Waits for the capture to be written, answering what it recorded.
    ///
    /// # Errors
    ///
    /// When the tool fails or writes no file - a client that never connected.
    pub fn finish(self) -> io::Result<(PathBuf, CaptureSummary)> {
        let out = self.child.wait_with_output()?;
        let text = String::from_utf8_lossy(&out.stdout);
        if !self.output.is_file() {
            return Err(io::Error::other(format!(
                "the capture wrote nothing - did the run start its Tracy client? {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        Ok((self.output, CaptureSummary::parse(&text)))
    }

    /// Stops a capture that will not be needed, such as one whose run failed to start.
    pub fn abandon(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::{CaptureSummary, TRACY_VERSION, Tools, release_url};

    /// The capture tool's closing lines, progress lines and all.
    #[test]
    fn a_capture_summary_reads_the_tool_s_own_words() {
        let text = concat!(
            "Connecting to 127.0.0.1:8086...\rTimer resolution: 0 ns\n",
            "\r1.2 Mbps / 12 MB\nFrames: 907\nTime span: 11.09 s\nZones: 606,322\n",
            "Elapsed time: 11.22 s\nSaving trace... done!\n",
        );
        let summary = CaptureSummary::parse(text);
        assert_eq!(summary.frames, 907);
        assert!((summary.seconds - 11.09).abs() < 1e-9);
        assert_eq!(summary.zones, 606_322);
        assert!((summary.fps() - 907.0 / 11.09).abs() < 1e-9);
    }

    /// The version this crate pins is the one the lockfile's client bundles: `tracy-client-sys`
    /// 0.30 is Tracy 0.14.1. A client bumped without this is caught here, not by a viewer refusing
    /// its protocol.
    #[test]
    fn the_pinned_version_is_the_lockfile_s_client() {
        let lock = include_str!("../../../Cargo.lock");
        let sys = lock
            .split("[[package]]")
            .find(|package| package.contains("name = \"tracy-client-sys\""))
            .and_then(|package| {
                package
                    .lines()
                    .find_map(|line| line.strip_prefix("version = "))
                    .map(|version| version.trim_matches('"').to_owned())
            });
        let bundled = match sys.as_deref() {
            Some("0.30.0") => "0.14.1",
            other => panic!("tracy-client-sys {other:?}: say which Tracy it bundles"),
        };
        assert_eq!(bundled, TRACY_VERSION);
        assert!(release_url().is_some_and(|url| url.contains(TRACY_VERSION)));
    }

    /// A directory without the tools is none, one with them is.
    #[test]
    fn tools_are_found_where_both_files_are() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(Tools::at(dir.path()), None);
        let name = |tool: &str| {
            if cfg!(windows) {
                format!("{tool}.exe")
            } else {
                tool.to_owned()
            }
        };
        std::fs::write(dir.path().join(name("tracy-capture")), b"").expect("written");
        assert_eq!(Tools::at(dir.path()), None, "the exporter too");
        std::fs::write(dir.path().join(name("tracy-csvexport")), b"").expect("written");
        assert!(Tools::at(dir.path()).is_some());
        assert_eq!(
            Tools::find([
                std::path::PathBuf::from("/nowhere"),
                dir.path().to_path_buf()
            ])
            .map(|tools| tools.dir().to_path_buf()),
            Some(dir.path().to_path_buf())
        );
    }
}
