//! A GPU inference runtime, downloaded and supervised like any other input.
//!
//! # Why this exists at all
//!
//! The requirement is narrow and it rules out everything else: inference must run on the
//! **GPU**, with **no action from the user**, in a **portable** install.
//!
//! - Compiling an accelerator backend into this binary fails the second requirement.
//!   It is a build-time dependency on a vendor toolkit, so a machine without one cannot
//!   produce a build that uses its own hardware - and a machine *with* one produces a
//!   binary that will not load anywhere else.
//! - Asking the user to install a model server fails it outright.
//! - Running on the processor fails the first. Measured, on sixteen cores: about one
//!   token per second, four minutes for a reply nobody would wait for.
//!
//! What is left is to **fetch a runtime the same way a model is fetched**, which is
//! exactly what "download whatever it needs" allows. So this module downloads a
//! prebuilt `llama-server`, starts it, and talks to it over the OpenAI-shaped wire that
//! [`crate::online`] already speaks.
//!
//! # Vulkan, not CUDA
//!
//! | backend | download | vendors |
//! |---|---|---|
//! | Vulkan | **34 MB** | NVIDIA, AMD, Intel |
//! | CUDA | 250 MB, plus a 391 MB redistributable | NVIDIA only, matched per toolkit version |
//!
//! Size is the smaller argument. **orbistoun translates guest command streams to
//! Vulkan**, so a machine that can run this project at all has a working Vulkan driver
//! by definition. It is the one accelerator interface this project may assume, and
//! assuming it costs nothing that is not already assumed.
//!
//! The archive also carries processor backends selected at load time, so a machine with
//! no usable Vulkan device still runs - slower, and without a second download.
//!
//! # What this does not solve
//!
//! A supervised child process can be orphaned if this one dies without unwinding.
//! [`Runtime`] kills it on drop, which covers every ordinary exit and no hard crash.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::Error;
use crate::catalog::Offline;

/// The release this project pins.
///
/// A tag, not "latest". The same argument as the wire version in [`crate::online`]: a
/// client that follows whatever is newest has behaviour that changes without a commit,
/// and here it would also change without a download anybody asked for. Bumping this is
/// a deliberate act with a diff attached.
pub const LLAMA_TAG: &str = "b10612";

/// Where releases come from.
const RELEASES: &str = "https://github.com/ggml-org/llama.cpp/releases/download";

/// How long to wait for the server to answer after it is started.
///
/// Generous, because the first start loads several gigabytes of weights onto a device.
pub const READY_TIMEOUT: Duration = Duration::from_secs(180);

/// The archive for this platform, and the server binary inside it.
///
/// `None` where no prebuilt Vulkan build is published, which is not a failure - it means
/// this machine falls back to the in-process engine, and the caller says so.
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

/// An accelerator the runtime can actually address.
///
/// Reported by the runtime rather than probed for, which is the point: it answers "can
/// *this* inference backend use *that* device", where a vendor tool answers only "is a
/// device present". Those differ, and only the first one decides anything.
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

/// How many lines of the runtime's own output to keep.
///
/// Startup diagnostics only. Enough to hold the device enumeration and the layer
/// placement, which are the two things worth being able to prove afterwards.
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
            // A published archive whose layout changed is a wrong assumption about
            // somebody else's release, which is exactly the class of thing that cannot
            // be checked by reading. Say what was expected and where.
            Err(Error::Download(format!(
                "{archive} unpacked without a {binary} in {}",
                dir.display()
            )))
        }
    }

    /// Every accelerator this runtime can address, by asking it.
    ///
    /// Cheap - no model is loaded - and vendor-neutral, so an AMD or Intel device
    /// reports itself as readily as an NVIDIA one. A real gain over the `nvidia-smi`
    /// probe in [`crate::host`], which can only ever see one vendor.
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
        // The listing goes to stdout and the runtime's logging to stderr. Reading the
        // wrong one returns nothing, which looks exactly like a machine with no device.
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
            // Everything on the device. llama.cpp places what it can and leaves the
            // rest on the processor, so this is a ceiling rather than a demand - a
            // machine with no usable device still runs, without a second download.
            .args(["--n-gpu-layers", "999"])
            // One request at a time, matching how this crate asks.
            .args(["--parallel", "1"])
            // **No thinking.** A reasoning model puts its working somewhere other than
            // `message.content`, so a short reply comes back *empty* rather than short -
            // which is what the first live start of this runtime actually did. This is
            // the same decision the in-process engine makes by appending `/no_think`,
            // taken here in the one place that covers every reasoning model rather than
            // one family's chat template.
            .args(["--reasoning", "off"]);

        // **Named, not inferred.** Asking for a device by name makes an unusable one a
        // refusal to start rather than a silent fall back to the processor - which is
        // what turns "it started" into evidence that it is accelerated. Reading the same
        // fact out of the log needs a verbosity that prints a line per layer, and that
        // output is a debug log rather than an interface.
        if let Some(device) = &accelerator {
            command.args(["--device", &device.id]);
        }

        let child = command
            // **Both streams, piped and drained.** They are not interchangeable: the
            // runtime's ordinary logging goes to stderr, but the *device enumeration*
            // goes to stdout. Discarding stdout - which the first version did - throws
            // away the only evidence that an accelerator was used at all, and since
            // falling back to the processor is silent and successful, the result is a
            // run that cannot be told from a GPU one.
            //
            // A piped stream nobody reads fills its buffer and stops the child, so the
            // drains below are load-bearing rather than a convenience.
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
    /// **`None` is the answer that matters.** Falling back to the processor is silent
    /// and successful, so "it answered" is not evidence of anything - a test asserting
    /// only that a reply arrived passes identically either way.
    ///
    /// Trustworthy because the device was *named* on the command line: an unusable one
    /// stops the runtime starting, so a live [`Runtime`] holding `Some` is one whose
    /// device the runtime itself accepted.
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
        // Covers every ordinary exit. A hard crash of this process orphans the child,
        // which the module documentation says rather than leaving it to be discovered.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Reads one of the child's streams into the shared log until it closes.
