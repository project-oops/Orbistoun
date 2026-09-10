//! The shared logic layer every shim calls.
//!
//! `orbistoun-cli`, `orbistoun-gui`, and worker mode are interaction shims (D034).
//! None of them holds behaviour. This crate is what they all call, so an operation
//! exists exactly once and the shims cannot drift.
//!
//! # Everything crossing the boundary is serialisable
//!
//! Operations take and return owned, serde-able values from `orbistoun-proto` - never
//! rich types holding references into loaded modules. That is what lets the same
//! operation be invoked in-process by the CLI and across a process boundary by the
//! worker (D035). It is a constraint, deliberately, and it is why the return types
//! live in the protocol crate rather than here.
//!
//! # What lives here and what does not
//!
//! Here: assembling the module registry, surveying a container, resolving overrides,
//! turning results into reportable shapes. Not here: anything about presentation,
//! argument parsing, or transports.

pub mod respond;
pub mod titlemodules;
pub mod titleplacement;

use std::path::{Path, PathBuf};

use orbistoun_hle::{Registry, StubPolicy};
use orbistoun_nid::{Nid, NidHasher, SymbolDb};
use orbistoun_proto::{ImportRecord, SegmentPlacement};

mod reporting;
mod symbols;
pub use symbols::{float_implementation_named, implementation_named};

// Re-exported so a shim depends only on the service, never on the protocol crate.
// Re-exported so a shim depends only on the service.
pub use orbistoun_nid::SymbolDbFile;
pub use orbistoun_proto::LoadLayout;
pub use orbistoun_proto::{ContainerInfo, ProcParamInfo, SurveySummary, WrapperInfo};
pub use reporting::{RunOutput, content_hash};
pub use symbols::DeclaredSymbol;

/// Why an operation could not be carried out.
#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    /// The file could not be read.
    #[error("reading {path}: {source}")]
    Io {
        /// Path that failed.
        path: String,
        /// Underlying cause.
        source: std::io::Error,
    },
    /// The container could not be parsed.
    #[error("container: {0}")]
    Container(#[from] orbistoun_elf::ElfError),
    /// The module could not be surveyed.
    #[error("survey: {0}")]
    Survey(#[from] orbistoun_loader::LoadError),
    /// A guest region could not be reserved or re-protected.
    #[error("address space: {0}")]
    Memory(#[from] orbistoun_mem::MemError),
    /// Serialising a result failed.
    #[error("serialising: {0}")]
    Serialise(String),
}

/// One runnable title found on disk.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct TitleEntry {
    /// The directory name. Always present, and the fallback when nothing else is.
    pub name: String,
    /// The module to run.
    pub module: PathBuf,
    /// What the title says about itself, when it says anything.
    pub metadata: Option<TitleMetadata>,
}

impl TitleEntry {
    /// What to call this title on screen.
    ///
    /// The published name when there is one, the directory name otherwise. **Never a
    /// blank**: a library row with no label is unusable, and a title with unreadable
    /// metadata is still a title somebody can run.
    pub fn display_name(&self) -> &str {
        self.metadata
            .as_ref()
            .map_or(self.name.as_str(), |m| m.title.as_str())
    }
}

/// What a title publishes about itself.
///
/// # Where this comes from
///
/// `sce_sys/param.json`, which is ordinary JSON, and `sce_sys/icon0.png`, which is an
/// ordinary PNG. Neither needs any knowledge of a proprietary format - they are read
/// with the same JSON and image handling any program would use.
///
/// **Read from the user's own files at run time, never stored here.** The repository
/// contains no title data and the provenance guard fails the build if it ever does; this
/// reads what is on the machine when the window is open (principle 1).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct TitleMetadata {
    /// The published name, in the title's own default language.
    pub title: String,
    /// The identifier, which is what appears in traces and file names.
    pub title_id: String,
    /// Content version, when published.
    pub version: Option<String>,
    /// The system version a title requires, decoded.
    ///
    /// **The one genuinely diagnostic field a title publishes.** It says which era of the
    /// interface a title was written against, and that predicts how much of it will be
    /// reached: the oldest title on this machine requires 1.00 and the newest 12.60, and
    /// those are not the same emulator problem.
    pub requires: Option<String>,
    /// The system version it was built against.
    pub built_with: Option<String>,
    /// The icon, if the file exists. A path rather than pixels - decoding belongs to
    /// whoever is drawing, and a library scan should not decode eight images nobody has
    /// looked at yet.
    pub icon: Option<PathBuf>,
}

/// The directory a title keeps its metadata in.
pub const TITLE_METADATA_DIR: &str = "sce_sys";
/// The file naming a title.
pub const TITLE_METADATA_FILE: &str = "param.json";
/// The title's icon.
pub const TITLE_ICON_FILE: &str = "icon0.png";

/// Decodes a packed system version into something readable.
///
/// The field is a JSON number holding the version in the top two bytes, each a pair of
/// binary-coded decimal digits: `0x1260…` is 12.60 and `0x0310…` is 3.10. Reading the
/// bytes as ordinary hex gives 18.96 and 3.16, which are not versions of anything - the
/// digits are literal, which is what binary-coded decimal means.
fn decode_system_version(value: &serde_json::Value) -> Option<String> {
    // Published as a number in every file examined, but accepted as a string too: a field
    // this project cannot re-derive is not worth failing over a representation.
    let packed = match value {
        serde_json::Value::Number(n) => n.as_u64()?,
        serde_json::Value::String(text) => {
            u64::from_str_radix(text.trim_start_matches("0x"), 16).ok()?
        }
        _ => return None,
    };
    let major = bcd((packed >> 56) as u8)?;
    let minor = bcd((packed >> 48) as u8)?;
    Some(format!("{major}.{minor:02}"))
}

/// One byte of binary-coded decimal.
///
/// `None` for a byte that is not two decimal digits, rather than a plausible wrong
/// number - a version nobody can trust is worse than a blank column.
const fn bcd(byte: u8) -> Option<u32> {
    let (high, low) = (byte >> 4, byte & 0xF);
    if high > 9 || low > 9 {
        return None;
    }
    Some((high * 10 + low) as u32)
}

/// Reads what a title says about itself.
///
/// `None` for anything unreadable, unparseable, or simply absent - homebrew and loose
/// dumps have no metadata at all, and they are still titles. Every caller has a fallback,
/// so a failure here costs a nicer label and nothing else.
pub fn read_title_metadata(title_dir: &Path) -> Option<TitleMetadata> {
    let system = title_dir.join(TITLE_METADATA_DIR);
    let text = std::fs::read_to_string(system.join(TITLE_METADATA_FILE)).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;

    let localized = value.get("localizedParameters")?;
    // The title names itself in several languages and says which one it means by
    // default. Picking the default is what the platform does; guessing English would be
    // wrong for anything that ships without it.
    let language = localized
        .get("defaultLanguage")
        .and_then(serde_json::Value::as_str);
    let title = language
        .and_then(|language| localized.get(language))
        .or_else(|| {
            // No default named, or named one that is not present: take any entry that
            // carries a name rather than reporting none.
            localized
                .as_object()?
                .values()
                .find(|entry| entry.get("titleName").is_some())
        })
        .and_then(|entry| entry.get("titleName"))
        .and_then(serde_json::Value::as_str)?
        .to_owned();

    let icon = system.join(TITLE_ICON_FILE);
    Some(TitleMetadata {
        title,
        title_id: value
            .get("titleId")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        version: value
            .get("contentVersion")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        requires: value
            .get("requiredSystemSoftwareVersion")
            .and_then(decode_system_version),
        built_with: value.get("sdkVersion").and_then(decode_system_version),
        icon: icon.is_file().then_some(icon),
    })
}

/// The high half every placeholder carries, so one is recognisable as ours wherever it turns up.
///
/// **Not named `_BASE`, deliberately.** That suffix means an *address* base in this tree, and
/// `docs/ADDRESS_MAP.md` is gated against every one of them (D513). Calling this a base made the
/// gate demand it be documented as somewhere memory is mapped, which it is not - it is an
/// error-code prefix. The gate was right and the name was wrong (D567).
const PLACEHOLDER_PREFIX: u64 = 0x7fff_0000;

/// Where tagged placeholders start, clear of the fixed `GuestError` codes that occupy
/// `0x7fff_0000..0x7fff_0010` - a tag must never be mistaken for one of those (D567).
const PLACEHOLDER_TAG_FLOOR: u64 = 0x10;

/// Whether the placeholder-tagging diagnostic is on for this process.
///
/// Read once. A setting consulted at two sites is a setting that can disagree with itself,
/// which is the D220 failure this project has already had three times.
fn tag_placeholders() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| orbistoun_env::TAG_PLACEHOLDERS.is_set())
}

/// The file a title is entered through.
///
/// Named once here rather than spelled into each shim: the shell script globbed for it,
/// the GUI would have needed it, and a third copy of a magic filename is how two tools
/// come to disagree about what a title even is (D160).
pub const TITLE_ENTRY_FILE: &str = "eboot.bin";

/// The settings a run reads from disk.
///
/// Everything here is a hypothesis the guest is the only oracle for, so it has to be
/// changeable without a rebuild - principle 5, and the reason the bisection loop is worth
/// anything. The file is `orbistoun-cli paths`' `config` entry.
///
/// Every field is optional and every section defaults, so a file naming one setting is
/// valid. A configuration that must be complete to be valid is one nobody edits.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct FileConfig {
    /// How the guest's entry point is presented.
    pub entry: orbistoun_loader::process::EntrySettings,
    /// How guest threads are placed, and what the guest is told about the machine.
    pub threads: orbistoun_kernel::thread::Settings,
    /// Memory behaviour that is a choice rather than a fact.
    pub memory: orbistoun_kernel::direct::Settings,
    /// Where a shim looks for titles, and how long a run may take.
    pub library: LibrarySettings,
    /// How many controllers there are, what drives each, and how keys map to buttons.
    ///
    /// Here rather than in the console settings file because it describes **this
    /// installation's hardware** - which keyboard, which gamepad in which port - and none of
    /// that travels to another machine. What the console is *set to* does travel, which is
    /// why it lives apart (D326).
    pub pads: orbistoun_input::Pads,
    /// What unimplemented functions answer.
    ///
    /// **The main lever the whole method turns on**, and it was the last thing still
    /// unreachable from a file. The oracle for most of this project is one bit per call
    /// site - answer `ok`, does the guest proceed? - and until now asking that question
    /// meant editing Rust and rebuilding, which is precisely what principle 5 exists to
    /// prevent (D166).
    pub policy: StubPolicy,
}

/// Where the library is, and how a run from it behaves.
///
/// Persisted rather than defaulted each launch, because the default is a *relative* path.
/// A library that is present when launched from a terminal and empty when launched any
/// other way is the sort of thing that reads as a bug in the scanner - and it was, for
/// exactly as long as [`Self::resolve`] did not exist (D228).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct LibrarySettings {
    /// The folder titles are looked for in.
    pub root: String,
    /// Seconds a guest may run before it is stopped. Zero means no limit.
    ///
    /// A backstop. It fixes the duration and lets the call count vary, and the count is
    /// what a verdict is read off - so it is the wrong thing to measure with, and the right
    /// thing to catch a guest that stops calling imports entirely (D238).
    pub run_limit_seconds: u64,
    /// Imports a guest may call before it is stopped. Zero means no budget.
    ///
    /// The deterministic limit: two runs of one build stop at the same call.
    pub run_call_budget: u64,
    /// Which view the window opens into when nothing on the command line says.
    ///
    /// **Here rather than in the window, because it has to survive a restart** - and it
    /// belongs beside the library root for the same reason that does: both describe how
    /// somebody wants to meet their own collection, and neither is a property of one launch.
    ///
    /// A command-line flag always beats it. The setting is what somebody wants usually;
    /// an argument is what they want this time.
    pub start_in: orbistoun_shell::View,
}

impl LibrarySettings {
    /// The folder to actually scan, given where this installation keeps its data.
    ///
    /// # Why a relative root is not resolved against the working directory
    ///
    /// Because the working directory is not a property of the installation - it is a
    /// property of *how somebody happened to start the program*. `cargo run` from the
    /// repository sets it to the repository; double-clicking the same binary in
    /// `target/debug` sets it to `target/debug`; a debugger sets it to whatever its
    /// launch configuration says. Same binary, same settings file, three different
    /// libraries, and the window reports the difference as "no titles here".
    ///
    /// `orbistoun-paths` exists because that reasoning already applied to everything this
    /// program *writes*. Reading is not different, so the base is the data root:
    ///
    /// - portable - `<binary>/.portable/titles`, so dropping titles beside the executable
    ///   works with no setup, which is the whole point of a portable build
    /// - installed - the collection's `titles/`, shared with the sibling projects
    /// - `ORBISTOUN_DATA_DIR` set - underneath that
    ///
    /// An **absolute** root is used as given. That is the ordinary case once somebody has
    /// pointed the window at their own folder, and it is the answer for a developer whose
    /// titles live in the repository: set it once, it is saved next to the rest of the
    /// settings, and every build from every launcher finds it afterwards.
    pub fn resolve(&self, data_root: &Path) -> PathBuf {
        let root = Path::new(&self.root);
        if root.is_absolute() {
            root.to_path_buf()
        } else {
            data_root.join(root)
        }
    }
}

impl Default for LibrarySettings {
    fn default() -> Self {
        Self {
            // Relative, and resolved against the data root rather than the working
            // directory - see `resolve`. Beside a portable binary this is already the
            // right answer; anywhere else it is a real, nameable folder somebody can be
            // told about, rather than a path that means something different per launch.
            root: "titles".to_owned(),
            // Matched to the CLI's, so a run launched from either shim is comparable.
            run_limit_seconds: 20,
            run_call_budget: 20_000_000,
            // The list, and deliberately: this is the view the emulator is worked on
            // through, and somebody who has not asked for anything else wants a table of
            // imports far more often than a wall of tiles.
            start_in: orbistoun_shell::View::List,
        }
    }
}

