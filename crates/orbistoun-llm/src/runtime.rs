//! A GPU inference runtime, downloaded and supervised like any other input.
//!
//! Inference must run on the GPU, need no action from the user, and work in a portable install.
//! A compiled-in accelerator backend needs a vendor toolkit at build time, a user-installed
//! server needs action, and the CPU is too slow, so this downloads a pinned prebuilt
//! `llama-server`, starts it, and speaks the OpenAI-shaped wire of [`crate::online`] (D219). The
//! Vulkan build is used: it is small, covers every GPU vendor, and any machine running orbistoun
//! already has a Vulkan driver; its bundled CPU backends cover a machine with no usable device.
//! [`Runtime`] kills the child on drop, which covers every ordinary exit but not a hard crash.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::Error;
use crate::catalog::Offline;

/// The release this project pins: a tag, so behaviour and downloads change only with a commit.
pub const LLAMA_TAG: &str = "b10612";

/// Where releases come from.
const RELEASES: &str = "https://github.com/ggml-org/llama.cpp/releases/download";

/// How long to wait for the server to answer after it is started; the first start loads
/// gigabytes of weights onto a device.
pub const READY_TIMEOUT: Duration = Duration::from_secs(180);

/// The archive for this platform, and the server binary inside it.
///
/// `None` where no prebuilt Vulkan build is published; the machine then falls back to the
/// in-process engine, and the caller says so.
#[must_use]
pub fn asset() -> Option<(String, &'static str)> {
    let (suffix, binary) = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => ("bin-win-vulkan-x64.zip", "llama-server.exe"),
        ("linux", "x86_64") => ("bin-ubuntu-vulkan-x64.tar.gz", "llama-server"),
        ("linux", "aarch64") => ("bin-ubuntu-vulkan-arm64.tar.gz", "llama-server"),
        _ => return None,
    };
    Some((format!("llama-{LLAMA_TAG}-{suffix}"), binary))
}

/// Whether this platform has a prebuilt runtime to fetch.
#[must_use]
pub fn available() -> bool {
    asset().is_some()
}

/// An accelerator the runtime can address.
///
/// Reported by the runtime, so it answers whether this backend can use the device, not only
/// whether a device is present.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Accelerator {
    /// The runtime's own identifier, and what `--device` takes - `Vulkan0`.
    pub id: String,
    /// What the driver calls it.
    pub name: String,
    /// Total device memory in MB, when the listing gives it.
    pub total_mb: Option<u32>,
    /// Free device memory in MB, when the listing gives it.
    pub free_mb: Option<u32>,
}

/// How many lines of the runtime's own output to keep: enough for the device enumeration and
/// the layer placement.
const LOG_LINES: usize = 400;

/// A supervised `llama-server`, and the port it answers on.
#[derive(Debug)]
pub struct Runtime {
    child: Child,
    port: u16,
    model: String,
    accelerator: Option<Accelerator>,
    log: Arc<Mutex<VecDeque<String>>>,
}

impl Runtime {
    /// Where the runtime binaries live beneath a root.
    #[must_use]
    pub fn dir(root: &Path) -> PathBuf {
        root.join("runtime").join(LLAMA_TAG)
    }

    /// Whether the runtime is already on disk.
    #[must_use]
    pub fn is_downloaded(root: &Path) -> bool {
        asset().is_some_and(|(_, binary)| {
            std::fs::metadata(Self::dir(root).join(binary)).is_ok_and(|m| m.len() > 0)
        })
    }

    /// Fetches the runtime if it is missing.
    ///
    /// # Errors
    ///
    /// If this platform has no prebuilt build, or the download or unpack fails.
    pub fn ensure_downloaded(root: &Path) -> Result<PathBuf, Error> {
        let (archive, binary) = asset().ok_or_else(|| {
            Error::Download(format!(
                "no prebuilt GPU runtime is published for {}/{}",
                std::env::consts::OS,
                std::env::consts::ARCH
            ))
        })?;
        let dir = Self::dir(root);
        let server = dir.join(binary);
        if std::fs::metadata(&server).is_ok_and(|m| m.len() > 0) {
            return Ok(server);
        }

        std::fs::create_dir_all(&dir)
            .map_err(|e| Error::Download(format!("creating {}: {e}", dir.display())))?;
        let url = format!("{RELEASES}/{LLAMA_TAG}/{archive}");
        let into = dir.join(&archive);
        tracing::info!(%url, "fetching the GPU inference runtime - this happens once");
        crate::embedded::fetch(&url, &into, 35, LLAMA_TAG)?;
        unpack(&into, &dir)?;
        let _ = std::fs::remove_file(&into);

        if std::fs::metadata(&server).is_ok_and(|m| m.len() > 0) {
            Ok(server)
        } else {
            // The published archive's layout is someone else's; say what was expected and where.
            Err(Error::Download(format!(
                "{archive} unpacked without a {binary} in {}",
                dir.display()
            )))
        }
    }