///
/// Both streams share one buffer. Interleaving is not a problem here because nothing
/// reconstructs an ordering from it - it is searched for two specific facts.
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
/// That listing is a user-facing interface rather than a debug log, which is why it is
/// the thing parsed. The device selection *is* reported in the log too, but only at a
/// verbosity that also prints a line per layer of the model.
///
/// The memory figures are optional: a device reporting none is still a device, and
/// refusing it over a missing number would turn a formatting change into "no GPU here".
fn accelerator_from(line: &str) -> Option<Accelerator> {
    // Trimmed because the listing is written with carriage returns.
    let line = line.trim();
    let (id, rest) = line.split_once(": ")?;
    // Indented entries only. `Available devices:` is a heading that also contains a
    // colon, and reading it as a device reports one on a machine that listed none.
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
/// Asked for and released, so there is a window in which something else could take it.
/// The alternative is a fixed port, which collides with *any* other copy of this rather
/// than with an unlucky one.
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
/// **Flattened deliberately, and it is currently a no-op.** The pinned release ships a
/// flat archive - checked, not assumed - but releases have carried a `build/bin` prefix
/// before. Flattening means the layout of somebody else's archive is not part of this
/// crate's contract, and everything landing in one directory is what the backend loader
/// wants anyway: it finds `ggml-*` beside the executable.
///
/// Entries whose names try to escape the directory are refused rather than sanitised.
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
            // `enclosed_name` is `None` for anything that would escape - an absolute
            // path or one climbing out with `..`. Refused rather than repaired: an
            // archive doing that is not one to be clever about.
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
/// Owns the server rather than borrowing it, because a supervised process outliving
/// nothing in particular is a process nobody stops. Dropping this stops it.
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

    /// The pinned tag is a tag, not a moving target.
    ///
    /// A client that follows whatever is newest changes behaviour with no commit, and
    /// here it would also change with no download anybody asked for.
    #[test]
    fn the_runtime_release_is_pinned() {
        assert!(LLAMA_TAG.starts_with('b'), "{LLAMA_TAG}");
        assert!(!LLAMA_TAG.contains("latest"), "{LLAMA_TAG}");
    }

    /// The asset name carries the pinned tag and asks for the Vulkan build.
    ///
    /// Getting this wrong is a 404 minutes into a first run, which is exactly how the
    /// model catalogue's coordinates turned out to be wrong.
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

    /// A device is read out of the listing, verbatim from a real one.
    ///
    /// Pinned against output that was captured rather than recalled. The first version
    /// of this parser was written against a log format from memory - `ggml_vulkan: 0 =
    /// ...` - which this build does not emit at all, so it reported no accelerator on a
    /// machine that was demonstrably using one.
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

    /// Carriage returns do not become part of the identifier.
    ///
    /// The listing is written with CRLF endings, and an id of `Vulkan0\r` is one the
    /// runtime does not recognise - so the device would be named on the command line
    /// and then refused, turning a working machine into a failure to start.
    #[test]
    fn a_carriage_return_does_not_become_part_of_the_device() {
        let device =
            super::accelerator_from("  Vulkan0: A Device (1 MiB, 1 MiB free)\r").expect("a device");
        assert_eq!(device.id, "Vulkan0");
        assert!(!device.name.contains('\r'), "{:?}", device.name);
    }

    /// The heading is not mistaken for a device.
    ///
    /// `Available devices:` contains a colon like every entry does. Reading it as one
    /// reports an accelerator on a machine that listed none, which is the precise false
    /// positive that would send `--device` a name nothing can use.
    #[test]
    fn the_listing_heading_is_not_a_device() {
        assert_eq!(super::accelerator_from("Available devices:"), None);
        assert_eq!(super::accelerator_from(""), None);
        assert_eq!(super::accelerator_from("no devices found"), None);
    }

    /// A device that reports no memory is still a device.
    ///
    /// Refusing it over a missing figure would turn a change in someone else's output
    /// format into "this machine has no GPU", which is the worst way to be wrong here.
    #[test]
    fn a_device_without_memory_figures_still_counts() {
        let device = super::accelerator_from("  Vulkan0: Some Device").expect("a device");
        assert_eq!(device.id, "Vulkan0");
        assert_eq!(device.total_mb, None);
    }

    /// An archive is flattened, because the publisher's directory layout is not this
    /// crate's contract.
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
    ///
    /// The archive is somebody else's, fetched over the network. An entry climbing out
    /// of the directory is not a layout quirk to be tidied up - it is an archive to stop
    /// unpacking.
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

    /// A port comes back, and it is not zero.
    ///
    /// Zero is what the operating system was *asked* for, and returning it would mean
    /// reading the request back rather than the answer - the server would then bind an
    /// arbitrary port and nothing would know which.
    #[test]
    fn a_free_port_is_a_real_one() {
        assert_ne!(free_port().expect("a port"), 0);
    }

    /// The whole thing, against real hardware. Opt-in: it downloads a runtime.
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

        // **The assertion the requirement actually needs.** Falling back to the
        // processor is silent and successful, so "it answered" passes identically
        // either way and proves nothing about acceleration.
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