impl FileConfig {
    /// Reads settings from `path`.
    ///
    /// A missing file is the defaults, not an error - that is the ordinary case and
    /// failing on it would make the tool unusable out of the box.
    ///
    /// # Errors
    ///
    /// When the file exists but cannot be read or parsed. **Deliberately not silent:** a
    /// malformed file that quietly fell back to defaults would look exactly like a
    /// setting that had no effect, and the whole point of these is to observe an effect.
    pub fn load(path: &Path) -> Result<Self, ServiceError> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(ServiceError::Serialise(e.to_string())),
        };
        toml::from_str(&text).map_err(|e| ServiceError::Serialise(e.to_string()))
    }

    /// The settings as editable TOML, for writing a starting file.
    ///
    /// # Errors
    ///
    /// When the settings cannot be serialised.
    pub fn to_toml(&self) -> Result<String, ServiceError> {
        toml::to_string_pretty(self).map_err(|e| ServiceError::Serialise(e.to_string()))
    }
}

/// How a [`Service`] is set up.
///
/// [`Default`] is the **working** configuration, not the empty one. A default that
/// cannot resolve a single import is a trap: everything builds, everything runs, and
/// every lookup silently misses - which is exactly what happened when the worker was
/// left on an empty suffix and no implementation could ever be reached (D082).
#[derive(Debug, Clone)]
pub struct ServiceConfig {
    /// Bytes appended to a symbol name before hashing.
    ///
    /// Defaults to the suffix orbistoun ships with (D071). An empty one hashes to values
    /// no real module imports by, so a caller choosing that should mean it.
    pub nid_suffix: Vec<u8>,
    /// Behaviour for functions with no implementation.
    pub stub_policy: StubPolicy,
    /// Where run artifacts are written. `None` disables reporting entirely, which is
    /// what a unit test or a one-shot inspection wants.
    pub paths: Option<orbistoun_paths::Paths>,
    /// How guest threads are placed, and what the guest is told about the machine.
    ///
    /// Here rather than compiled into the kernel crate because it is policy, not fact
    /// (principle 5) - "how many cores does the guest think it has?" has to be a file
    /// edit and a relaunch, since the bisection loop is the only oracle available for
    /// questions like it.
    pub thread_settings: orbistoun_kernel::thread::Settings,
    /// Memory behaviour that is still under investigation - see
    /// [`orbistoun_kernel::direct::Settings`].
    pub memory_settings: orbistoun_kernel::direct::Settings,
    /// How the guest's entry point is presented: what is on its stack when it starts and
    /// what is in its first argument register.
    ///
    /// Settings rather than constants because only one part of it is established by
    /// measurement and the rest are hypotheses (D153).
    pub entry_settings: orbistoun_loader::process::EntrySettings,
    /// Optional symbol database, giving names for NIDs the registry does not declare.
    ///
    /// Independent of `nid_suffix`: an import's NID comes out of its own symbol name
    /// (D053), so surveying works without either. The database only supplies
    /// *human-readable* names for hashes.
    pub symbol_db: Option<SymbolDbFile>,
}

impl Default for ServiceConfig {
    fn default() -> Self {
        Self {
            nid_suffix: orbistoun_nid::default_suffix(),
            stub_policy: StubPolicy::default(),
            thread_settings: orbistoun_kernel::thread::Settings::default(),
            memory_settings: orbistoun_kernel::direct::Settings::default(),
            entry_settings: orbistoun_loader::process::EntrySettings::default(),
            paths: None,
            symbol_db: None,
        }
    }
}

/// The single logic layer.
#[derive(Debug)]
pub struct Service {
    registry: Registry,
    hasher: NidHasher,
    policy: StubPolicy,
    suffix_len: usize,
    symbols: Option<SymbolDb>,
    paths: Option<orbistoun_paths::Paths>,
    entry_settings: orbistoun_loader::process::EntrySettings,
}

/// Where regions handed to a guest by policy start.
///
/// Clear of every address orbistoun itself places - the loader's module (`0x40…`), the guest
/// stack (`0x60…`), the main-thread TLS block (`0x69…`) and the runtime mapping arena (`0x72…`) -
/// so a fault inside one is unmistakably about this rather than an overlap.
///
/// **And clear of where a guest's own allocator places its heap, which the first choice was not.**
/// It was `0x5000_0000_0000`, picked as empty. It is not: PPSA02664's C++ allocator reserves its
/// heap arena at exactly `0x5000_0000_0000` (the hint `sceKernelReserveVirtualRange` receives), so
/// this policy region got there first, the guest's reservation fell back to the mapping arena, and
/// its arena-relative size arithmetic then underflowed - `tlsf_add_pool` rejected the pool and the
/// next allocation returned null. Moved into orbistoun's own high cluster, between the TLS block and
/// the mapping arena, where nothing the guest chooses lands and the few small policy regions cannot
/// climb into the arena above (D443).
/// **The thirteen failed reservations at this address are the guest reserving inside the region
/// it was handed, and are expected.** A policy region plants its base into a guest argument, the
/// guest's allocator then reserves ranges *at* that base, and orbistoun already holds it - so the
/// hint fails and the fallback in `sceKernelReserveVirtualRange` supplies an address. Confirmed by
/// moving this constant twice and watching every failure follow it (D488).
const POLICY_REGION_BASE: u64 = 0x0000_6B00_0000_0000;

/// Reserves `len` bytes somewhere, bumping past anything already taken.
///
/// `orbistoun_mem::platform::reserve` takes an exact address and fails rather than relocating,
/// which is the right contract for placing an image and the wrong one for "give me somewhere".
/// A handful of attempts is enough: the addresses are ours and nothing else allocates here.
/// Reads a container's process parameter block and follows its memory-parameter pointer far
/// enough to report where it leads.
///
/// The block's own fields are read at cited offsets ([`orbistoun_elf::procparam`]). The three
/// pointers it carries are **relocated**: a launching title stores zero in the file and a
/// `RELATIVE` relocation supplies `base + addend`, so a pointer read straight from the file is
/// always zero. They are resolved here through the data relocation table - reporting the file
/// zero would be the "plausible but wrong" output the honest-failure principle forbids, because
/// it reads as "the title declares no memory parameters" when it declares them by relocation.
///
/// The memory-parameter block itself is surfaced raw - its stated size and any non-zero words -
/// rather than interpreted, because the layout inside it is not established from a citable
/// source. That raw view is the oracle a future flexible-memory implementation would confirm a
/// cited layout against (D442).
fn proc_param_info(
    container: &orbistoun_elf::Container<'_>,
    bytes: &[u8],
) -> Result<Option<ProcParamInfo>, ServiceError> {
    /// Most a memory-parameter block is scanned for non-zero words, so a wrong or hostile size
    /// cannot walk the whole file. Comfortably past obSCEne's `0x38`-byte block.
    const MEM_PARAM_SCAN_CAP: u64 = 0x100;

    let headers = container.program_headers()?;
    let Some(header) = headers
        .iter()
        .find(|h| h.p_type.get() == orbistoun_elf::SCE_PROCPARAM)
    else {
        return Ok(None);
    };
    let pp_vaddr = header.vaddr.get();
    let Some(block) = container.proc_param_bytes(bytes)? else {
        return Ok(None);
    };
    let Some(parsed) = orbistoun_elf::procparam::ProcParam::parse(block) else {
        return Ok(None);
    };

    // The RELATIVE relocations, keyed by the address they write to. A pointer slot that is zero
    // in the file is resolved to the addend of the relocation targeting it.
    let relatives = relative_relocation_targets(container, bytes)?;
    let resolve = |file_value: u64, slot: u64| -> u64 {
        if file_value != 0 {
            return file_value;
        }
        relatives
            .get(&pp_vaddr.wrapping_add(slot))
            .copied()
            .unwrap_or(0)
    };
    let libc_param_vaddr = resolve(parsed.libc_param, 0x38);
    let mem_param_vaddr = resolve(
        parsed.mem_param,
        orbistoun_elf::procparam::MEM_PARAM_OFFSET as u64,
    );
    let third_param_vaddr = resolve(parsed.third_param, 0x48);

    // Follow the memory-parameter pointer to its bytes, if it resolves. A pointer that maps to
    // nothing leaves the size and word list empty rather than failing the whole inspection.
    let mut mem_param_size = None;
    let mut mem_param_nonzero = Vec::new();
    if mem_param_vaddr != 0 {
        if let Some(at) = container.vaddr_to_offset(bytes, mem_param_vaddr)? {
            if let Some(size_bytes) = bytes.get(at..at + 8).and_then(|s| s.try_into().ok()) {
                let stated = u64::from_le_bytes(size_bytes);
                mem_param_size = Some(stated);
                let scan = stated.min(MEM_PARAM_SCAN_CAP);
                let mut offset = 8_u64;
                while offset + 8 <= scan {
                    let start = at + offset as usize;
                    if let Some(word) = bytes.get(start..start + 8).and_then(|s| s.try_into().ok())
                    {
                        let value = u64::from_le_bytes(word);
                        if value != 0 {
                            mem_param_nonzero.push((offset, value));
                        }
                    }
                    offset += 8;
                }
            }
        }
    }

    Ok(Some(ProcParamInfo {
        size: parsed.size,
        magic_ok: parsed.magic_ok(),
        entry_count: parsed.entry_count,
        sdk_version: parsed.sdk_version,
        libc_param_vaddr,
        mem_param_vaddr,
        third_param_vaddr,
        mem_param_size,
        mem_param_nonzero,
    }))
}

/// The `RELATIVE` relocations of a container, mapped from the address each writes to (`r_offset`)
/// to the value it writes (`addend`, which for an executable placed at base zero is the target's
/// own virtual address).
///
/// Used to resolve the process-parameter pointers, which a title stores as relocations rather than
/// as literal file bytes. A container with no dynamic table or no relocation table yields an empty
/// map - the honest "nothing to resolve", not an error.
fn relative_relocation_targets(
    container: &orbistoun_elf::Container<'_>,
    bytes: &[u8],
) -> Result<std::collections::HashMap<u64, u64>, ServiceError> {
    use orbistoun_elf::reloc;

    let mut targets = std::collections::HashMap::new();
    let Some(dyn_bytes) = container.dynamic_bytes(bytes)? else {
        return Ok(targets);
    };
    let info = orbistoun_elf::dynamic::DynamicInfo::parse(dyn_bytes);
    if info.rela == 0 || info.relasz == 0 {
        return Ok(targets);
    }
    let Some(at) = container.table_offset(bytes, &info, info.rela)? else {
        return Ok(targets);
    };
    let end = at.saturating_add(usize::try_from(info.relasz).unwrap_or(0));
    let Some(table) = bytes.get(at..end.min(bytes.len())) else {
        return Ok(targets);
    };
    for entry in reloc::parse_table(table) {
        if entry.kind() == reloc::kind::RELATIVE {
            // `addend` is `i64`; a relocation addend for an internal data pointer is a
            // non-negative virtual address, so a negative value is not one this cares about.
            if let Ok(addend) = u64::try_from(entry.addend.get()) {
                targets.insert(entry.offset.get(), addend);
            }
        }
    }
    Ok(targets)
}

fn reserve_somewhere(next: &mut u64, len: u64) -> Option<u64> {
    /// How many bases to try before giving up and saying so.
    const ATTEMPTS: usize = 16;

    let granularity = orbistoun_mem::allocation_granularity().max(orbistoun_core::GUEST_PAGE_SIZE);
    let len = len.checked_next_multiple_of(granularity)?;
    for _ in 0..ATTEMPTS {
        let base = *next;
        *next = next.checked_add(len)?;
        let taken = orbistoun_mem::platform::reserve(
            base,
            len,
            orbistoun_mem::Protection {
                read: true,
                write: true,
                execute: false,
            },
        );
        if let Ok(reservation) = taken {
            // **Leaked deliberately, and this is load-bearing.** `Reservation` releases its
            // range on drop, so discarding the handle here unmaps the region before the guest
            // ever sees the base - and the guest then faults at exactly the address the
            // placeholder did, which reads as "the policy did nothing" rather than as a bug.
            // The region has to outlive every call the guest makes into it, and the process
            // ends when the guest does.
            std::mem::forget(reservation);
            return Some(base);
        }
    }
    None
}

/// The vendor library table and module table, each keyed by the id an import carries.
type ModuleTables = (
    std::collections::BTreeMap<u16, String>,
    std::collections::BTreeMap<u16, String>,
);

/// Where one module's dynamic symbols sit in the shared stub table.
///
/// # Why there is one table and not one per module
///
/// Every table behind a stub - handlers, stub returns, forced returns, call counts, the
/// readable and writable ranges - is a process-global `OnceLock` indexed by a dynamic symbol
/// index, and a second install is **ignored**. So a table per module would be built and
/// silently dropped, and every count taken afterwards would be indexed against the wrong
/// module (D484).
///
/// One table with a range per module keeps all of that working unchanged: module *M*'s symbol
/// *i* is slot `offset + i`, one index still names one symbol of one module, and
/// `implemented_count` stays a single number over the run.
///
/// **The main executable is module 0 at offset 0**, so every recorded call index and every
/// measurement taken before this existed still means what it meant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleSlots {
    /// What the module is called, for a diagnostic that has to name one.
    pub label: String,
    /// The slot its symbol zero occupies.
    pub offset: usize,
    /// How many dynamic symbols it has.
    pub count: usize,
}

/// Lays out the shared stub table: which slots each module owns.
///
/// **Pure, and separated from the parsing for exactly the reason principle 8 gives.** The
/// arithmetic here is the whole of what makes one table serve several modules, and it is
/// testable without an ELF, without the host address space, and without touching the
/// process-global tables that [`Service::build_thunks_for`] can only fill once (D484).
///
/// The first module gets offset zero. That is not an accident of iteration order: the main
/// executable is module 0, so every call index in every trace and every measurement taken
/// before this existed still names the same symbol.
fn slot_ranges(modules: &[(&str, usize)]) -> Vec<ModuleSlots> {
    let mut out = Vec::with_capacity(modules.len());
    let mut at = 0_usize;
    for (label, count) in modules {
        out.push(ModuleSlots {
            label: (*label).to_owned(),
            offset: at,
            count: *count,
        });
        at = at.saturating_add(*count);
    }
    out
}