    /// Every accelerator this runtime can address, by asking it.
    ///
    /// Cheap (no model is loaded) and vendor-neutral, unlike the `nvidia-smi` probe in
    /// [`crate::host`].
    ///
    /// # Errors
    ///
    /// If the runtime cannot be fetched or will not run.
    pub fn devices(root: &Path) -> Result<Vec<Accelerator>, Error> {
        let server = Self::ensure_downloaded(root)?;
        let output = Command::new(&server)
            .arg("--list-devices")
            .output()
            .map_err(|e| Error::Model(format!("listing devices with {}: {e}", server.display())))?;
        // The listing goes to stdout and the runtime's logging to stderr; reading the wrong one looks
        // like a machine with no device.
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(accelerator_from)
            .collect())
    }

    /// Downloads what is missing, starts a server, and waits for it to answer.
    ///
    /// # Errors
    ///
    /// If the runtime or the model cannot be fetched, the process will not start, or it
    /// does not become ready inside [`READY_TIMEOUT`].
    pub fn start(model: &Offline, root: &Path, models_dir: &Path) -> Result<Self, Error> {
        let server = Self::ensure_downloaded(root)?;
        let accelerator = Self::devices(root)?.into_iter().next();
        let weights = crate::embedded::ensure_model(model, models_dir)?;
        let port = free_port()?;

        tracing::info!(
            model = %model.id,
            port,
            device = accelerator.as_ref().map_or("none", |a| a.id.as_str()),
            "starting the inference runtime"
        );
        let mut command = Command::new(&server);
        command
            .arg("--model")
            .arg(&weights)
            .args(["--host", "127.0.0.1"])
            .args(["--port", &port.to_string()])
            // Everything on the device: llama.cpp places what it can and leaves the rest on the processor,
            // so this is a ceiling rather than a demand.
            .args(["--n-gpu-layers", "999"])
            // One request at a time, matching how this crate asks.
            .args(["--parallel", "1"])
            // No thinking: a reasoning model puts its working outside `message.content`, so a short reply
            // would come back empty. This covers every reasoning model, not one family's template.
            .args(["--reasoning", "off"]);

        // The device is named, so an unusable one refuses to start rather than silently falling back to
        // the processor; a started runtime is then evidence of acceleration.
        if let Some(device) = &accelerator {
            command.args(["--device", &device.id]);
        }

        let child = command
            // Both streams piped and drained: logging goes to stderr but the device enumeration to stdout,
            // the only evidence an accelerator was used. A piped stream nobody reads fills and stops the
            // child, so the drains are required.
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| Error::Model(format!("starting {}: {e}", server.display())))?;

        let mut child = child;
        let log = Arc::new(Mutex::new(VecDeque::with_capacity(LOG_LINES)));
        if let Some(stream) = child.stdout.take() {
            drain(stream, Arc::clone(&log));
        }
        if let Some(stream) = child.stderr.take() {
            drain(stream, Arc::clone(&log));
        }

        let runtime = Self {
            child,
            port,
            model: model.id.clone(),
            accelerator,
            log,
        };
        runtime.wait_until_ready()?;
        tracing::info!(model = %runtime.model, port, "GPU inference runtime ready");
        Ok(runtime)
    }

    /// What the runtime said about itself while starting.
    #[must_use]
    pub fn log(&self) -> Vec<String> {
        self.log
            .lock()
            .map(|lines| lines.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// The accelerator this runtime was started on, if any.
    ///
    /// Falling back to the processor is silent and successful, so a reply proves nothing; this does.
    /// The device was named on the command line, so a live [`Runtime`] holding `Some` runs on a
    /// device the runtime accepted.
    #[must_use]
    pub fn accelerator(&self) -> Option<&Accelerator> {
        self.accelerator.as_ref()
    }

    /// The OpenAI-shaped endpoint this server answers on.
    #[must_use]
    pub fn endpoint(&self) -> String {
        format!("http://127.0.0.1:{}/v1/chat/completions", self.port)
    }

    /// Polls until the server answers, or gives up.
    fn wait_until_ready(&self) -> Result<(), Error> {
        let health = format!("http://127.0.0.1:{}/health", self.port);
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .map_err(|e| Error::Transport(e.to_string()))?;
        let started = Instant::now();
        while started.elapsed() < READY_TIMEOUT {
            if client
                .get(&health)
                .send()
                .is_ok_and(|r| r.status().is_success())
            {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        Err(Error::Model(format!(
            "the inference runtime did not answer within {} seconds of starting",
            READY_TIMEOUT.as_secs()
        )))
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        // Covers every ordinary exit; a hard crash of this process orphans the child.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Reads one of the child's streams into the shared log until it closes.
///
/// Both streams share one buffer; nothing reconstructs an ordering from it.
fn drain(stream: impl Read + Send + 'static, sink: Arc<Mutex<VecDeque<String>>>) {
    std::thread::spawn(move || {
        for line in BufReader::new(stream).lines().map_while(Result::ok) {
            let Ok(mut sink) = sink.lock() else { return };
            if sink.len() == LOG_LINES {
                sink.pop_front();
            }
            sink.push_back(line);
        }
    });
}

/// Reads one device out of a `--list-devices` listing.
///
/// The listing is a user-facing interface, where the log reports the device only at a verbosity
/// that prints every layer. Memory figures are optional, so a format change does not read as
/// "no GPU".
fn accelerator_from(line: &str) -> Option<Accelerator> {
    // Trimmed because the listing is written with carriage returns.
    let line = line.trim();
    let (id, rest) = line.split_once(": ")?;
    // Indented entries only: the `Available devices:` heading also contains a colon.
    if id.is_empty() || id.contains(char::is_whitespace) {
        return None;
    }
    let (name, memory) = match rest.split_once(" (") {
        Some((name, memory)) => (name.trim(), Some(memory)),
        None => (rest.trim(), None),
    };
    let mut figures = memory
        .unwrap_or_default()
        .split(',')
        .filter_map(|part| part.split_whitespace().next())
        .filter_map(|number| number.parse::<u32>().ok());
    Some(Accelerator {
        id: id.to_owned(),
        name: name.to_owned(),
        total_mb: figures.next(),
        free_mb: figures.next(),
    })
}

/// A port nothing is listening on.
///
/// Asked for and released, so something else could take it in between; a fixed port would
/// collide with every other copy of this.
fn free_port() -> Result<u16, Error> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| Error::Transport(format!("finding a free port: {e}")))?;
    listener
        .local_addr()
        .map(|addr| addr.port())
        .map_err(|e| Error::Transport(format!("reading the bound port: {e}")))
}

/// Unpacks a release archive.
fn unpack(archive: &Path, into: &Path) -> Result<(), Error> {
    let name = archive.file_name().unwrap_or_default().to_string_lossy();
    if name.ends_with(".zip") {
        return unzip(archive, into);
    }
    Err(Error::Download(format!(
        "{name} is not an archive this build can unpack"
    )))
}

/// Unpacks a zip, flattening it.
///
/// Flattened so another project's archive layout is not this crate's contract, and because the
/// backend loader finds `ggml-*` beside the executable. Entries whose names escape the directory
/// are refused rather than sanitised.
fn unzip(archive: &Path, into: &Path) -> Result<(), Error> {
    let file = std::fs::File::open(archive)
        .map_err(|e| Error::Download(format!("opening {}: {e}", archive.display())))?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| Error::Download(format!("reading {}: {e}", archive.display())))?;

    for index in 0..zip.len() {
        let mut entry = zip
            .by_index(index)
            .map_err(|e| Error::Download(format!("reading entry {index}: {e}")))?;
        if entry.is_dir() {
            continue;
        }
        let Some(name) = entry.enclosed_name() else {
            // `enclosed_name` is `None` for an absolute path or one climbing out with `..`; such an archive
            // is refused.
            return Err(Error::Download(format!(
                "{} contains an entry that would write outside its directory",
                archive.display()
            )));
        };
        let Some(flat) = name.file_name() else {
            continue;
        };
        let out = into.join(flat);
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry
            .read_to_end(&mut bytes)
            .map_err(|e| Error::Download(format!("unpacking {}: {e}", flat.to_string_lossy())))?;
        std::fs::write(&out, &bytes)
            .map_err(|e| Error::Download(format!("writing {}: {e}", out.display())))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            if let Some(mode) = entry.unix_mode() {
                let _ = std::fs::set_permissions(&out, std::fs::Permissions::from_mode(mode));
            }
        }
    }
    Ok(())
}

/// A [`Runtime`] and the client that talks to it, as one engine.
///
/// Owns the server, so dropping the engine stops it.
#[derive(Debug)]
pub struct ManagedEngine {
    runtime: Runtime,
    client: crate::online::OnlineEngine,
    model: String,
}

impl ManagedEngine {
    /// Starts a runtime for `model` and returns an engine over it.
    ///
    /// # Errors
    ///
    /// If the runtime cannot be fetched, started, or reached.
    pub fn start(
        model: &Offline,
        root: &Path,
        models_dir: &Path,
        catalog: &crate::Catalog,
    ) -> Result<Self, Error> {
        let runtime = Runtime::start(model, root, models_dir)?;
        let integration = crate::Integration {
            id: "managed".to_owned(),
            name: "managed".to_owned(),
            kind: crate::Kind::Online,
            source: String::new(),
            model: Some(model.id.clone()),
            endpoint: Some(runtime.endpoint()),
            api_key: None,
            device: crate::Device::Gpu,
        };
        let client = crate::online::OnlineEngine::new(&integration, catalog)?;
        Ok(Self {
            runtime,
            client,
            model: model.id.clone(),
        })
    }

    /// The accelerator this is running on, if any.
    #[must_use]
    pub fn accelerator(&self) -> Option<&Accelerator> {
        self.runtime.accelerator()
    }
}

impl crate::Engine for ManagedEngine {
    fn describe(&self) -> String {
        match self.runtime.accelerator() {
            Some(device) => format!("{} on {} ({})", self.model, device.id, device.name),
            None => format!("{} on the processor, via the managed runtime", self.model),
        }
    }

    fn model(&self) -> String {
        self.model.clone()
    }

    fn complete(&self, request: &crate::Request) -> Result<String, Error> {
        self.client.complete(request)
    }
}

#[cfg(test)]
mod tests {
    use super::{LLAMA_TAG, Runtime, asset, free_port};

    /// The runtime release is pinned to a tag.
    #[test]
    fn the_runtime_release_is_pinned() {
        assert!(LLAMA_TAG.starts_with('b'), "{LLAMA_TAG}");
        assert!(!LLAMA_TAG.contains("latest"), "{LLAMA_TAG}");
    }

    /// The asset name carries the pinned tag and asks for the Vulkan build; a wrong name is a 404
    /// minutes into a first run.
    #[test]
    fn the_asset_is_the_vulkan_build_for_this_platform() {
        if let Some((archive, binary)) = asset() {
            assert!(archive.contains(LLAMA_TAG), "{archive}");
            assert!(archive.contains("vulkan"), "{archive}");
            assert!(binary.starts_with("llama-server"), "{binary}");
        }
    }

    /// Nothing is downloaded by asking where things go.
    #[test]
    fn asking_where_the_runtime_lives_touches_no_disk() {
        let dir = tempfile::tempdir().expect("temp dir");
        assert!(!Runtime::is_downloaded(dir.path()));
        assert!(Runtime::dir(dir.path()).starts_with(dir.path()));
        assert!(
            std::fs::read_dir(dir.path())
                .expect("readable")
                .next()
                .is_none()
        );
    }

    /// A device is read out of a listing captured from a real run.
    #[test]
    fn a_device_is_read_out_of_the_listing() {
        let device = super::accelerator_from(
            "  Vulkan0: NVIDIA GeForce RTX 5070 Ti (16211 MiB, 15418 MiB free)",
        )
        .expect("a device");
        assert_eq!(device.id, "Vulkan0");
        assert_eq!(device.name, "NVIDIA GeForce RTX 5070 Ti");
        assert_eq!(device.total_mb, Some(16211));
        assert_eq!(device.free_mb, Some(15418));
    }

    /// Carriage returns do not become part of the identifier, which the runtime would refuse.
    #[test]
    fn a_carriage_return_does_not_become_part_of_the_device() {
        let device =
            super::accelerator_from("  Vulkan0: A Device (1 MiB, 1 MiB free)\r").expect("a device");
        assert_eq!(device.id, "Vulkan0");
        assert!(!device.name.contains('\r'), "{:?}", device.name);
    }

    /// The listing heading is not mistaken for a device.
    #[test]
    fn the_listing_heading_is_not_a_device() {
        assert_eq!(super::accelerator_from("Available devices:"), None);
        assert_eq!(super::accelerator_from(""), None);
        assert_eq!(super::accelerator_from("no devices found"), None);
    }

    /// A device that reports no memory figures is still a device.
    #[test]
    fn a_device_without_memory_figures_still_counts() {
        let device = super::accelerator_from("  Vulkan0: Some Device").expect("a device");
        assert_eq!(device.id, "Vulkan0");
        assert_eq!(device.total_mb, None);
    }

    /// An archive is flattened, since the publisher's layout is not this crate's contract.
    #[test]
    fn unpacking_flattens_the_archive() {
        let dir = tempfile::tempdir().expect("temp dir");
        let archive = dir.path().join("runtime.zip");
        write_zip(
            &archive,
            &[
                ("build/bin/llama-server.exe", b"x"),
                ("build/bin/ggml.dll", b"y"),
            ],
        );

        let into = dir.path().join("out");
        std::fs::create_dir_all(&into).expect("dir");
        super::unpack(&archive, &into).expect("unpacks");

        assert!(into.join("llama-server.exe").exists());
        assert!(into.join("ggml.dll").exists());
        assert!(!into.join("build").exists(), "the prefix survived");
    }

    /// An entry that would write outside the directory is refused, not repaired.
    #[test]
    fn an_escaping_entry_is_refused() {
        let dir = tempfile::tempdir().expect("temp dir");
        let archive = dir.path().join("evil.zip");
        write_zip(&archive, &[("../../escaped.dll", b"x")]);

        let into = dir.path().join("out");
        std::fs::create_dir_all(&into).expect("dir");
        let error = super::unpack(&archive, &into).expect_err("refused");
        assert!(error.to_string().contains("outside"), "{error}");
        assert!(!dir.path().join("escaped.dll").exists());
    }

    /// An archive this build cannot unpack says so rather than half-succeeding.
    #[test]
    fn an_unknown_archive_kind_is_refused() {
        let dir = tempfile::tempdir().expect("temp dir");
        let archive = dir.path().join("runtime.tar.gz");
        std::fs::write(&archive, b"not really").expect("write");
        assert!(super::unpack(&archive, dir.path()).is_err());
    }

    /// Writes a zip with the given entries, for the two tests above.
    fn write_zip(at: &std::path::Path, entries: &[(&str, &[u8])]) {
        use std::io::Write as _;
        let file = std::fs::File::create(at).expect("create");
        let mut zip = zip::ZipWriter::new(file);
        let options: zip::write::FileOptions<'_, ()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, bytes) in entries {
            zip.start_file(*name, options).expect("entry");
            zip.write_all(bytes).expect("write");
        }
        zip.finish().expect("finish");
    }

    /// A free port is a real one, not the zero that was asked for.
    #[test]
    fn a_free_port_is_a_real_one() {
        assert_ne!(free_port().expect("a port"), 0);
    }

    /// The runtime end to end, against real hardware. Opt-in: it downloads a runtime.
    ///
    /// ```text
    /// cargo test -p orbistoun-llm --release -- --ignored gpu_runtime
    /// ```
    #[test]
    #[ignore = "downloads an inference runtime and starts it; opt-in via --ignored"]
    fn gpu_runtime_starts_and_answers() {
        use crate::catalog::Catalog;
        use crate::engine::{Engine, Request};

        let dir = tempfile::tempdir().expect("temp dir");
        let catalog = Catalog::default();
        let model = catalog.offline("qwen3-0.6b").expect("present");
        let runtime = Runtime::start(model, dir.path(), &dir.path().join("models"))
            .expect("the runtime downloads, starts and answers");

        let integration = crate::Integration {
            id: "managed".to_owned(),
            name: "managed".to_owned(),
            kind: crate::Kind::Online,
            source: "managed".to_owned(),
            model: Some(model.id.clone()),
            endpoint: Some(runtime.endpoint()),
            api_key: None,
            device: crate::Device::Gpu,
        };
        let engine =
            crate::online::OnlineEngine::new(&integration, &catalog).expect("an engine builds");
        let reply = engine
            .complete(&Request::new("Reply with one word: ready.").with_max_tokens(32))
            .expect("the runtime answers");
        assert!(!reply.trim().is_empty(), "empty reply");

        // Falling back to the processor is silent and successful, so the accelerator is asserted, not
        // just the reply.
        let device = runtime
            .accelerator()
            .expect("no accelerator was listed - this ran on the processor");
        eprintln!(
            "RUNTIME device={} ({}) total={:?} MB free={:?} MB",
            device.id, device.name, device.total_mb, device.free_mb
        );
        assert!(device.id.starts_with("Vulkan"), "{device:?}");
    }
}