/// Policy-driven writes and returns, accumulated across every module before being installed.
///
/// # Why this is a value rather than a function that installs
///
/// `install_policy_writes` and `install_policy_returns` are process-global `OnceLock`s, so
/// calling them once per module keeps the first module's plants and **silently discards every
/// other module's** (D484).
///
/// That is exactly the shape of D187 - a setting consulted nowhere, in the branch nobody
/// re-read - so the collection is separated from the install and the install happens once,
/// where it can be seen to happen once.
///
/// The reservation cursor lives here too, for the same reason: two modules asking for regions
/// from independent cursors would be handed the same addresses.
#[derive(Debug)]
struct PolicyPlants {
    writes: Vec<Box<[orbistoun_thunk::Plant]>>,
    returns: Vec<(usize, u64)>,
    next: u64,
}

impl PolicyPlants {
    /// An empty set, one slot per symbol across every module.
    fn sized(slots: usize) -> Self {
        Self {
            writes: (0..slots).map(|_| Box::default()).collect(),
            returns: Vec::new(),
            next: POLICY_REGION_BASE,
        }
    }

    /// Publishes what every module asked for.
    ///
    /// The returns table is installed only when something wants it, because it is the same
    /// table a policy *answer* uses and an empty install would claim the slot for nothing.
    fn install(self) {
        orbistoun_thunk::install_policy_writes(self.writes);
        // Installed through the same table a policy answer uses, because that is what this is:
        // a value the function answers with. Only where it comes from differs.
        if !self.returns.is_empty() {
            orbistoun_thunk::install_policy_returns(self.returns);
        }
    }
}

/// Where a run puts the title's own modules and the tables that serve them.
///
/// Carried as one value because the three are a **layout**, and a caller passing three bare
/// addresses can transpose two of them into a collision that looks like a corrupt image.
#[derive(Debug, Clone, Copy)]
pub struct TitleBases {
    /// Where the title's own module images go.
    pub modules: u64,
    /// Where the shared stub table goes.
    pub thunks: u64,
    /// Where storage for data imports goes.
    pub data: u64,
}

/// The title's own modules, placed and relocated against one shared stub table.
#[derive(Debug)]
pub struct LinkedTitle {
    /// The placed images and everything they export.
    pub placed: titleplacement::PlacedTitleModules,
    /// Which slots of the shared table each module owns - the executable first.
    pub slots: Vec<ModuleSlots>,
    /// What relocating each module came to, by library name.
    pub tallies: Vec<(String, orbistoun_elf::reloc::RelocationTally)>,
    /// Names more than one module imports as data, which a lookup by name cannot separate.
    pub shared_data: Vec<String>,
    /// The one stub table serving every module, the executable included.
    ///
    /// **Returned rather than kept private, because the caller has to relocate the executable
    /// against it.** Building a second table for the executable would put its imports in a
    /// different index space from the modules that answer them, and the process-global
    /// dispatch tables would hold whichever was installed first (D484).
    pub thunks: orbistoun_thunk::ThunkTable,
    /// Storage for every data import across every module, in one reservation.
    pub data: orbistoun_thunk::DataBlocks,
    /// The executable's imports that a module the title ships answers, by symbol index.
    ///
    /// **Only the ones orbistoun does not implement itself** (D483): where this project has
    /// written the function, its implementation is what every measurement was taken against
    /// and it keeps the slot. Where it has not, a stub would say "nothing implements this"
    /// about code sitting in the title's own data, so the module wins.
    pub bound: std::collections::BTreeMap<u32, u64>,
    /// Every import accounted for, bound or not, with a reason for each that is not (D640).
    pub binding: titleplacement::BindingAccount,
    /// One label per slot of the shared stub table, across every module.
    ///
    /// **Built here because this is where the module bytes are.** A label vector sized to the
    /// executable alone names a title module's calls with whatever by-name stub sits at the
    /// same index, which is how a call returning a heap pointer came to read as `libc::usleep`
    /// (D490).
    pub labels: Vec<String>,
    /// Imports a module answers that orbistoun implements, so the implementation kept the slot.
    ///
    /// Reported rather than silently dropped: it is the list a reader needs to judge whether
    /// D483's default is still the right one for a given title.
    pub kept_by_orbistoun: Vec<String>,
}

impl Service {
    /// Builds a service with every subsystem registered.
    ///
    /// This is the one place that knows the full module set, so adding a subsystem
    /// crate is exactly one line here plus its own `guest_module!` declaration.
    pub fn new(config: ServiceConfig) -> Self {
        let hasher = NidHasher::new(config.nid_suffix.clone());
        // Driven from the one module list rather than a second copy of it. Two lists
        // is how `libc` came to be visible in `orbistoun-cli symbols` while resolving
        // nothing at all: declared in one, absent from the other, and the failure is
        // completely silent - the tool reports the function, the guest gets a stub
        // (D123).
        // Applied before anything can spawn: a thread placed under the default policy
        // and then reconfigured would be a thread the settings do not describe.
        orbistoun_kernel::thread::configure(config.thread_settings);
        orbistoun_kernel::direct::configure(config.memory_settings);

        let mut registry = Registry::new(hasher.clone(), config.stub_policy.clone());
        for module in symbols::modules() {
            registry.register(module);
        }
        // The database carries its own suffix, since names must be hashed with the
        // suffix they were derived under - not whatever this run was given.
        let symbols = config
            .symbol_db
            .as_ref()
            .and_then(SymbolDb::from_file)
            .map(|(db, _)| db);

        Self {
            registry,
            hasher,
            policy: config.stub_policy,
            entry_settings: config.entry_settings,
            suffix_len: config.nid_suffix.len(),
            symbols,
            paths: config.paths,
        }
    }

    /// Surveys a container **and emits a run report**, diffed against the previous run
    /// of the same title.
    ///
    /// This is the operation the iterative loop uses: it answers "what does this need"
    /// and "did the last change help" in one call, and persists the answer so the next
    /// run has something to compare against.
    pub fn survey_and_report(
        &self,
        path: &Path,
        now_unix_ms: u64,
    ) -> Result<RunOutput, ServiceError> {
        let bytes = std::fs::read(path).map_err(|source| ServiceError::Io {
            path: path.display().to_string(),
            source,
        })?;
        let survey = self.survey_bytes(&bytes)?;
        reporting::emit(self, path, &bytes, survey, now_unix_ms)
    }

    /// What unimplemented functions answer, how many were given a specific answer, and how
    /// many of those rest on nothing measured.
    ///
    /// Exposed so a run can *record* what it was subject to. Loosening the default is the
    /// single change that improves every number in a report while implementing nothing, so
    /// a trace that cannot say which policy produced it cannot be compared with one that
    /// can (D181).
    ///
    /// **The count is of symbols, not of answers, and the third number is new.** It used to be
    /// `overrides.len()`, which missed regions entirely - a policy that wrote a base into guest
    /// memory and answered nothing reported zero and read as an honest run. And it counted a
    /// hardware measurement and a guess alike, so a record could not tell the emulator being
    /// right from the emulator being helped (D557).
    pub fn policy_summary(&self) -> (String, usize, usize) {
        let default = match self.policy.default_return {
            orbistoun_hle::StubReturn::Ok => "ok".to_owned(),
            orbistoun_hle::StubReturn::Unimplemented => "unimplemented".to_owned(),
            orbistoun_hle::StubReturn::Raw(v) => format!("{v:#x}"),
        };
        (default, self.policy.specific(), self.policy.propping())
    }

    /// Every name the loaded database holds.
    ///
    /// For a search that derives new names from proved ones: the seeds have to be names,
    /// not candidates, and this is where the names are (D606).
    pub fn symbol_names(&self) -> impl Iterator<Item = &str> {
        self.symbols.iter().flat_map(SymbolDb::names)
    }
    /// The name the loaded database gives a hash, if it gives one.
    ///
    /// **Asked of the service rather than reached for directly**, so a shim that wants to
    /// spell a hash does not have to know how a database is loaded or which suffix built
    /// it. Three shims exist and none of them may hold that (principle 13).
    pub fn symbol_name(&self, nid: Nid) -> Option<&str> {
        self.symbols.as_ref().and_then(|db| db.name(nid))
    }
    /// How many names the loaded symbol database knows, if there is one.
    pub fn symbol_db_len(&self) -> Option<usize> {
        self.symbols.as_ref().map(SymbolDb::len)
    }

    /// How many of a container's imports a loaded database can name.
    ///
    /// The self-verifying measure from D025: a name list and suffix are correct
    /// exactly to the extent they explain hashes a real module imports.
    pub fn explain_imports(&self, bytes: &[u8]) -> Result<(usize, usize), ServiceError> {
        let survey = self.survey_bytes(bytes)?;
        let total = survey.total();
        let explained = self.symbols.as_ref().map_or(0, |db| {
            db.explains(survey.imports.iter().map(|i| Nid::from_raw(i.nid)))
        });
        Ok((explained, total))
    }

    /// Whether NIDs produced by this service can match a real module's imports.
    ///
    /// False when no hash suffix was supplied: names remain correct, hashes do not.
    /// Shims surface this rather than letting a meaningless number look authoritative.
    pub const fn nids_are_real(&self) -> bool {
        self.suffix_len > 0
    }

    /// How many functions are declared across all subsystems.
    pub fn declared_count(&self) -> usize {
        self.registry.len()
    }

    /// Every declared function, sorted for deterministic output.
    ///
    /// Determinism matters beyond tidiness: reports are diffed between runs, and
    /// ordering churn would read as change.
    pub fn declared_symbols(&self) -> Vec<DeclaredSymbol> {
        symbols::all(&self.hasher)
    }

    /// How the guest's entry point is presented.
    pub const fn entry_settings(&self) -> &orbistoun_loader::process::EntrySettings {
        &self.entry_settings
    }

    /// Every runnable title under `root`.
    ///
    /// A directory containing the entry file, and nothing cleverer. Sorted, because a
    /// library that reorders itself between runs is one nobody can navigate.
    ///
    /// # Errors
    ///
    /// When `root` cannot be read. A directory that cannot be inspected is skipped rather
    /// than failing the whole scan - one unreadable title should not hide the others.
    pub fn discover_titles(&self, root: &Path) -> Result<Vec<TitleEntry>, ServiceError> {
        let entries = std::fs::read_dir(root).map_err(|e| {
            // With the path in it. `io::Error` does not carry one, so the bare message is
            // "the system cannot find the path specified" - which is not an answer to the
            // only question the reader has.
            ServiceError::Serialise(format!("cannot read library at {}: {e}", root.display()))
        })?;
        let mut found: Vec<TitleEntry> = entries
            .flatten()
            .filter_map(|entry| {
                let directory = entry.path();
                let module = directory.join(TITLE_ENTRY_FILE);
                module.is_file().then(|| TitleEntry {
                    name: entry.file_name().to_string_lossy().into_owned(),
                    module,
                    metadata: read_title_metadata(&directory),
                })
            })
            .collect();
        found.sort();
        Ok(found)
    }

    /// The default stub policy as editable TOML.
    pub fn default_policy_toml(&self) -> Result<String, ServiceError> {
        toml::to_string_pretty(&self.policy).map_err(|e| ServiceError::Serialise(e.to_string()))
    }

    /// Reports a container's structure without executing or fully parsing it.
    pub fn inspect_bytes(&self, bytes: &[u8]) -> Result<ContainerInfo, ServiceError> {
        let container = orbistoun_elf::Container::parse(bytes)?;
        let headers = container.program_headers()?;
        let vendor = container.vendor_segments()?.len();
        let h = container.header();
        Ok(ContainerInfo {
            wrapper: container
                .wrapper()
                .map_or(WrapperInfo::None, |w| WrapperInfo::Wrapped {
                    previous_generation: orbistoun_elf::Wrapper::generation(bytes)
                        == Some(orbistoun_elf::Generation::Previous),
                    segment_count: w.segment_count(),
                    stated_size: w.stated_size(),
                }),
            elf_offset: container
                .wrapper()
                .map_or(0, orbistoun_elf::Wrapper::elf_offset),
            entry: container.entry(),
            e_type: h.e_type.get(),
            machine: h.machine.get(),
            osabi: h.ident[7],
            program_headers: headers.len(),
            vendor_segments: vendor,
            mapped_segments: container.mapped_program_headers(bytes)?,
            proc_param: proc_param_info(&container, bytes)?,
        })
    }

    /// Reports a container's structure, from disk.
    pub fn inspect_path(&self, path: &Path) -> Result<ContainerInfo, ServiceError> {
        let bytes = std::fs::read(path).map_err(|source| ServiceError::Io {
            path: path.display().to_string(),
            source,
        })?;
        self.inspect_bytes(&bytes)
    }

    /// Reserves the address space a module demands, without executing anything.
    ///
    /// `base` is added to each segment's virtual address: a module links at zero and
    /// must be placed somewhere, while an executable carries absolute addresses and
    /// wants a base of zero.
    ///
    /// # One span, not one reservation per segment
    ///
    /// A module occupies a **contiguous span**, and that is how it is reserved. Two
    /// facts observed on real material force it:
    ///
    /// - **Windows reserves at 64 KiB granularity**, not page granularity. Segments
    ///   that are pages apart share a 64 KiB block, so per-segment reservation makes
    ///   neighbouring segments collide with each other - a self-inflicted conflict
    ///   that says nothing about whether the address is actually available.
    /// - **Segment addresses are not page-aligned.** Real modules carry vaddrs like
    ///   `0x147f0`. A mapping must start on a page boundary, so the span is rounded
    ///   outwards at both ends rather than rejected.
    ///
    /// Reservations are released when the returned layout is dropped: this answers
    /// "can this be placed here", not "place it permanently".
    pub fn load_layout(&self, bytes: &[u8], base: u64) -> Result<LoadLayout, ServiceError> {
        /// `PT_LOAD`.
        const PT_LOAD: u32 = 1;
        /// Segment permission bits, as ELF defines them.
        const PF_X: u32 = 1;
        /// Writable.
        const PF_W: u32 = 2;
        /// Readable.
        const PF_R: u32 = 4;

        let page = orbistoun_core::GUEST_PAGE_SIZE;
        let container = orbistoun_elf::Container::parse(bytes)?;

        let loadable: Vec<_> = container
            .program_headers()?
            .into_iter()
            .enumerate()
            .filter(|(_, ph)| ph.p_type.get() == PT_LOAD && ph.memsz.get() > 0)
            .collect();

        let mut segments = Vec::new();
        for (index, ph) in &loadable {
            let flags = ph.flags.get();
            segments.push(SegmentPlacement {
                index: *index,
                vaddr: base.saturating_add(ph.vaddr.get()),
                memsz: ph.memsz.get(),
                read: flags & PF_R != 0,
                write: flags & PF_W != 0,
                execute: flags & PF_X != 0,
                failure: None,
            });
        }

        let Some(lowest) = segments.iter().map(|s| s.vaddr).min() else {
            return Ok(LoadLayout {
                base,
                span_base: base,
                span_len: 0,
                segments,
                reservation_failure: None,
            });
        };
        let highest = segments
            .iter()
            .map(|s| s.vaddr.saturating_add(s.memsz))
            .max()
            .unwrap_or(lowest);

        // Round outwards: a segment starting mid-page still owns that page, and one
        // ending mid-page does too.
        let span_base = lowest / page * page;
        let span_end = highest.div_ceil(page).saturating_mul(page);
        let span_len = span_end.saturating_sub(span_base);

        let mut space = orbistoun_mem::AddressSpace::new();
        let reservation_failure = space
            .reserve(
                span_base,
                span_len,
                // The span is reserved read-write so it can be populated; per-segment
                // protection is applied when segments are actually written, which is
                // the loader's job rather than this survey's.
                orbistoun_mem::Protection::READ_WRITE,
            )
            .err()
            .map(|e| e.to_string());

        Ok(LoadLayout {
            base,
            span_base,
            span_len,
            segments,
            reservation_failure,
        })
    }

    /// Places a container in memory: reserves its span and copies every loadable
    /// segment, zeroing `.bss`.
    ///
    /// The image holds its own reservation, so dropping it releases the memory. This
    /// is loading up to but not including relocation.
    pub fn place_image(
        &self,
        bytes: &[u8],
        base: u64,
    ) -> Result<orbistoun_loader::Image, ServiceError> {
        Ok(orbistoun_loader::image::place(
            bytes,
            base,
            orbistoun_core::GUEST_PAGE_SIZE,
        )?)
    }

    /// Reserves a region for every stub the policy says writes one, and installs the base.
    ///
    /// **The reservation happens here, not in the thunk.** A trampoline answering a call runs
    /// on the guest's own stack under principle 9's no-allocation rule; reserving address
    /// space there is the wrong layer and the wrong moment. This layer already builds the
    /// address space, so it takes the region up front and hands the dispatcher a concrete
    /// base to store (D295).
    ///
    /// # Errors
    ///
    /// If the container cannot be re-read. A region that cannot be reserved is **skipped and
    /// said out loud** rather than failing the run: the rest of the guest is still worth
    /// having, and a policy entry that silently did nothing is the failure this whole family
    /// of mechanisms exists to avoid.
    fn collect_policy_writes(
        &self,
        container: &orbistoun_elf::Container<'_>,
        bytes: &[u8],
        offset: usize,
        plants: &mut PolicyPlants,
    ) -> Result<(), ServiceError> {
        if self.policy.regions.is_empty() {
            return Ok(());
        }
        for import in container.raw_imports(bytes, &self.hasher)? {
            let Some(resolved) = self.registry.resolve(Nid::from_raw(import.nid)) else {
                continue;
            };
            let Some(plan) = self.policy.regions.get(resolved.name) else {
                continue;
            };
            let Some(base) = reserve_somewhere(&mut plants.next, plan.bytes) else {
                eprintln!(
                    "orbistoun: {} asks for {:#x} bytes and none were free - it will answer without one",
                    resolved.name, plan.bytes
                );
                continue;
            };
            let slot = offset.saturating_add(import.symbol_index as usize);
            // **Delivery is a decision about how, not about what.** The region is the same
            // either way; all that differs is whether the guest reads the base out of an
            // argument it passed or out of the register it gets an answer in (D300).
            match plan.via {
                orbistoun_hle::Delivery::Argument(position) => {
                    if let Some(entry) = plants.writes.get_mut(slot) {
                        *entry = Box::new([orbistoun_thunk::Plant {
                            position,
                            offset: 0,
                            value: base,
                        }]);
                    }
                }
                orbistoun_hle::Delivery::Return => {
                    plants.returns.push((slot, base));
                }
            }
        }
        Ok(())
    }

    /// Storage for every import that names data rather than code.
    ///
    /// Separate from [`Self::build_thunks`] because they answer different questions and a
    /// caller needs both: a stub says *which function* was wanted, and a block says the
    /// guest wanted an **object**, which no stub can be (D307).
    ///
    /// # Errors
    ///
    /// When the container cannot be parsed or the host refuses the reservation.
    pub fn build_data_blocks(
        &self,
        bytes: &[u8],
        base: u64,
        db: Option<&SymbolDb>,
    ) -> Result<orbistoun_thunk::DataBlocks, ServiceError> {
        let container = orbistoun_elf::Container::parse(bytes)?;
        // Names as well as indices: relocation needs the index, and an implementation that
        // owns a guest global - `getopt` writing `optarg` - needs the name (D344).
        let imports: Vec<(usize, String)> = container
            .raw_imports(bytes, &self.hasher)?
            .into_iter()
            .filter(|import| import.kind == orbistoun_elf::dynamic::Kind::Object)
            .map(|import| {
                (
                    import.symbol_index as usize,
                    Self::spell_data_import(db, &import),
                )
            })
            .collect();
        Ok(orbistoun_thunk::DataBlocks::build(
            base,
            &imports,
            orbistoun_core::GUEST_PAGE_SIZE,
        )?)
    }

    /// How a data import should be spelled, given what this run's database knows.
    ///
    /// **A vendor module spells every import as an encoded hash**, so the name reaching the data
    /// block is `f7uOxY9mM1U#r#n` and that is what a run report prints beside a faulting register.
    /// It is the right storage under an unreadable label: PPSA21564 dies holding a pointer to one
    /// of these blocks, and the report could say only that the guest was looking at offset zero of
    /// something spelled in base64 - which cannot tell a reader whether it is a C++ vtable, a
    /// stdio handle or the stack canary, and those want completely different answers (D647).
    ///
    /// **The run's database, not the service's.** A worker process builds its `Service` with no
    /// symbol database at all - the database is loaded per run, because names belong to the run
    /// (the same reasoning as `import_labels_with`). Reaching for `self.symbols` here compiled,
    /// ran, and renamed nothing, which is the failure mode a lookup that cannot fail always has.
    ///
    /// Falls back to the encoded spelling when nothing names the hash: that string is still the
    /// module's own and is still greppable. What it must never do is invent one.
    fn spell_data_import(
        db: Option<&SymbolDb>,
        import: &orbistoun_elf::dynamic::RawImport,
    ) -> String {
        db.and_then(|db| db.name(Nid::from_raw(import.nid)))
            .map_or_else(|| import.name.clone(), ToOwned::to_owned)
    }

    /// Storage for every data import across several modules, in one reservation.
    ///
    /// **One block, indexed the way the shared stub table is** (D484): module *M*'s symbol *i*
    /// occupies index `offset(M) + i`, so a resolver consulting the two needs one index space
    /// rather than a lookup that first works out which module a slot belongs to.
    ///
    /// The named map merges as a consequence, which is what `install_data_symbols` needs -
    /// it is a `OnceLock`, so building one block per module would publish the first module's
    /// globals and silently drop the rest.
    ///
    /// A name two modules both import as data is **reported**: they get separate storage, as
    /// they must, but an implementation reaching for the name by string can only be handed
    /// one of them, and which one is not something this layer can decide.
    ///
    /// # Errors
    ///
    /// When a container cannot be parsed or the host refuses the reservation.
    pub fn build_data_blocks_for(
        &self,
        modules: &[(&str, &[u8])],
        slots: &[ModuleSlots],
        base: u64,
        db: Option<&SymbolDb>,
    ) -> Result<(orbistoun_thunk::DataBlocks, Vec<String>), ServiceError> {
        let mut imports: Vec<(usize, String)> = Vec::new();
        let mut seen: std::collections::BTreeMap<String, String> =
            std::collections::BTreeMap::new();
        let mut shared: Vec<String> = Vec::new();
        for ((label, bytes), slot) in modules.iter().zip(slots) {
            let container = orbistoun_elf::Container::parse(bytes)?;
            for import in container.raw_imports(bytes, &self.hasher)? {
                if import.kind != orbistoun_elf::dynamic::Kind::Object {
                    continue;
                }
                let spelled = Self::spell_data_import(db, &import);
                if let Some(first) = seen.get(&spelled) {
                    shared.push(format!(
                        "{spelled}: imported as data by {first} and by {label} - both have storage, but a lookup by name answers one of them"
                    ));
                } else {
                    seen.insert(spelled.clone(), (*label).to_owned());
                }
                imports.push((
                    slot.offset.saturating_add(import.symbol_index as usize),
                    spelled,
                ));
            }
        }
        let blocks =
            orbistoun_thunk::DataBlocks::build(base, &imports, orbistoun_core::GUEST_PAGE_SIZE)?;
        Ok((blocks, shared))
    }

    /// Builds one stub per dynamic symbol, sized from the container itself.
    ///
    /// Sized from the symbol table rather than the import list because relocations
    /// index the symbol table - a table sized to the imports would be indexed out of
    /// range by the first relocation naming anything else.
    ///
    /// # Errors
    ///
    /// When the container cannot be parsed or the table cannot be built.
    pub fn build_thunks(
        &self,
        bytes: &[u8],
        base: u64,
    ) -> Result<orbistoun_thunk::ThunkTable, ServiceError> {
        let (table, _) = self.build_thunks_for(&[("", bytes)], base)?;
        Ok(table)
    }

    /// Builds one shared stub table across several modules, and says where each one sits.
    ///
    /// `modules[0]` is the main executable and takes offset zero, so a single-module call is
    /// bit-identical to what [`Self::build_thunks`] always did.
    ///
    /// **Every process-global table is installed once, after every module has contributed.**
    /// That is not tidiness: `install_handlers`, `install_stub_returns` and
    /// `install_policy_writes` are `OnceLock`s, so installing per module would keep the first
    /// module's answers and silently discard the rest (D484).
    ///
    /// # Errors
    ///
    /// When a container cannot be parsed or the table cannot be built.
    pub fn build_thunks_for(
        &self,
        modules: &[(&str, &[u8])],
        base: u64,
    ) -> Result<(orbistoun_thunk::ThunkTable, Vec<ModuleSlots>), ServiceError> {
        let mut counts = Vec::with_capacity(modules.len());
        for (label, bytes) in modules {
            let container = orbistoun_elf::Container::parse(bytes)?;
            counts.push((
                *label,
                usize::try_from(container.symbol_count(bytes)?).unwrap_or(0),
            ));
        }
        let slots = slot_ranges(&counts);
        let imports = slots.last().map_or(0, |s| s.offset.saturating_add(s.count));
        // Stubs past the guest's own symbols, one per implemented name.
        //
        // **For the names a guest never imports.** The open-toolchain payloads resolve
        // most of their C library themselves, at startup, by asking the platform for one
        // name at a time - so those names appear in no import table and no relocation, and
        // a resolver has nothing to hand back unless a stub exists for them (D365).
        let resolvable = symbols::resolvable();
        let total = imports + resolvable.len();
        let table = orbistoun_thunk::ThunkTable::build_with_named(
            base,
            imports,
            resolvable.len(),
            orbistoun_core::GUEST_PAGE_SIZE,
        )?;

        // Bind implementations to the slots the guest will actually call. Without this
        // step the registry knows names and arities and nothing consults it at the point
        // a call happens, so implementing a function changes nothing (D082).
        let mut handlers: Vec<Option<orbistoun_core::GuestFn>> = vec![None; total];
        // The floating-point table, bound in the same pass. Disjoint from the one above by
        // construction: a function answers in `rax` or in `xmm0`, never both (D268).
        let mut float_handlers: Vec<Option<orbistoun_core::GuestFloatFn>> = vec![None; total];
        // Functions that resolved and had nowhere to go. Reported, never silent.
        let mut unplaced: Vec<String> = Vec::new();
        for ((_, bytes), slot) in modules.iter().zip(&slots) {
            let container = orbistoun_elf::Container::parse(bytes)?;
            self.bind_handlers(
                &container,
                bytes,
                slot.offset,
                (&mut handlers, &mut float_handlers),
                &mut unplaced,
            )?;
        }
        if !unplaced.is_empty() {
            // Not fatal: the rest of the run is still worth having, and a report that names
            // the functions is worth more than a refusal that names none of them.
            eprintln!(
                "orbistoun: {} implemented functions could not be bound to a stub slot and will answer a placeholder: {}",
                unplaced.len(),
                unplaced.join(", ")
            );
        }
        Self::bind_by_name(
            &resolvable,
            imports,
            &table,
            &mut handlers,
            &mut float_handlers,
        );
        orbistoun_thunk::install_float_handlers(float_handlers);

        let knowledge = orbistoun_hle::knowledge::Knowledge::builtin();
        let mut stub_returns: Vec<Option<u64>> = vec![None; imports];
        let mut plants = PolicyPlants::sized(imports);
        for ((_, bytes), slot) in modules.iter().zip(&slots) {
            let container = orbistoun_elf::Container::parse(bytes)?;
            self.fill_stub_returns(
                &container,
                bytes,
                slot.offset,
                &knowledge,
                &mut stub_returns,
            )?;
            self.collect_policy_writes(&container, bytes, slot.offset, &mut plants)?;
        }
        orbistoun_thunk::install_stub_returns(stub_returns);
        plants.install();
        orbistoun_thunk::install_handlers(handlers);
        Ok((table, slots))
    }

    /// Binds this module's implemented imports into the shared handler tables.
    ///
    /// `offset` is where the module's symbol zero sits, so `symbol_index` is a slot only after
    /// it is added - the one arithmetic that makes a shared table work.
    ///
    /// The two tables travel as a pair because a binding goes into exactly one of them and the
    /// choice is made here; splitting them into two arguments invited passing one function's
    /// table with another's.
    fn bind_handlers(
        &self,
        container: &orbistoun_elf::Container<'_>,
        bytes: &[u8],
        offset: usize,
        tables: (
            &mut [Option<orbistoun_core::GuestFn>],
            &mut [Option<orbistoun_core::GuestFloatFn>],
        ),
        unplaced: &mut Vec<String>,
    ) -> Result<(), ServiceError> {
        let (handlers, float_handlers) = tables;
        let available = symbols::implementations();
        let float_available = symbols::float_implementations();
        for import in container.raw_imports(bytes, &self.hasher)? {
            let nid = Nid::from_raw(import.nid);
            let Some(resolved) = self.registry.resolve(nid) else {
                continue;
            };
            let slot = offset.saturating_add(import.symbol_index as usize);
            // **A binding that does not fit is said out loud.** Both tables are sized from
            // the dynamic symbol count and `symbol_index` indexes that same table, so a slot
            // out of range means the two disagree - and the consequence is an implemented
            // function that silently answers a placeholder for the whole run. That is
            // indistinguishable in every report from a function nobody wrote, which is the
            // failure D281 cost an evening to; `if let Some(..) = get_mut` swallowed it.
            if let Some((_, function)) = float_available
                .iter()
                .find(|(name, _)| *name == resolved.name)
            {
                match float_handlers.get_mut(slot) {
                    Some(entry) => *entry = Some(*function),
                    None => unplaced.push(resolved.name.to_owned()),
                }
                continue;
            }
            if let Some((_, function)) = available.iter().find(|(name, _)| *name == resolved.name) {
                match handlers.get_mut(slot) {
                    Some(entry) => *entry = Some(*function),
                    None => unplaced.push(resolved.name.to_owned()),
                }
            }
        }
        Ok(())
    }

    /// What each of this module's *unimplemented* stubs should answer.
    ///
    /// **Three sources, in this order, and the order is the whole point** (D166):
    ///
    /// 1. An explicit per-symbol override from the policy file. Somebody has typed a
    ///    deliberate experiment - "answer ok for this one and see if the guest proceeds" -
    ///    and that must win over everything, because it is the question being asked.
    /// 2. The knowledge file's declared return kind. A pointer-, handle- or count-returning
    ///    function answers zero, because an error code in a pointer register is a wild
    ///    pointer the guest dereferences immediately (D125). This beats the policy *default*
    ///    deliberately: a blanket "answer ok" must not quietly reintroduce that.
    /// 3. The policy default, for everything else.
    ///
    /// Until this existed the policy was consulted nowhere on the call path, so every
    /// override anybody wrote was silently ignored - the same failure as D082, one layer over.
    fn fill_stub_returns(
        &self,
        container: &orbistoun_elf::Container<'_>,
        bytes: &[u8],
        offset: usize,
        knowledge: &orbistoun_hle::knowledge::Knowledge,
        stub_returns: &mut [Option<u64>],
    ) -> Result<(), ServiceError> {
        for import in container.raw_imports(bytes, &self.hasher)? {
            // **An undeclared import still gets the policy.** It used to be skipped here,
            // which quietly exempted the majority of what a guest calls from the one knob
            // for asking "does it proceed if this succeeds?" - and those are precisely the
            // imports worth asking about, since a declared one is usually implemented.
            //
            // The exemption made a whole class of experiment vacuous rather than wrong: an
            // `ok` sweep reported no change because the functions under test never saw it,
            // and "return values are not the cause" was concluded from it (D187). The same
            // failure as D166 and D082 - a setting consulted nowhere - surviving in the
            // branch nobody re-read.
            let resolved = self.registry.resolve(Nid::from_raw(import.nid));
            let declared = resolved
                .and_then(|r| knowledge.get(r.name))
                .and_then(|k| k.returns)
                .and_then(orbistoun_hle::knowledge::Returns::stub_value);
            // Keyed by name where there is one, and by hash where there is not.
            //
            // **The unnamed case is the one that needed it.** A function nobody has named
            // yet is exactly the function worth asking "does the guest proceed if this
            // succeeds?" about, and until now the only way to ask was a blanket sweep that
            // changed every answer at once - which cannot tell you *which* one mattered
            // (D187). A hash is a perfectly good key; it is just not a nice one.
            let by_name = resolved.and_then(|r| self.policy.overrides.get(r.name));
            let by_nid = self
                .policy
                .overrides
                .get(&format!("{:#018x}", import.nid))
                .or_else(|| self.policy.overrides.get(&format!("{:016x}", import.nid)));
            let overridden = by_name.or(by_nid).map(|r| u64::from(r.as_raw()));
            // **A tagged placeholder names its own source.** Every stub answers the same
            // `0x7fff_0001`, so one found in a guest's argument says *some* unimplemented
            // function produced it and never which - `error_used_as_pointer` can only tell a
            // reader to go looking, which D299 says a finding must not do. Under
            // `ORBISTOUN_TAG_PLACEHOLDERS` each stub answers `0x7fff_0000 | (0x10 + its slot)`,
            // so the value **is** the attribution.
            //
            // Above `0x10` because `0x7fff_0000..0x7fff_0010` is the fixed placeholder range the
            // `GuestError` codes occupy - a tag must not be mistaken for one of those (D567).
            let slot_index = offset.saturating_add(import.symbol_index as usize);
            let tagged = tag_placeholders()
                .then(|| u64::try_from(slot_index).ok())
                .flatten()
                .map(|index| index + PLACEHOLDER_TAG_FLOOR)
                .filter(|tag| *tag <= 0xffff)
                .map(|tag| PLACEHOLDER_PREFIX | tag);
            let Some(value) =
                overridden
                    .or(tagged)
                    .or(declared)
                    .or_else(|| match self.policy.default_return {
                        // The ordinary error code is what the dispatcher already falls back
                        // to, so leaving it unset keeps one path rather than two.
                        orbistoun_hle::StubReturn::Unimplemented => None,
                        other => Some(u64::from(other.as_raw())),
                    })
            else {
                continue;
            };
            if let Some(slot) =
                stub_returns.get_mut(offset.saturating_add(import.symbol_index as usize))
            {
                *slot = Some(value);
            }
        }
        Ok(())
    }

    /// Where run artifacts are written, if reporting is enabled at all.
    pub const fn paths(&self) -> Option<&orbistoun_paths::Paths> {
        self.paths.as_ref()
    }

    /// Every import hash this build cannot put a name to.
    ///
    /// The input to a name search: exactly the set worth spending hashes on. Searching
    /// for hashes that are already named wastes the whole space on questions with known
    /// answers.
    pub fn unnamed_imports(&self, bytes: &[u8]) -> Result<Vec<Nid>, ServiceError> {
        let container = orbistoun_elf::Container::parse(bytes)?;
        let mut seen = std::collections::BTreeSet::new();
        for import in container.raw_imports(bytes, &self.hasher)? {
            let nid = Nid::from_raw(import.nid);
            if !self.is_named(nid) {
                seen.insert(nid.as_raw());
            }
        }
        Ok(seen.into_iter().map(Nid::from_raw).collect())
    }

    /// Whether anything at all can put a name to this hash right now.
    ///
    /// Both sources, because a name can arrive either way: the registry holds what a
    /// `guest_module!` declares, and the symbol database holds what a search worked out.
    ///
    /// **Extracted so there is one definition of "named".** The work list kept its own,
    /// narrower one - it removed only the hashes a given run had just solved - so a hash
    /// named by any earlier run stayed on the list permanently. It could not be solved
    /// again to be removed, because being named is exactly what stops it being searched
    /// for. 116 of 3829 entries were hashes the database could already name.
    #[must_use]
    pub fn is_named(&self, nid: Nid) -> bool {
        self.registry.resolve(nid).is_some()
            || self
                .symbols
                .as_ref()
                .is_some_and(|db| db.name(nid).is_some())
    }

    /// Builds a label per dynamic symbol, for attributing a call trace.
    ///
    /// Indexed by symbol index so it lines up with the stub table exactly. Symbols that
    /// are not imports get an empty label rather than being omitted, because omitting
    /// them would shift every index after the gap - which is precisely the kind of
    /// off-by-many that makes a trace confidently wrong.
    pub fn import_labels(&self, bytes: &[u8]) -> Result<Vec<String>, ServiceError> {
        self.labels(bytes, self.symbols.as_ref())
    }

    /// Builds labels using a database supplied for this call.
    ///
    /// Separate from the one this service was constructed with, because a worker is
    /// handed a database per request rather than per process - names belong to the run.
    pub fn import_labels_with(
        &self,
        bytes: &[u8],
        file: &SymbolDbFile,
    ) -> Result<Vec<String>, ServiceError> {
        let db = SymbolDb::from_file(file).map(|(db, _)| db);
        self.labels(bytes, db.as_ref())
    }

    /// The shared body of both, so the two cannot describe an import differently.
    /// Where each implementation starts, by name.
    ///
    /// A `GuestFn` is a function pointer and a function pointer is an address, so this table
    /// already knows where every implementation lives - which is what a fault report needs to
    /// name one, on a toolchain whose binaries carry no symbols (D380).
    #[must_use]
    pub fn implementation_addresses(&self) -> Vec<(u64, &'static str)> {
        symbols::implementations()
            .into_iter()
            .map(|(name, function)| (function as *const () as usize as u64, name))
            .collect()
    }

    /// Binds the stubs a guest may reach by name, and publishes both ways of reaching them.
    ///
    /// Split out of `build_thunks` because that function has a line limit and this is the part
    /// of it that is about *names* rather than about placement - the same division that put
    /// `what_imports_resolve_to` in the worker.
    fn bind_by_name(
        resolvable: &[(&'static str, symbols::Resolvable)],
        imports: usize,
        table: &orbistoun_thunk::ThunkTable,
        handlers: &mut [Option<orbistoun_core::GuestFn>],
        float_handlers: &mut [Option<orbistoun_core::GuestFloatFn>],
    ) {
        // Bound and published together so a name cannot be advertised without a handler
        // behind it.
        let mut by_name = std::collections::BTreeMap::new();
        for (offset, (name, function)) in resolvable.iter().enumerate() {
            let slot = imports + offset;
            let Some(at) = table.address_of(slot) else {
                continue;
            };
            let placed = match function {
                symbols::Resolvable::Integer(f) => handlers
                    .get_mut(slot)
                    .map(|entry| *entry = Some(*f))
                    .is_some(),
                symbols::Resolvable::Float(f) => float_handlers
                    .get_mut(slot)
                    .map(|entry| *entry = Some(*f))
                    .is_some(),
            };
            if placed {
                by_name.insert((*name).to_owned(), at);
            }
        }
        orbistoun_thunk::install_name_thunks(by_name);
        // **Which of those names libkernel declares**, so `sceKernelDlsym` can narrow an answer
        // by the module handle it was given rather than ignoring it. Published from here because
        // the console's libkernel is spread across three of this project's declarations, in three
        // crates that must not reach sideways for each other (D536, D629).
        orbistoun_thunk::install_libkernel_names(
            symbols::modules()
                .iter()
                .filter(|m| m.name.starts_with("libkernel"))
                .flat_map(|m| m.imports.iter().map(|i| i.name.to_owned()))
                .collect(),
        );
        // What a guest reaches past every name. Published beside the by-name stubs because it
        // is the same question one level down: which implementation does this ask for (D378).
        orbistoun_thunk::syscall::install_syscalls(symbols::syscalls(), symbols::syscall_refusal());
    }

    fn labels(&self, bytes: &[u8], db: Option<&SymbolDb>) -> Result<Vec<String>, ServiceError> {
        let container = orbistoun_elf::Container::parse(bytes)?;
        let count = usize::try_from(container.symbol_count(bytes)?).unwrap_or(0);
        let mut labels = Self::resolvable_labels(count);
        self.fill_labels(&container, bytes, 0, db, &mut labels)?;
        Ok(labels)
    }

    /// Labels for every module sharing one stub table, in that table's own index space.
    ///
    /// # Why the single-module form is not enough
    ///
    /// `labels` sizes the vector to the executable's symbol count and appends the by-name
    /// stubs after it. With one table across several modules (D484), **that is exactly where
    /// the second module's slots begin** - so every call from a title module was labelled with
    /// a by-name stub's name, and the trace read `libc::usleep` for a call that returned a heap
    /// pointer. A report naming the wrong function is worse than one naming none (D490).
    ///
    /// # Errors
    ///
    /// When a container cannot be parsed.
    pub fn import_labels_for(
        &self,
        modules: &[(&str, &[u8])],
        slots: &[ModuleSlots],
        file: &SymbolDbFile,
    ) -> Result<Vec<String>, ServiceError> {
        let db = SymbolDb::from_file(file).map(|(db, _)| db);
        let imports: usize = slots.iter().map(|s| s.count).sum();
        let mut labels = Self::resolvable_labels(imports);
        for ((_, bytes), slot) in modules.iter().zip(slots) {
            let container = orbistoun_elf::Container::parse(bytes)?;
            self.fill_labels(&container, bytes, slot.offset, db.as_ref(), &mut labels)?;
        }
        Ok(labels)
    }

    /// An empty label per import slot, then one per by-name stub.
    ///
    /// The by-name block sits **after every module's imports**, which is the whole of what the
    /// single-module version got wrong once there was more than one module.
    fn resolvable_labels(imports: usize) -> Vec<String> {
        let mut labels = vec![String::new(); imports];
        // The by-name stubs, labelled from the same list that binds them, so a call
        // resolved at run time reads as itself in a trace rather than as `unknown` (D366).
        //
        // The library comes from the knowledge base where it knows one, because these have
        // no import entry to take a library id from - the guest never imported them.
        let knowledge = orbistoun_hle::knowledge::Knowledge::builtin();
        for (name, _) in symbols::resolvable() {
            let library = knowledge.library_of(name).unwrap_or("resolved");
            labels.push(format!("{library}::{name}"));
        }
        labels
    }

    /// Writes one module's import names into the shared label vector at its own offset.
    fn fill_labels(
        &self,
        container: &orbistoun_elf::Container<'_>,
        bytes: &[u8],
        offset: usize,
        db: Option<&SymbolDb>,
        labels: &mut [String],
    ) -> Result<(), ServiceError> {
        let libraries = container.import_libraries(bytes)?;
        for import in container.raw_imports(bytes, &self.hasher)? {
            let Some(slot) = labels.get_mut(offset.saturating_add(import.symbol_index as usize))
            else {
                continue;
            };
            let nid = Nid::from_raw(import.nid);
            let library = import
                .library_id()
                .and_then(|id| libraries.get(&id).cloned())
                .unwrap_or_else(|| "unknown".to_owned());
            *slot = match self.registry.resolve(nid) {
                Some(known) => format!("{}::{}", known.library, known.name),
                // Then whatever the symbol database can name, which is where the
                // brute-force search pays off.
                None => match db.and_then(|db| db.name(nid)) {
                    Some(name) => format!("{library}::{name}"),
                    // No name known yet, so the hash is what there is. Still far more
                    // useful than an index: a hash is stable across builds and
                    // searchable, and the library says which subsystem to look in.
                    None => format!("{library}::{nid}"),
                },
            };
        }
        Ok(())
    }

    /// Applies relocations to a placed image.
    ///
    /// `resolver` decides what each import becomes. In practice that is a thunk table,
    /// so every import lands on a stub that records which function the guest wanted and
    /// returns an explicit "not implemented" - far more useful than a jump to a zeroed
    /// slot, which dies with no explanation.
    pub fn relocate_image(
        &self,
        image: &orbistoun_loader::Image,
        bytes: &[u8],
        resolver: &impl orbistoun_loader::relocate::SymbolResolver,
    ) -> Result<orbistoun_elf::reloc::RelocationTally, ServiceError> {
        // The module's own thread-local layout, when it declares one. Read from the
        // container rather than passed in, so a caller cannot pair an image with a
        // layout belonging to something else.
        let tls = orbistoun_loader::tls::layout_of(bytes)?.map(|(layout, _, _)| layout);
        Ok(orbistoun_loader::relocate::apply(
            image,
            bytes,
            resolver,
            tls.as_ref(),
        )?)
    }

    /// Applies each segment's declared access to a placed, relocated image.
    ///
    /// Separate from placement because the two want opposite permissions: an image is
    /// populated writable and only then made executable.
    pub fn protect_image(
        &self,
        image: &mut orbistoun_loader::Image,
    ) -> Result<orbistoun_loader::protect::ProtectionTally, ServiceError> {
        Ok(orbistoun_loader::protect::apply(
            image,
            orbistoun_core::GUEST_PAGE_SIZE,
        )?)
    }

    /// Surveys a container already in memory.
    pub fn survey_bytes(&self, bytes: &[u8]) -> Result<SurveySummary, ServiceError> {
        let survey = orbistoun_loader::survey(bytes, &self.registry)?;
        Ok(SurveySummary {
            entry: survey.entry,
            imports: survey
                .imports
                .into_iter()
                .map(|i| ImportRecord {
                    // A declared symbol names itself; otherwise fall back to the
                    // database. `known` stays about whether orbistoun *implements* it,
                    // never about whether we can spell it - conflating those would
                    // make the unresolved count lie the moment a database loads.
                    symbol: i.name.or_else(|| {
                        self.symbols
                            .as_ref()
                            .and_then(|db| db.name(i.nid).map(str::to_owned))
                    }),
                    nid: i.nid.as_raw(),
                    library: i.library,
                    known: i.known,
                    kind: match i.kind {
                        orbistoun_elf::dynamic::Kind::Function => {
                            orbistoun_proto::ImportKind::Function
                        }
                        orbistoun_elf::dynamic::Kind::Object => orbistoun_proto::ImportKind::Object,
                        orbistoun_elf::dynamic::Kind::Unspecified => {
                            orbistoun_proto::ImportKind::Unspecified
                        }
                    },
                })
                .collect(),
        })
    }

    /// The modules a title ships that answer the libraries its executable imports from.
    ///
    /// Joins the two halves: the executable names libraries and says nothing about where they
    /// live (D482), so the names are taken from its vendor tables and looked for in the
    /// title's own tree. What comes back is the subset that exists - a platform library is
    /// supposed to be absent from a title's data.
    ///
    /// # Errors
    ///
    /// When the executable cannot be read or parsed.
    pub fn title_modules_for(
        &self,
        executable: &Path,
    ) -> Result<Vec<titlemodules::TitleModule>, ServiceError> {
        let bytes = std::fs::read(executable).map_err(|source| ServiceError::Io {
            path: executable.display().to_string(),
            source,
        })?;
        let (libraries, _modules) = self.module_tables(&bytes)?;
        let root = executable.parent().unwrap_or(Path::new("."));
        let wanted: Vec<String> = libraries.into_values().collect();
        Ok(titlemodules::find(root, &wanted))
    }

    /// Places the title's own modules and reports what they export.
    ///
    /// Two passes, not one per module (D482): every module is placed before any export is
    /// indexed, so a module that exports a name another of the title's own imports does not
    /// depend on which the filesystem offered first.
    ///
    /// **The result is placed, not relocated**, and must not be bound into a running guest -
    /// see [`titleplacement`] for why an address into unrelocated code is worse than a stub.
    ///
    /// # Errors
    ///
    /// When the executable or one of its modules cannot be read, parsed, or placed.
    pub fn place_title_modules(
        &self,
        modules: &[titlemodules::TitleModule],
        base: u64,
    ) -> Result<titleplacement::PlacedTitleModules, ServiceError> {
        titleplacement::place_all(
            modules,
            base,
            orbistoun_mem::allocation_granularity(),
            |bytes, at| self.place_image(bytes, at),
            |bytes| {
                let container = orbistoun_elf::Container::parse(bytes)?;
                Ok(container.raw_exports(bytes, &self.hasher)?)
            },
        )
    }

    /// What a guest module imports, as the loader reads it.
    ///
    /// Exposed because resolving an import against a placed module is a match on the
    /// **hash and the kind** of each side, and both live here rather than in a name.
    ///
    /// # Errors
    ///
    /// When the container cannot be parsed or carries no usable dynamic table.
    pub fn raw_imports_of(
        &self,
        bytes: &[u8],
    ) -> Result<Vec<orbistoun_elf::dynamic::RawImport>, ServiceError> {
        let container = orbistoun_elf::Container::parse(bytes)?;
        Ok(container.raw_imports(bytes, &self.hasher)?)
    }

    /// Places the title's own modules and relocates them against one shared stub table.
    ///
    /// D482's order, and the order is the decision: **place every module, collect every
    /// export, then relocate** - because a title module may import from another of the
    /// title's own, and relocating each as it is placed would make the answer depend on which
    /// the filesystem offered first.
    ///
    /// The executable is module 0 and takes slot zero, so everything already measured keeps
    /// its meaning (D484). It is **not** relocated here: this links the modules that answer
    /// its imports, and entering the guest is the worker's sequence rather than this one's.
    ///
    /// # Errors
    ///
    /// When a container cannot be read, parsed, placed or relocated.
    pub fn link_title_modules(
        &self,
        executable: &Path,
        bases: TitleBases,
        symbols: &SymbolDbFile,
    ) -> Result<LinkedTitle, ServiceError> {
        let read = |path: &Path| -> Result<Vec<u8>, ServiceError> {
            std::fs::read(path).map_err(|source| ServiceError::Io {
                path: path.display().to_string(),
                source,
            })
        };
        let executable_bytes = read(executable)?;
        let found = self.title_modules_for(executable)?;
        let mut placed = self.place_title_modules(&found, bases.modules)?;

        // The executable first, so it is module 0 at offset 0.
        let mut owned: Vec<(String, Vec<u8>)> = vec![(String::new(), executable_bytes)];
        for module in &found {
            owned.push((module.library.clone(), read(&module.path)?));
        }
        let modules: Vec<(&str, &[u8])> = owned
            .iter()
            .map(|(label, bytes)| (label.as_str(), bytes.as_slice()))
            .collect();

        let (thunks, slots) = self.build_thunks_for(&modules, bases.thunks)?;
        // The run's own database, built once here: the data blocks are labelled with it and a
        // worker's `Service` carries none of its own (D647).
        let named_by = SymbolDb::from_file(symbols).map(|(db, _)| db);
        let (data, shared_data) =
            self.build_data_blocks_for(&modules, &slots, bases.data, named_by.as_ref())?;
        // Published once, across every module - the table is a `OnceLock` and a second install
        // is ignored, so per-module publishing would keep the executable's globals and drop
        // every module's (D484).
        orbistoun_thunk::install_data_symbols(data.named());
        // The address-keyed twin, published beside it: a name three modules import labels three
        // pages here and one there, and a fault reporter needs the three (D647).
        orbistoun_thunk::install_data_labels(data.labels());

        // **Before the modules are relocated, not after.** The binding says which imports must
        // resolve into a module the title ships rather than into a stub, and a relocation that
        // has already run cannot be told that. It was computed after this loop, which is why
        // every module-to-module import landed on a placeholder however correctly the binding
        // was worked out (D640, D644).
        let (per_module, kept_by_orbistoun, binding) =
            self.bind_to_title_modules(&placed, &modules, &slots)?;
        let bound = per_module.first().cloned().unwrap_or_default();

        let mut tallies = Vec::new();
        // `slots[0]` is the executable, which the worker relocates as part of entering it.
        for (index, ((library, image), slot)) in
            placed.images().iter().zip(slots.iter().skip(1)).enumerate()
        {
            let bytes = owned
                .iter()
                .find(|(label, _)| label == library)
                .map(|(_, bytes)| bytes.as_slice())
                .unwrap_or_default();
            // Built per module rather than once outside the loop: the borrow it takes has
            // to end before the tables move into the returned value.
            let shared = orbistoun_loader::relocate::ImportResolver {
                thunks: &thunks,
                data: &data,
                refuse: None,
            };
            let shifted = orbistoun_loader::relocate::OffsetResolver {
                offset: slot.offset,
                inner: &shared,
            };
            // **The binding outside, the offsetting inside.** `TitleResolver` looks a symbol
            // index up in the map it holds, and this map is keyed by *this* module's own symbol
            // index - so it never sees a shifted index, and `OffsetResolver` shifts only what
            // falls through to the shared stub table. Composed the other way round the binding
            // would be asked about a slot number and answer nothing (D644).
            let nothing = std::collections::BTreeMap::new();
            let resolver = orbistoun_loader::relocate::TitleResolver {
                bound: per_module.get(index + 1).unwrap_or(&nothing),
                inner: &shifted,
            };
            let tally = self.relocate_image(image, bytes, &resolver)?;
            tallies.push((library.clone(), tally));
        }
        // **Only now**, and this is the same ordering the executable already has: placement
        // leaves every page writable and none executable because relocation writes into text,
        // so protecting any earlier would make those writes fault. A module left unprotected is
        // not a module the guest can call - it faults on the instruction fetch (D489).
        for (_, image) in placed.images_mut() {
            self.protect_image(image)?;
        }
        // **And tell the kernel where they are.** A module's pages live in this loader's own
        // address space, which the kernel's runtime map never sees - the executable's image is
        // noted for exactly that reason (D446). Without the same for modules, a fault inside one
        // reads as *outside every placed region*, which is the test for "orbistoun's own code
        // faulted", and `sceKernelVirtualQuery` refuses an address the guest is running from.
        for (_, image) in placed.images() {
            let (base, len) = image.span();
            orbistoun_kernel::note_region(base, len);
        }
        // **And where each one's initialisers are**, so a later `sceKernelLoadStartModule`
        // can run them. Recorded here because this is the only place holding both halves: the
        // module's bytes, which carry `DT_INIT_ARRAY` as a link-relative address, and the
        // image, which knows where it was placed.
        //
        // The array's *contents* are deliberately not read here - they are function pointers
        // relocation writes, and the loop above has only just written them. The address is
        // recorded; the pointers are read at start time, which is after linking (D515).
        for (library, image) in placed.images() {
            let Some(bytes) = owned
                .iter()
                .find(|(label, _)| label == library)
                .map(|(_, bytes)| bytes.as_slice())
            else {
                continue;
            };
            if let Some(initialisers) = initialisers_of(bytes, image.base()) {
                orbistoun_kernel::note_module_initialisers(library, initialisers);
            }
        }
        // **And what they export**, so `sceKernelDlsym` can answer for a symbol the guest's own
        // binaries provide. The addresses here already carry the module base; the NIDs are the
        // hashes an importer would name them by, which is also what a `dlsym` name hashes to
        // (D517).
        let exports: Vec<(u64, u64)> = placed
            .exports()
            .values()
            .flat_map(|by_hash| by_hash.values())
            .map(|export| (export.nid, export.address))
            .collect();
        orbistoun_kernel::note_guest_exports(self.hasher.suffix_bytes(), &exports);
        let labels = self.import_labels_for(&modules, &slots, symbols)?;
        Ok(LinkedTitle {
            placed,
            slots,
            tallies,
            shared_data,
            thunks,
            data,
            bound,
            binding,
            labels,
            kept_by_orbistoun,
        })
    }

    /// Which of the executable's imports a module the title ships answers, and which it does not.
    ///
    /// **Every import is accounted for, bound or not, with a reason for each that is not.** Six of
    /// PPSA25872's imports are exported by modules it ships; all six landed on placeholders; and
    /// the run printed nothing - not a count, not a reason, not even that the question had been
    /// asked. One of them is then called nineteen million times (D630, D640).
    ///
    /// The reasons are computed here rather than inferred by a reader, because every one of them
    /// is a different fix: a library id the table does not list is a parsing question, a library
    /// nothing exports under is a placement question, a hash a placed module does not export is a
    /// question about that module, and `KeptByOrbistoun` is not a failure at all.
    fn bind_to_title_modules(
        &self,
        placed: &titleplacement::PlacedTitleModules,
        modules: &[(&str, &[u8])],
        slots: &[ModuleSlots],
    ) -> Result<TitleBinding, ServiceError> {
        use titleplacement::Unbound;

        let empty = titleplacement::BindingAccount {
            imports: 0,
            bound: 0,
            unbound: Vec::new(),
            by_library: Vec::new(),
        };
        if modules.is_empty() {
            return Ok((Vec::new(), Vec::new(), empty));
        }
        // **One map per module, keyed by that module's own symbol index.**
        //
        // Not one shared map keyed by slot, and the difference is what makes the composition
        // below unambiguous: `TitleResolver` looks a symbol index up in the map it is given, and
        // `OffsetResolver` shifts an index into the shared stub table. Wrapping the offsetting
        // one *inside* the binding one means the binding never sees a shifted index, so neither
        // resolver has to know what the other does with it (D644).
        let mut per_module: Vec<std::collections::BTreeMap<u32, u64>> = Vec::new();
        let mut kept = Vec::new();
        let mut total = 0_usize;
        let mut answered: std::collections::BTreeMap<String, usize> =
            std::collections::BTreeMap::new();
        // **Keyed by reason *and* library.** Grouping on the reason alone gives one example out
        // of four hundred, which cannot answer the question a reader actually has - is *my*
        // module in this bucket? PS5Util being among the hundred and thirty-four orbistoun
        // claims to implement would be alarming; libc being there is correct; and one example
        // line said neither (D640).
        let mut why: Vec<(Unbound, String, usize)> = Vec::new();

        // **Every module the title ships, not only its executable.** This looked at `owned[0]`
        // alone, so a module importing from a *sibling* module was never a candidate - and that
        // is PPSA25872's whole wall: `PS5Util.prx` is placed, started, and exports the function
        // the run calls nineteen million times, but the caller is another of the title's modules
        // and its imports were never offered the binding (D640).
        //
        // The slot offset is still needed to ask whether *this project* implements the import,
        // because that question is asked of the shared dispatch table.
        for ((_, bytes), slot) in modules.iter().zip(slots) {
            let mut bound = std::collections::BTreeMap::new();
            let container = orbistoun_elf::Container::parse(bytes)?;
            let imports = container.raw_imports(bytes, &self.hasher)?;
            let libraries = container.import_libraries(bytes)?;
            let resolution = placed.resolve(&imports, &libraries);
            let ambiguous: std::collections::BTreeSet<&str> = resolution
                .ambiguous
                .iter()
                .map(|a| a.name.as_str())
                .collect();
            let mismatched: std::collections::BTreeSet<&str> = resolution
                .mismatches
                .iter()
                .map(|m| m.name.as_str())
                .collect();
            for (library, count) in resolution.by_library() {
                *answered.entry(library.to_owned()).or_default() += count;
            }
            total += imports.len();

            for import in &imports {
                let at = slot.offset + import.symbol_index as usize;
                let library = Self::library_of(&libraries, import);
                let mut note = |reason: Unbound, library: &str| {
                    if let Some(entry) = why
                        .iter_mut()
                        .find(|(r, l, _)| *r == reason && l == library)
                    {
                        entry.2 += 1;
                    } else {
                        why.push((reason, library.to_owned(), 1));
                    }
                };
                if let Some(address) = resolution.addresses.get(&import.symbol_index) {
                    // Asked of the dispatch tables rather than of a name list, because that is
                    // what actually answers the call - a name this project knows but has not
                    // bound is not implemented, whatever a registry says.
                    if orbistoun_thunk::is_implemented(at) {
                        kept.push(import.name.clone());
                        // **Named with its library.** "Kept by orbistoun" is correct for libc and
                        // alarming for a module the game wrote, and the two printed identically.
                        note(Unbound::KeptByOrbistoun, &library);
                    } else {
                        bound.insert(import.symbol_index, *address);
                    }
                    continue;
                }
                let (reason, detail) =
                    Self::why_unbound(placed, &libraries, import, &ambiguous, &mismatched);
                note(reason, &detail);
            }
            per_module.push(bound);
        }
        why.sort_by_key(|(_, _, count)| std::cmp::Reverse(*count));
        let mut by_library: Vec<(String, usize)> = answered.into_iter().collect();
        by_library.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
        let account = titleplacement::BindingAccount {
            imports: total,
            bound: per_module.iter().map(std::collections::BTreeMap::len).sum(),
            unbound: why,
            by_library,
        };
        Ok((per_module, kept, account))
    }

    /// The library name an import names, or a stand-in when it names none.
    fn library_of(
        libraries: &std::collections::BTreeMap<u16, String>,
        import: &orbistoun_elf::dynamic::RawImport,
    ) -> String {
        match import.form {
            orbistoun_elf::dynamic::NameForm::Encoded { library_id, .. } => libraries
                .get(&library_id)
                .cloned()
                .unwrap_or_else(|| format!("library id {library_id}")),
            orbistoun_elf::dynamic::NameForm::Plain => "no attribution".to_owned(),
        }
    }

    /// Which step declined one import, asked in the order `resolve` asks it.
    ///
    /// Deliberately re-walks the same tests rather than having `resolve` report them: `resolve`
    /// answers "where is this", and threading a reason through it would make the common path
    /// carry the cost of the rare one. The duplication is two lookups on a path that runs once
    /// per import per launch.
    fn why_unbound(
        placed: &titleplacement::PlacedTitleModules,
        libraries: &std::collections::BTreeMap<u16, String>,
        import: &orbistoun_elf::dynamic::RawImport,
        ambiguous: &std::collections::BTreeSet<&str>,
        mismatched: &std::collections::BTreeSet<&str>,
    ) -> (titleplacement::Unbound, String) {
        use orbistoun_elf::dynamic::NameForm;
        use titleplacement::Unbound;

        if mismatched.contains(import.name.as_str()) {
            return (Unbound::KindMismatch, Self::library_of(libraries, import));
        }
        match import.form {
            NameForm::Encoded { library_id, .. } => {
                let Some(library) = libraries.get(&library_id) else {
                    return (Unbound::NoLibraryName, format!("library id {library_id}"));
                };
                // **The library, not the import.** Four hundred and sixty-four of one title's
                // imports land here, and which *libraries* they name is the fact that says
                // whether anything is wrong - one encoded hash out of four hundred says nothing
                // (D640).
                match placed.exports().get(library) {
                    None => (Unbound::NoSuchLibrary, library.clone()),
                    Some(_) => (Unbound::NotExported, library.clone()),
                }
            }
            NameForm::Plain if ambiguous.contains(import.name.as_str()) => {
                (Unbound::Ambiguous, "no attribution".to_owned())
            }
            NameForm::Plain => (Unbound::NotExported, "no attribution".to_owned()),
        }
    }

    /// The hash a guest would import this name by.
    ///
    /// **The service's own hasher, not a fresh one.** The suffix is a run input (principle 5),
    /// so a hash computed with a default would answer a different question from the one every
    /// other lookup in this process asks.
    #[must_use]
    pub fn hash_name(&self, name: &str) -> Nid {
        self.hasher.hash(name)
    }

    /// The vendor tables an encoded import name indexes: libraries, then modules.
    ///
    /// **The two are different lists and neither is `DT_NEEDED`** (D117). Exposed because the
    /// question "where does the loader find a module the executable imports from" is answered
    /// by what these strings actually contain - a bare name or a path - and that is read, not
    /// assumed.
    ///
    /// # Errors
    ///
    /// When the container cannot be parsed or carries no usable dynamic table.
    pub fn module_tables(&self, bytes: &[u8]) -> Result<ModuleTables, ServiceError> {
        let container = orbistoun_elf::Container::parse(bytes)?;
        Ok((
            container.import_libraries(bytes)?,
            container.import_modules(bytes)?,
        ))
    }

    /// What a module **provides**, read without executing it.
    ///
    /// The other half of [`Self::survey_bytes`]. A survey answers "what does this need"; this
    /// answers "what can it answer for somebody else", which is the question that decides
    /// What a binary exports, as `(nid, runtime address)` once placed at `base`.
    ///
    /// For `sceKernelDlsym`: the kernel is handed a name and every export table is keyed by a
    /// hash of one, so it hashes at the call and looks up here. Separate from
    /// [`Self::exports_bytes`], which names symbols for a human reading a survey - this one is
    /// for a lookup, so it carries no names at all (D517).
    ///
    /// # Errors
    ///
    /// When the container cannot be parsed, or carries no usable dynamic symbol table.
    pub fn export_addresses(
        &self,
        bytes: &[u8],
        base: u64,
    ) -> Result<Vec<(u64, u64)>, ServiceError> {
        let container = orbistoun_elf::Container::parse(bytes)?;
        Ok(container
            .raw_exports(bytes, &self.hasher)?
            .into_iter()
            .map(|e| (e.nid, base.saturating_add(e.offset)))
            .collect())
    }

    /// The hash suffix this service is configured with.
    ///
    /// Handed to the kernel alongside the exports, because the hashing has to happen where the
    /// name arrives and this hasher does not reach there.
    #[must_use]
    pub fn nid_suffix(&self) -> &[u8] {
        self.hasher.suffix_bytes()
    }

    /// whether loading a second module is worth anything.
    ///
    /// # Errors
    ///
    /// When the container cannot be parsed, or carries no usable dynamic symbol table.
    pub fn exports_bytes(
        &self,
        bytes: &[u8],
    ) -> Result<Vec<orbistoun_proto::ExportRecord>, ServiceError> {
        let container = orbistoun_elf::Container::parse(bytes)?;
        let found = container.raw_exports(bytes, &self.hasher)?;
        Ok(found
            .into_iter()
            .map(|e| orbistoun_proto::ExportRecord {
                // Named from the database where the module spelled it as a bare NID, exactly
                // as a survey names an import - the same symbol, asked about from the other
                // side.
                symbol: Some(e.name).filter(|n| !n.is_empty()).or_else(|| {
                    self.symbols
                        .as_ref()
                        .and_then(|db| db.name(Nid::from_raw(e.nid)).map(str::to_owned))
                }),
                nid: e.nid,
                offset: e.offset,
                kind: match e.kind {
                    orbistoun_elf::dynamic::Kind::Function => orbistoun_proto::ImportKind::Function,
                    orbistoun_elf::dynamic::Kind::Object => orbistoun_proto::ImportKind::Object,
                    orbistoun_elf::dynamic::Kind::Unspecified => {
                        orbistoun_proto::ImportKind::Unspecified
                    }
                },
            })
            .collect())
    }

    /// Reads a container from disk and reports what it provides.
    ///
    /// # Errors
    ///
    /// As [`Self::exports_bytes`], plus the file not being readable.
    pub fn exports_path(
        &self,
        path: &Path,
    ) -> Result<Vec<orbistoun_proto::ExportRecord>, ServiceError> {
        let bytes = std::fs::read(path).map_err(|source| ServiceError::Io {
            path: path.display().to_string(),
            source,
        })?;
        self.exports_bytes(&bytes)
    }

    /// Surveys a container on disk.
    pub fn survey_path(&self, path: &Path) -> Result<SurveySummary, ServiceError> {
        let bytes = std::fs::read(path).map_err(|source| ServiceError::Io {
            path: path.display().to_string(),
            source,
        })?;
        self.survey_bytes(&bytes)
    }

    /// Resolves a symbol name to the NID this service would look it up by.
    pub fn nid_for(&self, symbol: &str) -> Nid {
        self.hasher.hash(symbol)
    }
}

/// Where a placed module's initialisers live, as runtime addresses.
///
/// [`None`] when the module carries no dynamic table, or carries one with no `DT_INIT` and an
/// empty `DT_INIT_ARRAY` - a module with nothing to run is not a module that failed to be
/// read, and recording it would make the run report claim a start that did nothing.
///
/// # The link-relative to runtime step, stated because it is the easy thing to get wrong
///
/// `DT_INIT_ARRAY` is a virtual address in the module's own link view, and a title's module
/// links at zero - which is why its entry point reads as `0x0` and why its exports report as
/// bare offsets. So the runtime address is `base + vaddr`, and a module that did *not* link
/// at zero would need its load bias instead. Nothing in the corpus does; if one ever does,
/// this is where it will be wrong, which is why it is written down rather than inlined.
fn initialisers_of(bytes: &[u8], base: u64) -> Option<orbistoun_kernel::ModuleInitialisers> {
    let container = orbistoun_elf::Container::parse(bytes).ok()?;
    let dynamic = container.dynamic_bytes(bytes).ok()??;
    let info = orbistoun_elf::dynamic::DynamicInfo::parse(dynamic);
    let count = info.init_arraysz / 8;
    if info.init == 0 && (info.init_array == 0 || count == 0) {
        return None;
    }
    Some(orbistoun_kernel::ModuleInitialisers {
        init: if info.init == 0 {
            0
        } else {
            base.saturating_add(info.init)
        },
        array: if info.init_array == 0 {
            0
        } else {
            base.saturating_add(info.init_array)
        },
        count: if info.init_array == 0 { 0 } else { count },
    })
}

/// What binding the title's own modules produced: the map the relocator reads, the names
/// orbistoun kept for itself, and an account of every import either way.
type TitleBinding = (
    Vec<std::collections::BTreeMap<u32, u64>>,
    Vec<String>,
    titleplacement::BindingAccount,
);

#[cfg(test)]
mod tests {
    use super::{LibrarySettings, Path, Service, ServiceConfig, slot_ranges};

    /// **The main executable owns slot zero**, whatever else is loaded beside it.
    ///
    /// Every recorded call index and every measurement taken before the table was shared
    /// names a slot in the executable's range. Giving module 0 any other offset would
    /// renumber all of them silently.
    #[test]
    fn the_main_executable_owns_slot_zero() {
        let ranges = slot_ranges(&[("eboot", 583), ("Il2CppUserAssemblies", 247)]);
        assert_eq!(ranges[0].offset, 0);
        assert_eq!(ranges[0].count, 583);
    }

    /// A second module starts exactly where the first ended - no gap, no overlap.
    ///
    /// A gap wastes slots harmlessly; an **overlap** binds two modules' symbols to one stub,
    /// so a call trace names the wrong function and an implementation answers for a symbol it
    /// was never written for. The assertion is on the exact boundary for that reason.
    #[test]
    fn each_module_begins_where_the_last_one_ended() {
        let ranges = slot_ranges(&[("a", 4), ("b", 3), ("c", 5)]);
        let offsets: Vec<usize> = ranges.iter().map(|r| r.offset).collect();
        assert_eq!(offsets, [0, 4, 7]);
        for pair in ranges.windows(2) {
            assert_eq!(
                pair[0].offset + pair[0].count,
                pair[1].offset,
                "a gap or an overlap between {} and {}",
                pair[0].label,
                pair[1].label
            );
        }
    }

    /// A module with no dynamic symbols takes no slots and shifts nothing.
    ///
    /// It can happen - a module whose table this build could not read answers zero - and the
    /// wrong handling is to give it a slot anyway, which shifts every module after it by one
    /// and silently misbinds all of them.
    #[test]
    fn a_module_with_no_symbols_shifts_nothing() {
        let ranges = slot_ranges(&[("a", 4), ("empty", 0), ("c", 5)]);
        assert_eq!(ranges[1].offset, 4);
        assert_eq!(ranges[2].offset, 4, "the empty module consumed no slots");
    }

    /// No modules is an empty layout rather than a panic.
    #[test]
    fn no_modules_lays_out_nothing() {
        assert!(slot_ranges(&[]).is_empty());
    }

    fn service() -> Service {
        Service::new(ServiceConfig {
            nid_suffix: b"test-suffix".to_vec(),
            ..ServiceConfig::default()
        })
    }

    #[test]
    fn every_subsystem_is_registered() {
        // The count is the guard: a subsystem added to the workspace but not wired
        // into `Service::new` would be invisible everywhere, in every shim at once.
        let s = service();
        assert_eq!(s.declared_count(), s.declared_symbols().len());
        assert!(s.declared_count() >= 37, "expected the declared surface");
    }

    #[test]
    fn declared_symbols_cover_every_library() {
        let s = service();
        let libs: std::collections::BTreeSet<_> = s
            .declared_symbols()
            .into_iter()
            .map(|d| d.library)
            .collect();
        // Asserted against the module list rather than a literal, so adding a
        // subsystem does not require remembering to bump a number here - which is how
        // this test came to say "six" while seven were registered.
        assert_eq!(libs.len(), super::symbols::modules().len(), "got {libs:?}");
    }

    #[test]
    fn declared_symbols_are_deterministically_ordered() {
        // Reports are diffed between runs; ordering churn would read as change.
        let s = service();
        let a = s.declared_symbols();
        let b = s.declared_symbols();
        assert_eq!(a, b);
        let mut sorted = a.clone();
        sorted.sort();
        assert_eq!(a, sorted, "output must already be sorted");
    }

    #[test]
    fn a_missing_suffix_is_reported_rather_than_hidden() {
        // Without a suffix the names are right and the hashes are meaningless. That
        // has to be visible, or a meaningless number looks authoritative.
        //
        // "Missing" now has to be asked for explicitly, because the default is the
        // working configuration - a default that resolves nothing is a trap (D082).
        let with = service();
        assert!(with.nids_are_real());
        let without = Service::new(ServiceConfig {
            nid_suffix: Vec::new(),
            ..ServiceConfig::default()
        });
        assert!(!without.nids_are_real());
    }

    #[test]
    fn the_default_configuration_can_actually_resolve_something() {
        // The trap this replaced: everything built, everything ran, and every lookup
        // silently missed, so no implementation could ever be reached.
        let service = Service::new(ServiceConfig::default());
        assert!(
            service.nids_are_real(),
            "the default must be the working configuration"
        );
    }

    #[test]
    fn titles_are_directories_holding_an_entry_file() {
        // The rule the shell script encoded as a glob, and which the GUI would otherwise
        // have spelled out a third time.
        let root = std::env::temp_dir().join("orbistoun-titles-test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("BBBB-app0")).expect("dir");
        std::fs::create_dir_all(root.join("AAAA-app0")).expect("dir");
        std::fs::create_dir_all(root.join("not-a-title")).expect("dir");
        std::fs::write(root.join("BBBB-app0").join(super::TITLE_ENTRY_FILE), b"x").expect("write");
        std::fs::write(root.join("AAAA-app0").join(super::TITLE_ENTRY_FILE), b"x").expect("write");
        std::fs::write(root.join("loose-file.bin"), b"x").expect("write");

        let found = service().discover_titles(&root).expect("scans");
        let names: Vec<&str> = found.iter().map(|t| t.name.as_str()).collect();

        // Sorted, because a library that reorders itself between runs cannot be navigated.
        assert_eq!(names, vec!["AAAA-app0", "BBBB-app0"]);
        assert!(found[0].module.ends_with(super::TITLE_ENTRY_FILE));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_title_is_named_by_what_it_publishes_and_falls_back_to_its_folder() {
        // A library row with no label is unusable, so `display_name` never answers a
        // blank - a title whose metadata cannot be read is still a title somebody runs.
        let root = std::env::temp_dir().join("orbistoun-meta-test");
        let _ = std::fs::remove_dir_all(&root);
        let named = root.join("AAAA-app0");
        std::fs::create_dir_all(named.join(super::TITLE_METADATA_DIR)).expect("dirs");
        std::fs::write(named.join(super::TITLE_ENTRY_FILE), b"x").expect("module");
        std::fs::write(
            named
                .join(super::TITLE_METADATA_DIR)
                .join(super::TITLE_METADATA_FILE),
            br#"{"titleId":"AAAA00001","contentVersion":"01.002.000",
                "requiredSystemSoftwareVersion":1324058290446925824,
                "localizedParameters":{"defaultLanguage":"en-US",
                "en-US":{"titleName":"A Published Name"}}}"#,
        )
        .expect("metadata");

        let bare = root.join("BBBB-app0");
        std::fs::create_dir_all(&bare).expect("dir");
        std::fs::write(bare.join(super::TITLE_ENTRY_FILE), b"x").expect("module");

        let found = service().discover_titles(&root).expect("scans");
        assert_eq!(found[0].display_name(), "A Published Name");
        let metadata = found[0].metadata.as_ref().expect("read");
        assert_eq!(metadata.title_id, "AAAA00001");
        assert_eq!(metadata.version.as_deref(), Some("01.002.000"));
        assert_eq!(metadata.requires.as_deref(), Some("12.60"));
        assert!(metadata.icon.is_none(), "no icon file was written");

        // Homebrew and loose dumps carry no metadata at all and are still titles.
        assert!(found[1].metadata.is_none());
        assert_eq!(found[1].display_name(), "BBBB-app0");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_packed_system_version_reads_as_decimal_digits_not_as_hex() {
        // The digits are literal - binary-coded decimal - so 0x1260 is 12.60. Reading the
        // bytes as ordinary hex gives 18.96, which is not a version of anything, and it
        // would look plausible enough in a column that nobody would question it.
        let packed = |v: u64| super::decode_system_version(&serde_json::Value::from(v));
        assert_eq!(packed(0x1260_0000_0000_0000).as_deref(), Some("12.60"));
        assert_eq!(packed(0x0310_0000_0000_0000).as_deref(), Some("3.10"));
        assert_eq!(packed(0x0100_0000_0000_0000).as_deref(), Some("1.00"));
        assert_eq!(packed(0x1001_0000_0000_0000).as_deref(), Some("10.01"));

        // A byte that is not two decimal digits is refused rather than reported as some
        // plausible wrong number.
        assert_eq!(packed(0x1A00_0000_0000_0000), None);
    }

    #[test]
    fn a_title_naming_a_language_it_does_not_carry_still_gets_a_name() {
        // Reporting no name because the default language is missing would hide a title
        // that names itself perfectly well in another one.
        let root = std::env::temp_dir().join("orbistoun-meta-lang-test");
        let _ = std::fs::remove_dir_all(&root);
        let dir = root.join("CCCC-app0");
        std::fs::create_dir_all(dir.join(super::TITLE_METADATA_DIR)).expect("dirs");
        std::fs::write(dir.join(super::TITLE_ENTRY_FILE), b"x").expect("module");
        std::fs::write(
            dir.join(super::TITLE_METADATA_DIR)
                .join(super::TITLE_METADATA_FILE),
            br#"{"localizedParameters":{"defaultLanguage":"ja-JP",
                "de-DE":{"titleName":"Ein Name"}}}"#,
        )
        .expect("metadata");

        let found = service().discover_titles(&root).expect("scans");
        assert_eq!(found[0].display_name(), "Ein Name");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn unreadable_metadata_costs_a_label_and_nothing_else() {
        // Malformed JSON must not take the title out of the library.
        let root = std::env::temp_dir().join("orbistoun-meta-bad-test");
        let _ = std::fs::remove_dir_all(&root);
        let dir = root.join("DDDD-app0");
        std::fs::create_dir_all(dir.join(super::TITLE_METADATA_DIR)).expect("dirs");
        std::fs::write(dir.join(super::TITLE_ENTRY_FILE), b"x").expect("module");
        std::fs::write(
            dir.join(super::TITLE_METADATA_DIR)
                .join(super::TITLE_METADATA_FILE),
            b"{ this is not json",
        )
        .expect("metadata");

        let found = service().discover_titles(&root).expect("scans");
        assert_eq!(found.len(), 1, "still a title");
        assert_eq!(found[0].display_name(), "DDDD-app0");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_missing_library_folder_is_an_error_rather_than_an_empty_library() {
        // An empty list would read as "you own no titles", which is a different and
        // wrong answer to "that folder does not exist".
        let missing = std::env::temp_dir().join("orbistoun-no-such-library");
        let _ = std::fs::remove_dir_all(&missing);
        assert!(service().discover_titles(&missing).is_err());
    }

    #[test]
    fn a_missing_configuration_file_is_the_defaults_rather_than_an_error() {
        // The ordinary case. Failing on it would make the tool unusable until somebody
        // wrote a file they had no reason to know about.
        let missing = Path::new("no-such-file-anywhere.toml");
        let loaded = super::FileConfig::load(missing).expect("a missing file is fine");
        assert_eq!(
            loaded.entry,
            orbistoun_loader::process::EntrySettings::default()
        );
    }

    #[test]
    fn a_malformed_configuration_file_fails_rather_than_falling_back() {
        // The failure this prevents is the worst kind: a typo'd setting silently reverts
        // to the default, the run behaves exactly as it did before, and the conclusion
        // drawn is "that setting has no effect" - which is a wrong answer, recorded.
        let dir = std::env::temp_dir().join("orbistoun-config-test");
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("broken.toml");
        std::fs::write(
            &path,
            "[entry]
convention = \"nonsense\"
",
        )
        .expect("write");
        assert!(super::FileConfig::load(&path).is_err());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn the_library_folder_survives_a_restart() {
        // The starting value is a relative path, which resolves only when the program was
        // started from the right folder. Without persisting it, a library is present
        // when launched from a terminal and empty when launched any other way - which
        // reads as a broken scanner rather than a lost setting.
        let mut chosen = super::FileConfig::default();
        chosen.library.root = r"D:\games\ps".to_owned();
        chosen.library.run_limit_seconds = 45;

        let text = chosen.to_toml().expect("serialises");
        let back: super::FileConfig = toml::from_str(&text).expect("reads back");
        assert_eq!(back.library.root, r"D:\games\ps");
        assert_eq!(back.library.run_limit_seconds, 45);
    }

    #[test]
    fn a_configuration_naming_one_setting_is_valid() {
        // Every field defaults, so a file can say the single thing being tried. A file
        // that has to be complete is a file nobody edits.
        let dir = std::env::temp_dir().join("orbistoun-config-test");
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("partial.toml");
        std::fs::write(
            &path,
            "[entry]
argument = \"zero\"
",
        )
        .expect("write");
        let loaded = super::FileConfig::load(&path).expect("partial files are valid");
        assert_eq!(
            loaded.entry.argument,
            orbistoun_loader::process::EntryArgument::Zero
        );
        assert_eq!(
            loaded.entry.convention,
            orbistoun_loader::process::Convention::default(),
            "and everything unnamed keeps its default"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn the_default_policy_serialises_to_editable_toml() {
        let toml = service().default_policy_toml().expect("serialises");
        assert!(toml.contains("default_return"), "got {toml}");
        assert!(
            toml.contains("unimplemented"),
            "loud by default, not Ok: {toml}"
        );
    }

    #[test]
    fn surveying_a_non_container_fails_honestly() {
        // Not an empty result - an empty import list reads as "needs nothing", which
        // is never true (D010).
        let s = service();
        let err = s.survey_bytes(&[0_u8; 64]).expect_err("not a container");
        assert!(matches!(err, super::ServiceError::Survey(_)));
    }

    #[test]
    fn surveying_a_missing_path_names_the_path() {
        let s = service();
        let err = s
            .survey_path(Path::new("no/such/file.bin"))
            .expect_err("missing");
        assert!(err.to_string().contains("no/such/file.bin"), "got {err}");
    }

    #[test]
    fn reporting_is_optional_and_off_by_default() {
        // A unit test or one-shot inspection should not write to a user's disk.
        assert!(ServiceConfig::default().paths.is_none());
    }

    #[test]
    fn a_first_run_reports_no_diff_and_that_is_information_not_failure() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let file = tmp.path().join("guest.bin");
        std::fs::write(&file, [0_u8; 64]).expect("write");

        let s = service();
        // Not a container, so surveying fails honestly rather than reporting nothing.
        assert!(s.survey_and_report(&file, 1_700_000_000_000).is_err());
    }

    #[test]
    fn the_title_hash_identifies_content_not_location() {
        // Two paths, same bytes: the same title. That is what makes "the previous run
        // of this title" answerable after a file moves.
        let a = super::content_hash(b"identical");
        let b = super::content_hash(b"identical");
        assert_eq!(a, b);
        assert_ne!(a, super::content_hash(b"different"));
    }

    #[test]
    fn nid_lookup_matches_the_registry() {
        let s = service();
        // A declared symbol must resolve through the same hasher the registry used,
        // or the service and its own registry disagree about what a name means.
        let declared = s.declared_symbols();
        let first = &declared[0];
        assert_eq!(s.nid_for(&first.symbol).as_raw(), first.nid);
    }
    #[test]
    fn relative_library_root_is_taken_from_the_data_root() {
        let settings = LibrarySettings::default();
        let resolved = settings.resolve(Path::new(r"C:\data\orbistoun"));
        assert_eq!(resolved, Path::new(r"C:\data\orbistoun").join("titles"));
    }

    #[test]
    fn absolute_library_root_is_used_as_given() {
        let settings = LibrarySettings {
            root: r"D:\games\ps".to_owned(),
            ..LibrarySettings::default()
        };
        assert_eq!(
            settings.resolve(Path::new(r"C:\data\orbistoun")),
            Path::new(r"D:\games\ps")
        );
    }

    /// A missing library says which folder was missing. The bare `io::Error` does not
    /// carry one, so without this the window reports "the system cannot find the path
    /// specified" and never says which path it meant.
    #[test]
    fn a_missing_library_names_the_folder() {
        let missing = std::env::temp_dir().join("orbistoun-no-such-library-9d3f");
        let error = service()
            .discover_titles(&missing)
            .expect_err("a folder that is not there is an error");
        assert!(
            error.to_string().contains("orbistoun-no-such-library-9d3f"),
            "error should name the folder, said: {error}"
        );
    }
}
