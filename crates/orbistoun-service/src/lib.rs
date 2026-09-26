//! The shared logic layer every shim calls.
//!
//! `orbistoun-cli`, `orbistoun-gui` and worker mode are interaction shims that hold no behaviour
//! (D034); an operation exists here exactly once. Operations take and return owned, serialisable
//! values from `orbistoun-proto`, never types holding references into loaded modules, so the same
//! operation runs in-process for the CLI and across a process boundary for the worker. Here:
//! assembling the module registry, surveying a container, resolving overrides, and turning results
//! into reportable shapes. Not here: presentation, argument parsing or transports.

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
    /// Rendering a result as TOML failed.
    #[error("serialising TOML: {0}")]
    Toml(#[from] toml::ser::Error),
    /// The configuration file exists and could not be read.
    #[error("serialising: {0}")]
    ConfigRead(std::io::Error),
    /// The configuration file was read and is not the format it should be.
    #[error("serialising: {0}")]
    ConfigParse(Box<toml::de::Error>),
    /// The library directory could not be listed.
    #[error("serialising: cannot read library at {}: {source}", path.display())]
    Library {
        /// The library root.
        path: PathBuf,
        /// What the filesystem said.
        source: std::io::Error,
    },
    /// The run-report store could not be read or written.
    #[error("serialising: {0}")]
    Report(orbistoun_report::store::StoreError),
    /// A pad script could not be read.
    #[error("reading input script {}: {source}", path.display())]
    ScriptRead {
        /// The script file.
        path: PathBuf,
        /// What the filesystem said.
        source: std::io::Error,
    },
    /// A pad script is not the format it should be.
    #[error("parsing input script {}: {source}", path.display())]
    ScriptParse {
        /// The script file.
        path: PathBuf,
        /// What the parser said, boxed to keep the error small.
        source: Box<toml::de::Error>,
    },
    /// A pad script parsed but names a run that could not happen.
    #[error("input script {}: {reason}", path.display())]
    ScriptInvalid {
        /// The script file.
        path: PathBuf,
        /// Why the script was refused.
        reason: orbistoun_input::script::ScriptError,
    },
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
    /// What to call this title on screen: the published name when there is one, the directory name
    /// otherwise. Never blank, because a title with unreadable metadata is still a title somebody
    /// can run.
    pub fn display_name(&self) -> &str {
        self.metadata
            .as_ref()
            .map_or(self.name.as_str(), |m| m.title.as_str())
    }
}

/// What a title publishes about itself.
///
/// Read from `sce_sys/param.json` (ordinary JSON) and `sce_sys/icon0.png` (an ordinary PNG) on the
/// user's own machine at run time. The repository holds no title data, and the provenance guard
/// fails the build if it does.
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
    /// It says which era of the interface a title was written against, which predicts how much of
    /// it will be reached.
    pub requires: Option<String>,
    /// The system version it was built against.
    pub built_with: Option<String>,
    /// The icon, if the file exists. A path rather than pixels: decoding belongs to whoever draws,
    /// so a library scan decodes nothing.
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
/// The field is a JSON number holding the version in the top two bytes, each a pair of binary-coded
/// decimal digits: `0x1260…` is 12.60 and `0x0310…` is 3.10. Read as ordinary hex they would be
/// 18.96 and 3.16.
fn decode_system_version(value: &serde_json::Value) -> Option<String> {
    // A number in every file examined, but a string is accepted too.
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

/// One byte of binary-coded decimal. `None` for a byte that is not two decimal digits, rather than
/// a plausible wrong number.
const fn bcd(byte: u8) -> Option<u32> {
    let (high, low) = (byte >> 4, byte & 0xF);
    if high > 9 || low > 9 {
        return None;
    }
    Some((high * 10 + low) as u32)
}

/// Reads what a title says about itself.
///
/// `None` for anything unreadable, unparseable or absent: homebrew and loose dumps have no metadata
/// and are still titles, and every caller has a fallback.
pub fn read_title_metadata(title_dir: &Path) -> Option<TitleMetadata> {
    let system = title_dir.join(TITLE_METADATA_DIR);
    let text = std::fs::read_to_string(system.join(TITLE_METADATA_FILE)).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;

    let localized = value.get("localizedParameters")?;
    // The title names itself in several languages and says which it means by default, which is what
    // the platform picks.
    let language = localized
        .get("defaultLanguage")
        .and_then(serde_json::Value::as_str);
    let title = language
        .and_then(|language| localized.get(language))
        .or_else(|| {
            // No default named, or one that is not present: take any entry that carries a name.
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
/// Not named `_BASE`, because that suffix means an address base and `docs/ADDRESS_MAP.md` is gated
/// against every one (D513); this is an error-code prefix. It is the core's `PLACEHOLDER_BASE`, so
/// a tagged placeholder carries the same high bit as the untagged one and a guest testing `rc < 0`
/// still reads it as failure.
const PLACEHOLDER_PREFIX: u64 = orbistoun_core::PLACEHOLDER_BASE as u64;

/// Where tagged placeholders start, clear of the fixed `GuestError` codes that occupy
/// `PLACEHOLDER_BASE..=PLACEHOLDER_BASE | 0xf`, so a tag is never mistaken for one (D567).
const PLACEHOLDER_TAG_FLOOR: u64 = 0x10;

/// Whether the placeholder-tagging diagnostic is on for this process. Read once, so two sites
/// cannot disagree.
fn tag_placeholders() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| orbistoun_env::TAG_PLACEHOLDERS.is_set())
}

/// The file a title is entered through, named once for every shim (D034).
pub const TITLE_ENTRY_FILE: &str = "eboot.bin";

/// The settings a run reads from disk.
///
/// Each is a hypothesis the guest is the only oracle for, so it changes without a rebuild. The file
/// is `orbistoun-cli paths`' `config` entry. Every field is optional and every section defaults, so
/// a file naming one setting is valid.
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
    /// Here rather than in the shell settings file because it describes this installation's
    /// hardware, which does not travel to another machine (D326).
    pub pads: orbistoun_input::Pads,
    /// What unimplemented functions answer.
    ///
    /// The main lever of the method: the oracle is often one bit per call site (answer `ok`, does
    /// the guest proceed?), so it is set from a file (D166).
    pub policy: StubPolicy,
}

/// Reads and validates the scripted pad a run's controller configuration names, if any.
///
/// [`orbistoun_input::script`] carries the [`Script`](orbistoun_input::script::Script) type and
/// depends on no file format, so turning bytes on disk into a script is this crate's job; it
/// already owns the configuration and parses TOML.
///
/// Returns the first port driven by a script (`None` when none is), as the resolved path and the
/// validated script. A relative path is taken against `base`, the directory the configuration was
/// read from, so `path = "inputs/press-start.toml"` sits beside `config.toml`.
///
/// # Errors
///
/// When the file cannot be read, does not parse as a script, or names a run that could not happen:
/// each fails the run rather than proceeding on an input nobody wrote.
pub fn scripted_pad(
    pads: &orbistoun_input::Pads,
    base: &Path,
) -> Result<Option<(PathBuf, orbistoun_input::script::Script)>, ServiceError> {
    let Some(path) = pads.ports.iter().find_map(|port| match &port.source {
        orbistoun_input::Source::Script { path } => Some(path.as_str()),
        _ => None,
    }) else {
        return Ok(None);
    };
    let resolved = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        base.join(path)
    };
    let script = read_pad_script(&resolved)?;
    Ok(Some((resolved, script)))
}

/// Reads, parses and validates the pad script at `path`, one a run names itself (D721) or one the
/// configuration names.
///
/// # Errors
///
/// When the file cannot be read, does not parse as a script, or names a run that could not happen.
pub fn read_pad_script(path: &Path) -> Result<orbistoun_input::script::Script, ServiceError> {
    let text = std::fs::read_to_string(path).map_err(|source| ServiceError::ScriptRead {
        path: path.to_path_buf(),
        source,
    })?;
    let script: orbistoun_input::script::Script =
        toml::from_str(&text).map_err(|source| ServiceError::ScriptParse {
            path: path.to_path_buf(),
            source: Box::new(source),
        })?;
    script
        .validate()
        .map_err(|reason| ServiceError::ScriptInvalid {
            path: path.to_path_buf(),
            reason,
        })?;
    Ok(script)
}

/// One recorded step, as the `[[step]]` table a script file holds (D721). Appended to a recording
/// as it happens, so the file reads back as a script however the run ended.
#[must_use]
pub fn pad_script_step(step: &orbistoun_input::script::Step) -> String {
    let body = toml::to_string(step).unwrap_or_default();
    format!("\n[[step]]\n{body}")
}

/// Where the library is, and how a run from it behaves.
///
/// Persisted rather than defaulted each launch, because the default is a relative path resolved by
/// [`Self::resolve`] (D038).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct LibrarySettings {
    /// The folder titles are looked for in.
    pub root: String,
    /// Seconds a guest may run before it is stopped. Zero means no limit.
    ///
    /// A backstop for a guest that stops calling imports. It fixes the duration and lets the call
    /// count vary, so a verdict is read off the call budget instead (D238).
    pub run_limit_seconds: u64,
    /// Imports a guest may call before it is stopped. Zero means no budget.
    ///
    /// The deterministic limit: two runs of one build stop at the same call.
    pub run_call_budget: u64,
    /// Which view the window opens into when nothing on the command line says.
    ///
    /// Here rather than in the window so it survives a restart. A command-line flag always beats
    /// it.
    pub start_in: orbistoun_shell::View,
}

impl LibrarySettings {
    /// The folder to actually scan, given where this installation keeps its data.
    ///
    /// A relative root is resolved against the data root, not the working directory, which depends
    /// on how the program was started (D038): beside a portable binary it is
    /// `<binary>/.portable/titles`, installed it is the collection's `titles/`, and with
    /// `ORBISTOUN_DATA_DIR` set it is under that. An absolute root is used as given.
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
            // Relative, and resolved against the data root: see `resolve`.
            root: "titles".to_owned(),
            // No limit: a title launched from a front end is being played, and a clock that ends it
            // reads as a crash. The CLI's own loop sets its limit per run (`ORBISTOUN_LIMIT`).
            run_limit_seconds: 0,
            // No budget either: a title that calls freely would be stopped mid-play.
            run_call_budget: 0,
            // The list view, which is how the emulator is worked on.
            start_in: orbistoun_shell::View::List,
        }
    }
}

impl FileConfig {
    /// Reads settings from `path`. A missing file is the defaults, not an error.
    ///
    /// # Errors
    ///
    /// When the file exists but cannot be read or parsed. Not silent, because a malformed file that
    /// fell back to defaults would look like a setting with no effect.
    pub fn load(path: &Path) -> Result<Self, ServiceError> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(ServiceError::ConfigRead(e)),
        };
        toml::from_str(&text).map_err(|e| ServiceError::ConfigParse(Box::new(e)))
    }

    /// The settings as editable TOML, for writing a starting file.
    ///
    /// # Errors
    ///
    /// When the settings cannot be serialised.
    pub fn to_toml(&self) -> Result<String, ServiceError> {
        Ok(toml::to_string_pretty(self)?)
    }
}

/// How a [`Service`] is set up.
///
/// [`Default`] is the working configuration, not the empty one: a default that cannot resolve an
/// import builds and runs while every lookup silently misses (D082).
#[derive(Debug, Clone)]
pub struct ServiceConfig {
    /// Bytes appended to a symbol name before hashing.
    ///
    /// Defaults to the suffix orbistoun ships with (D071). An empty one hashes to values no real
    /// module imports by.
    pub nid_suffix: Vec<u8>,
    /// Behaviour for functions with no implementation.
    pub stub_policy: StubPolicy,
    /// Where run artifacts are written. `None` disables reporting, for a unit test or a one-shot
    /// inspection.
    pub paths: Option<orbistoun_paths::Paths>,
    /// How guest threads are placed, and what the guest is told about the machine.
    ///
    /// Policy rather than fact, so "how many cores does the guest think it has?" is a file edit and
    /// a relaunch.
    pub thread_settings: orbistoun_kernel::thread::Settings,
    /// Memory behaviour that is a choice rather than a fact: see
    /// [`orbistoun_kernel::direct::Settings`].
    pub memory_settings: orbistoun_kernel::direct::Settings,
    /// How the guest's entry point is presented: what is on its stack when it starts and what is in
    /// its first argument register. Settings rather than constants because only part of it is
    /// established by measurement.
    pub entry_settings: orbistoun_loader::process::EntrySettings,
    /// Optional symbol database, giving names for NIDs the registry does not declare.
    ///
    /// Independent of `nid_suffix`: an import's NID comes from its own symbol name, so surveying
    /// works without either. The database only supplies human-readable names for hashes.
    pub symbol_db: Option<SymbolDbFile>,
    /// The controllers this run presents, and what drives each.
    ///
    /// Carried so the worker can start a script-driven port at guest entry: a script is the one
    /// input source a run with no window can drive itself, sampled as a function of run time
    /// (D707). Live sources reach a run through the GUI streaming them in.
    pub pads: orbistoun_input::Pads,
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
            pads: orbistoun_input::Pads::default(),
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
    pads: orbistoun_input::Pads,
}

/// Where regions handed to a guest by policy start.
///
/// Clear of every address orbistoun places (the loader's module at `0x40…`, the guest stack at
/// `0x60…`, the main-thread TLS block at `0x69…`, the mapping arena at `0x72…`), so a fault in one
/// is unmistakably about this. Also clear of where a guest's own allocator places its heap: a C++
/// allocator can reserve its arena at `0x5000_0000_0000`, so this sits in orbistoun's high cluster,
/// between the TLS block and the mapping arena (D443). Failed reservations at this address are the
/// guest reserving inside the region it was handed, and are expected.
const POLICY_REGION_BASE: u64 = 0x0000_6B00_0000_0000;

/// Reads a container's process parameter block and follows its memory-parameter pointer far enough
/// to report where it leads.
///
/// The block's fields are read at cited offsets ([`orbistoun_elf::procparam`]). Its three pointers
/// are relocated: the file holds zero and a `RELATIVE` relocation supplies `base + addend`, so they
/// are resolved through the data relocation table rather than reported as a misleading zero. The
/// memory-parameter block is surfaced raw (its stated size and non-zero words) because its layout
/// has no citable source.
fn proc_param_info(
    container: &orbistoun_elf::Container<'_>,
    bytes: &[u8],
) -> Result<Option<ProcParamInfo>, ServiceError> {
    /// Most a memory-parameter block is scanned for non-zero words, so a wrong or hostile size
    /// cannot walk the whole file.
    const MEM_PARAM_SCAN_CAP: u64 = 0x100;

    let headers = container.program_headers()?;
    let Some(header) = headers
        .iter()
        .find(|h| h.p_type.get() == orbistoun_elf::segment::SCE_PROCPARAM)
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
/// to the value it writes (`addend`, the target's own virtual address for an executable placed at
/// base zero).
///
/// A container with no dynamic table or no relocation table yields an empty map.
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
            // `addend` is `i64`; an internal data pointer's addend is a non-negative virtual
            // address.
            if let Ok(addend) = u64::try_from(entry.addend.get()) {
                targets.insert(entry.offset.get(), addend);
            }
        }
    }
    Ok(targets)
}

/// Reserves `len` bytes somewhere, bumping past anything already taken.
///
/// `orbistoun_mem::platform::reserve` takes an exact address and fails rather than relocating,
/// which suits placing an image and not "give me somewhere". A handful of attempts is enough: the
/// addresses are ours and nothing else allocates here.
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
            // Leaked deliberately: `Reservation` releases its range on drop, which would unmap the
            // region before the guest sees the base. The region must outlive every guest call into
            // it, and the process ends when the guest does.
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
/// Every table behind a stub (handlers, stub returns, forced returns, call counts, readable and
/// writable ranges) is a process-global `OnceLock` indexed by dynamic symbol index, and a second
/// install is ignored, so all modules share one table (D484). Module M's symbol i is slot `offset +
/// i`. The main executable is module 0 at offset 0, so its call indices are unchanged by loading
/// other modules.
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
/// Pure, so the arithmetic is testable without an ELF, the host address space, or the
/// process-global tables [`Service::build_thunks_for`] can fill only once (D484). The first module,
/// the main executable, gets offset zero.
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
/// `install_policy_writes` and `install_policy_returns` are process-global `OnceLock`s, so
/// installing per module would keep the first module's plants and discard the rest (D484). The
/// reservation cursor lives here too, so two modules are never handed the same addresses.
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
    /// The returns table is installed only when something wants it, because a policy answer uses
    /// the same table and an empty install would claim it for nothing.
    fn install(self) {
        orbistoun_thunk::install_policy_writes(self.writes);
        // Installed through the policy-answer table, because this is a value the function answers
        // with.
        if !self.returns.is_empty() {
            orbistoun_thunk::install_policy_returns(self.returns);
        }
    }
}

/// Where a run puts the title's own modules and the tables that serve them.
///
/// One value because the three are a layout, and three bare addresses can be transposed into a
/// collision that looks like a corrupt image.
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
    /// What linking decided for each of the title's own modules, the executable excluded: the
    /// worker relocates that, and puts its plan first (D724).
    pub plans: Vec<orbistoun_loader::plan::ModulePlan>,
    /// Names more than one module imports as data, which a lookup by name cannot separate.
    pub shared_data: Vec<String>,
    /// The one stub table serving every module, the executable included.
    ///
    /// Returned because the caller relocates the executable against it: a second table would put
    /// the executable's imports in a different index space, and the process-global dispatch tables
    /// would hold whichever was installed first (D484).
    pub thunks: orbistoun_thunk::ThunkTable,
    /// Storage for every data import across every module, in one reservation.
    pub data: orbistoun_thunk::DataBlocks,
    /// The executable's imports that a module the title ships answers, by symbol index.
    ///
    /// Only the ones orbistoun does not implement itself (D483): where this project has written the
    /// function, its implementation keeps the slot.
    pub bound: std::collections::BTreeMap<u32, u64>,
    /// Every import accounted for, bound or not, with a reason for each that is not (D640).
    pub binding: titleplacement::BindingAccount,
    /// One label per slot of the shared stub table, across every module.
    ///
    /// Built here because the module bytes are here. A label vector sized to the executable alone
    /// would name a title module's calls after whatever by-name stub sits at the same index (D490).
    pub labels: Vec<String>,
    /// Imports a module answers that orbistoun implements, so the implementation kept the slot.
    /// Reported so a reader can judge that default for a given title (D483).
    pub kept_by_orbistoun: Vec<String>,
}

impl Service {
    /// Builds a service with every subsystem registered.
    ///
    /// The one place that knows the full module set, so adding a subsystem crate is one line here
    /// plus its own `guest_module!` declaration.
    pub fn new(config: ServiceConfig) -> Self {
        let hasher = NidHasher::new(config.nid_suffix.clone());
        // Applied before anything can spawn, so no thread is placed under a policy the settings do
        // not describe.
        orbistoun_kernel::thread::configure(config.thread_settings);
        orbistoun_kernel::direct::configure(config.memory_settings);

        let mut registry = Registry::new(hasher.clone(), config.stub_policy.clone());
        // Driven from the one module list (D123).
        for module in symbols::modules() {
            registry.register(module);
        }
        // The database carries its own suffix, since names must be hashed with the suffix they were
        // derived under.
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
            pads: config.pads,
        }
    }

    /// Surveys a container and emits a run report, diffed against the previous run of the same
    /// title.
    ///
    /// The operation the iterative loop uses: it answers "what does this need" and "did the last
    /// change help" in one call, and persists the answer for the next run to compare against.
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

    /// What unimplemented functions answer, how many symbols were given a specific answer, and how
    /// many of those rest on nothing measured.
    ///
    /// Exposed so a run records what it was subject to (D181). The count is of symbols, not
    /// answers, so a policy that plants a region counts; the third number separates a hardware
    /// measurement from a guess.
    pub fn policy_summary(&self) -> (String, usize, usize) {
        let default = match self.policy.default_return {
            orbistoun_hle::StubReturn::Ok => "ok".to_owned(),
            orbistoun_hle::StubReturn::Unimplemented => "unimplemented".to_owned(),
            orbistoun_hle::StubReturn::Raw(v) => format!("{v:#x}"),
        };
        (default, self.policy.specific(), self.policy.propping())
    }

    /// Every name the loaded database holds: the seeds for a search that derives new names from
    /// proved ones (D606).
    pub fn symbol_names(&self) -> impl Iterator<Item = &str> {
        self.symbols.iter().flat_map(SymbolDb::names)
    }
    /// The name the loaded database gives a hash, if it gives one. Asked of the service so a shim
    /// need not know how a database is loaded or which suffix built it.
    pub fn symbol_name(&self, nid: Nid) -> Option<&str> {
        self.symbols.as_ref().and_then(|db| db.name(nid))
    }
    /// How many names the loaded symbol database knows, if there is one.
    pub fn symbol_db_len(&self) -> Option<usize> {
        self.symbols.as_ref().map(SymbolDb::len)
    }

    /// How many of a container's imports a loaded database can name: a name list and suffix are
    /// correct exactly to the extent they explain hashes a real module imports.
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
    /// False when no hash suffix was supplied: names remain correct, hashes do not. Shims surface
    /// this.
    pub const fn nids_are_real(&self) -> bool {
        self.suffix_len > 0
    }

    /// How many functions are declared across all subsystems.
    pub fn declared_count(&self) -> usize {
        self.registry.len()
    }

    /// Every declared function, sorted: reports are diffed between runs, and ordering churn would
    /// read as change.
    pub fn declared_symbols(&self) -> Vec<DeclaredSymbol> {
        symbols::all(&self.hasher)
    }

    /// How the guest's entry point is presented.
    pub const fn entry_settings(&self) -> &orbistoun_loader::process::EntrySettings {
        &self.entry_settings
    }

    /// The controllers this run presents, and what drives each.
    pub const fn pads(&self) -> &orbistoun_input::Pads {
        &self.pads
    }

    /// Every runnable title under `root`.
    ///
    /// A directory containing the entry file. Sorted, so the library keeps its order between runs.
    ///
    /// # Errors
    ///
    /// When `root` cannot be read. A directory that cannot be inspected is skipped rather than
    /// failing the whole scan.
    pub fn discover_titles(&self, root: &Path) -> Result<Vec<TitleEntry>, ServiceError> {
        let entries = std::fs::read_dir(root).map_err(|e| {
            // With the path in it: `io::Error` carries none, and the bare message does not say
            // which folder.
            ServiceError::Library {
                path: root.to_path_buf(),
                source: e,
            }
        })?;
        // Staged titles too, and they win. The library's `data/homebrew` tree holds titles staged
        // as the hardware stages homebrew (D722); a title also present as an image is the same
        // title, and the staged copy is the one whose storage is writable.
        let staged = std::fs::read_dir(orbistoun_paths::staged_under(root))
            .into_iter()
            .flatten();
        let mut by_name = std::collections::BTreeMap::new();
        for entry in entries.flatten().chain(staged.flatten()) {
            let directory = entry.path();
            let module = directory.join(TITLE_ENTRY_FILE);
            if module.is_file() {
                let name = entry.file_name().to_string_lossy().into_owned();
                by_name.insert(
                    name.clone(),
                    TitleEntry {
                        name,
                        module,
                        metadata: read_title_metadata(&directory),
                    },
                );
            }
        }
        let mut found: Vec<TitleEntry> = by_name.into_values().collect();
        found.sort();
        Ok(found)
    }

    /// The default stub policy as editable TOML.
    pub fn default_policy_toml(&self) -> Result<String, ServiceError> {
        Ok(toml::to_string_pretty(&self.policy)?)
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
    /// `base` is added to each segment's virtual address: a module links at zero and must be placed
    /// somewhere, while an executable carries absolute addresses and wants a base of zero.
    ///
    /// A module is reserved as one contiguous span. The host reserves at 64 KiB granularity, so
    /// per-segment reservations of segments pages apart would collide with each other; and segment
    /// addresses are not page-aligned (`0x147f0`), so the span is rounded outwards at both ends.
    /// Reservations are released when the returned layout is dropped: this answers "can this be
    /// placed here", not "place it permanently".
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

        // Round outwards: a segment starting or ending mid-page still owns that page.
        let span_base = lowest / page * page;
        let span_end = highest.div_ceil(page).saturating_mul(page);
        let span_len = span_end.saturating_sub(span_base);

        let mut space = orbistoun_mem::AddressSpace::new();
        let reservation_failure = space
            .reserve(
                span_base,
                span_len,
                // Reserved read-write so it can be populated; per-segment protection is the
                // loader's job.
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

    /// Places a container in memory: reserves its span and copies every loadable segment, zeroing
    /// `.bss`.
    ///
    /// The image holds its own reservation, so dropping it releases the memory. This is loading up
    /// to but not including relocation.
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
    /// The reservation happens here, not in the thunk: a trampoline runs on the guest's stack and
    /// may not allocate, and this layer already builds the address space (D300).
    ///
    /// # Errors
    ///
    /// If the container cannot be re-read. A region that cannot be reserved is skipped and reported
    /// rather than failing the run.
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
                tracing::warn!(
                    "{} asks for {:#x} bytes and none were free - it will answer without one",
                    resolved.name,
                    plan.bytes
                );
                continue;
            };
            let slot = offset.saturating_add(import.symbol_index as usize);
            // The region is the same either way; only whether the guest reads the base from an
            // argument it passed or from its answer register differs (D300).
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
    /// Separate from [`Self::build_thunks`]: a stub says which function was wanted, and a block
    /// says the guest wanted an object, which no stub can be (D307).
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
        // Names as well as indices: relocation needs the index, and an implementation that owns a
        // guest global (`getopt` writing `optarg`) needs the name.
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
    /// A vendor module spells every import as an encoded hash, which is what a run report would
    /// print beside a faulting register; a name says whether the object is a vtable, a stdio handle
    /// or the stack canary. The run's database is used, not the service's: a worker builds its
    /// `Service` with none, because names belong to the run. Falls back to the encoded spelling,
    /// which is still greppable; it never invents a name.
    fn spell_data_import(
        db: Option<&SymbolDb>,
        import: &orbistoun_elf::dynamic::RawImport,
    ) -> String {
        db.and_then(|db| db.name(Nid::from_raw(import.nid)))
            .map_or_else(|| import.name.clone(), ToOwned::to_owned)
    }

    /// Storage for every data import across several modules, in one reservation.
    ///
    /// Indexed like the shared stub table (D484): module M's symbol i is index `offset(M) + i`. The
    /// named map merges as a consequence, which `install_data_symbols` (a `OnceLock`) needs. A name
    /// two modules both import as data gets separate storage and is reported, since a lookup by
    /// name can be handed only one.
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
    /// Sized from the symbol table rather than the import list, because relocations index the
    /// symbol table.
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
    /// `modules[0]` is the main executable and takes offset zero, so a single-module call matches
    /// [`Self::build_thunks`]. Every process-global table (`install_handlers`,
    /// `install_stub_returns`, `install_policy_writes`) is a `OnceLock` and is installed once,
    /// after every module has contributed (D484).
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
        // Stubs past the guest's own symbols, one per implemented name, for names a guest resolves
        // at run time instead of importing (D365).
        let resolvable = symbols::resolvable();
        let total = imports + resolvable.len();
        let table = orbistoun_thunk::ThunkTable::build_with_named(
            base,
            imports,
            resolvable.len(),
            orbistoun_core::GUEST_PAGE_SIZE,
        )?;

        // Bind implementations to the slots the guest will call; the registry alone is not
        // consulted at call time (D082).
        let mut handlers: Vec<Option<orbistoun_core::GuestFn>> = vec![None; total];
        // The floating-point table, bound in the same pass and disjoint from the integer one
        // (D268).
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
            // Not fatal: a report that names the functions is worth more than a refusal.
            tracing::warn!(
                "{} implemented functions could not be bound to a stub slot and will answer a placeholder: {}",
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
    /// `offset` is where the module's symbol zero sits, so `symbol_index` becomes a slot only after
    /// it is added. The two tables travel as a pair because a binding goes into exactly one of them
    /// and the choice is made here.
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
            // A binding that does not fit is reported. Both tables are sized from the dynamic
            // symbol count that `symbol_index` indexes, so an out-of-range slot means the two
            // disagree, and the function would otherwise answer a placeholder all run,
            // indistinguishable from one nobody wrote.
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

    /// What each of this module's unimplemented stubs should answer.
    ///
    /// Three sources, in this order (D166):
    ///
    /// 1. An explicit per-symbol override from the policy file: a deliberate experiment wins over
    ///    everything.
    /// 2. The knowledge file's declared return kind. A pointer-, handle- or count-returning
    ///    function answers zero, because an error code in a pointer register is a wild pointer.
    ///    This beats the policy default so a blanket "answer ok" cannot reintroduce that.
    /// 3. The policy default, for everything else.
    fn fill_stub_returns(
        &self,
        container: &orbistoun_elf::Container<'_>,
        bytes: &[u8],
        offset: usize,
        knowledge: &orbistoun_hle::knowledge::Knowledge,
        stub_returns: &mut [Option<u64>],
    ) -> Result<(), ServiceError> {
        for import in container.raw_imports(bytes, &self.hasher)? {
            // An undeclared import still gets the policy: those are the imports worth asking "does
            // it proceed if this succeeds?" about, since a declared one is usually implemented.
            let resolved = self.registry.resolve(Nid::from_raw(import.nid));
            let declared = resolved
                .and_then(|r| knowledge.get(r.name))
                .and_then(|k| k.returns)
                .and_then(orbistoun_hle::knowledge::Returns::stub_value);
            // Keyed by name where there is one, and by hash where there is not, so an unnamed
            // function can be the subject of a single-symbol experiment.
            let by_name = resolved.and_then(|r| self.policy.overrides.get(r.name));
            let by_nid = self
                .policy
                .overrides
                .get(&format!("{:#018x}", import.nid))
                .or_else(|| self.policy.overrides.get(&format!("{:016x}", import.nid)));
            let overridden = by_name.or(by_nid).map(|r| u64::from(r.as_raw()));
            // Under `ORBISTOUN_TAG_PLACEHOLDERS` each stub answers `PLACEHOLDER_PREFIX | (0x10 +
            // its slot)`, so a placeholder found in a guest argument names its source (D567). It
            // keeps the high bit, so a guest's `rc < 0` still catches it, and starts above the
            // fixed `GuestError` range.
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
                        // The ordinary error code is the dispatcher's fallback, so leaving it unset
                        // keeps one path.
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

    /// Every import hash this build cannot put a name to: the input to a name search.
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

    /// Whether anything can put a name to this hash right now.
    ///
    /// Both sources: the registry holds what a `guest_module!` declares, and the symbol database
    /// holds what a search worked out. The one definition of "named", shared by the work list.
    #[must_use]
    pub fn is_named(&self, nid: Nid) -> bool {
        self.is_named_with(nid, self.symbols.as_ref())
    }

    /// Whether anything at all can put a name to this hash right now, given a symbol database.
    #[must_use]
    pub fn is_named_with(&self, nid: Nid, symbols: Option<&SymbolDb>) -> bool {
        self.registry.resolve(nid).is_some() || symbols.is_some_and(|db| db.name(nid).is_some())
    }

    /// Builds a label per dynamic symbol, for attributing a call trace.
    ///
    /// Indexed by symbol index so it lines up with the stub table. Symbols that are not imports get
    /// an empty label rather than being omitted, which would shift every later index.
    pub fn import_labels(&self, bytes: &[u8]) -> Result<Vec<String>, ServiceError> {
        self.labels(bytes, self.symbols.as_ref())
    }

    /// Builds labels using a database supplied for this call. A worker is handed a database per
    /// request, because names belong to the run.
    pub fn import_labels_with(
        &self,
        bytes: &[u8],
        file: &SymbolDbFile,
    ) -> Result<Vec<String>, ServiceError> {
        let db = SymbolDb::from_file(file).map(|(db, _)| db);
        self.labels(bytes, db.as_ref())
    }

    /// Where each implementation starts, by name.
    ///
    /// A `GuestFn` is a function pointer, so this table knows where every implementation lives,
    /// which a fault report needs on a toolchain whose binaries carry no symbols (D380).
    #[must_use]
    pub fn implementation_addresses(&self) -> Vec<(u64, &'static str)> {
        symbols::implementations()
            .into_iter()
            .map(|(name, function)| (function as *const () as usize as u64, name))
            .collect()
    }

    /// Binds the stubs a guest may reach by name, and publishes both ways of reaching them.
    fn bind_by_name(
        resolvable: &[(&'static str, symbols::Resolvable)],
        imports: usize,
        table: &orbistoun_thunk::ThunkTable,
        handlers: &mut [Option<orbistoun_core::GuestFn>],
        float_handlers: &mut [Option<orbistoun_core::GuestFloatFn>],
    ) {
        // Bound and published together, so a name is never advertised without a handler.
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
        // Which of those names libkernel declares, so `sceKernelDlsym` can narrow an answer by the
        // module handle it was given. Published here because the platform's libkernel spans three
        // declarations in crates that must not reach sideways (D536).
        orbistoun_thunk::install_libkernel_names(
            symbols::modules()
                .iter()
                .filter(|m| m.name.starts_with("libkernel"))
                .flat_map(|m| m.imports.iter().map(|i| i.name.to_owned()))
                .collect(),
        );
        // What a guest reaches past every name: the syscall table, published beside the by-name
        // stubs (D378).
        orbistoun_thunk::syscall::install_syscalls(symbols::syscalls(), symbols::syscall_refusal());
    }

    /// The shared body of both label builders, so the two cannot describe an import differently.
    fn labels(&self, bytes: &[u8], db: Option<&SymbolDb>) -> Result<Vec<String>, ServiceError> {
        let container = orbistoun_elf::Container::parse(bytes)?;
        let count = usize::try_from(container.symbol_count(bytes)?).unwrap_or(0);
        let mut labels = Self::resolvable_labels(count);
        self.fill_labels(&container, bytes, 0, db, &mut labels)?;
        Ok(labels)
    }

    /// Labels for every module sharing one stub table, in that table's own index space.
    ///
    /// With one table across several modules, the by-name stubs follow every module's
    /// imports, not only the executable's; otherwise a title module's calls would be labelled with
    /// by-name stubs' names (D490).
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

    /// An empty label per import slot, then one per by-name stub, after every module's imports.
    fn resolvable_labels(imports: usize) -> Vec<String> {
        let mut labels = vec![String::new(); imports];
        // The by-name stubs, labelled from the list that binds them, so a call resolved at run time
        // reads as itself in a trace (D366). The library comes from the knowledge base, because
        // these have no import entry.
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
                // Then whatever the symbol database can name.
                None => match db.and_then(|db| db.name(nid)) {
                    Some(name) => format!("{library}::{name}"),
                    // No name known, so the hash: stable across builds and searchable, and the
                    // library says which subsystem to look in.
                    None => format!("{library}::{nid}"),
                },
            };
        }
        Ok(())
    }

    /// Applies relocations to a placed image.
    ///
    /// `resolver` decides what each import becomes, in practice a thunk table, so every import
    /// lands on a stub that records which function the guest wanted and returns an explicit "not
    /// implemented".
    pub fn relocate_image(
        &self,
        image: &orbistoun_loader::Image,
        bytes: &[u8],
        resolver: &impl orbistoun_loader::relocate::SymbolResolver,
    ) -> Result<orbistoun_elf::reloc::RelocationTally, ServiceError> {
        self.relocate_image_recorded(image, bytes, resolver)
            .map(|applied| applied.tally)
    }

    /// [`Self::relocate_image`], also returning every value written, for the link plan (D724).
    pub fn relocate_image_recorded(
        &self,
        image: &orbistoun_loader::Image,
        bytes: &[u8],
        resolver: &impl orbistoun_loader::relocate::SymbolResolver,
    ) -> Result<orbistoun_loader::relocate::Applied, ServiceError> {
        // The module's own thread-local layout, read from the container so a caller cannot pair an
        // image with another module's layout.
        let tls = orbistoun_loader::tls::layout_of(bytes)?.map(|(layout, _, _)| layout);
        Ok(orbistoun_loader::relocate::apply_recorded(
            image,
            bytes,
            resolver,
            tls.as_ref(),
        )?)
    }

    /// Applies each segment's declared access to a placed, relocated image. Separate from
    /// placement: an image is populated writable and only then made executable.
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
                    // A declared symbol names itself; otherwise the database does. `known` stays
                    // about whether orbistoun implements it, never whether it can be spelled.
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
    /// The executable names libraries and not where they live (D482), so the names from its vendor
    /// tables are looked for in the title's own tree. A platform library is absent from the result.
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
    /// Every module is placed before any export is indexed (D482), so the result does not depend on
    /// the order the filesystem offered them. The result is placed, not relocated, and is not bound
    /// into a running guest: see [`titleplacement`].
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

    /// What a guest module imports, as the loader reads it: resolving an import against a placed
    /// module matches the hash and the kind of each side.
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
    /// Every module is placed and every export collected before any is relocated (D482), because a
    /// title module may import from another. The executable is module 0 at slot zero and is not
    /// relocated here: entering the guest is the worker's sequence.
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
        // The run's own database, built once: the data blocks are labelled with it and a worker's
        // `Service` carries none.
        let named_by = SymbolDb::from_file(symbols).map(|(db, _)| db);
        let (data, shared_data) =
            self.build_data_blocks_for(&modules, &slots, bases.data, named_by.as_ref())?;
        // Published once across every module: the table is a `OnceLock` and a second install is
        // ignored (D484).
        orbistoun_thunk::install_data_symbols(data.named());
        // The address-keyed twin: a name several modules import labels several pages, and a fault
        // reporter needs each.
        orbistoun_thunk::install_data_labels(data.labels());

        // Before the modules are relocated: the binding says which imports resolve into a title
        // module rather than a stub, and a relocation that has run cannot be told (D640).
        let (per_module, kept_by_orbistoun, binding) =
            self.bind_to_title_modules(&placed, &modules, &slots)?;
        let bound = per_module.first().cloned().unwrap_or_default();

        let mut tallies = Vec::new();
        let mut plans = Vec::new();
        // `slots[0]` is the executable, which the worker relocates as part of entering it.
        for (index, ((library, image), slot)) in
            placed.images().iter().zip(slots.iter().skip(1)).enumerate()
        {
            let bytes = owned
                .iter()
                .find(|(label, _)| label == library)
                .map(|(_, bytes)| bytes.as_slice())
                .unwrap_or_default();
            // Built per module: the borrow it takes must end before the tables move into the
            // returned value.
            let shared = orbistoun_loader::relocate::ImportResolver {
                thunks: &thunks,
                data: &data,
                refuse: None,
                weak_zero: None,
            };
            let shifted = orbistoun_loader::relocate::OffsetResolver {
                offset: slot.offset,
                inner: &shared,
            };
            // The binding outside, the offsetting inside. `TitleResolver` looks up this module's
            // own symbol index, and `OffsetResolver` shifts only what falls through to the shared
            // stub table.
            let nothing = std::collections::BTreeMap::new();
            let resolver = orbistoun_loader::relocate::TitleResolver {
                bound: per_module.get(index + 1).unwrap_or(&nothing),
                inner: &shifted,
            };
            let applied = self.relocate_image_recorded(image, bytes, &resolver)?;
            plans.push(orbistoun_loader::plan::ModulePlan::of(
                library,
                image,
                applied.writes,
            ));
            tallies.push((library.clone(), applied.tally));
        }
        // Protected only now: relocation writes into text, so protecting earlier would fault those
        // writes, and an unprotected module faults on instruction fetch (D489).
        for (_, image) in placed.images_mut() {
            self.protect_image(image)?;
        }
        // Tell the kernel where the modules are. Their pages are in this loader's own address
        // space, which the kernel's runtime map never sees (D446); without this a fault inside one
        // reads as outside every placed region, and `sceKernelVirtualQuery` refuses an address the
        // guest runs from.
        for (_, image) in placed.images() {
            let (base, len) = image.span();
            orbistoun_kernel::note_region(base, len);
        }
        // Record where each module's initialisers are, so a later `sceKernelLoadStartModule` can
        // run them. Only this place holds both the bytes (`DT_INIT_ARRAY`, link-relative) and the
        // placed image. The array's contents are pointers relocation writes, so they are read at
        // start time (D515).
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
        // Record what they export, so `sceKernelDlsym` can answer for a symbol the title's own
        // binaries provide. Addresses carry the module base; NIDs are what an importer, or a
        // `dlsym` name, hashes to (D517).
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
            plans,
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
    /// Every import is accounted for, with a reason for each that is not bound (D640). Each reason
    /// points at a different fix: an unlisted library id is a parsing question, a library nothing
    /// exports under is placement, a hash a placed module does not export is about that module, and
    /// `KeptByOrbistoun` is not a failure.
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
        // One map per module, keyed by that module's own symbol index, so the binding resolver
        // never sees a shifted index and neither resolver needs to know what the other does.
        let mut per_module: Vec<std::collections::BTreeMap<u32, u64>> = Vec::new();
        let mut kept = Vec::new();
        let mut total = 0_usize;
        let mut answered: std::collections::BTreeMap<String, usize> =
            std::collections::BTreeMap::new();
        // Keyed by reason and library, so a reader can see whether a given module is in a bucket.
        let mut why: Vec<(Unbound, String, usize)> = Vec::new();

        // Every module the title ships, not only the executable: a module can import from a sibling
        // module (D640). The slot offset is still needed to ask whether this project implements the
        // import, a question for the shared dispatch table.
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
                    // Asked of the dispatch tables, which answer the call: a name known but not
                    // bound is not implemented.
                    if orbistoun_thunk::is_implemented(at) {
                        kept.push(import.name.clone());
                        // Named with its library: kept by orbistoun is correct for libc and
                        // suspicious for a title's own module.
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
    /// Re-walks the same tests rather than threading a reason through `resolve`, so the common path
    /// does not carry the cost of the rare one. It runs once per import per launch.
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
                // The library, not the import: which libraries the unbound imports name says
                // whether anything is wrong.
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

    /// The hash a guest would import this name by, from the service's own hasher: the suffix is a
    /// run input, and a default would answer a different question.
    #[must_use]
    pub fn hash_name(&self, name: &str) -> Nid {
        self.hasher.hash(name)
    }

    /// The vendor tables an encoded import name indexes: libraries, then modules.
    ///
    /// The two are different lists and neither is `DT_NEEDED`. Exposed so what the strings contain,
    /// a bare name or a path, is read rather than assumed.
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

    /// What a binary exports, as `(nid, runtime address)` once placed at `base`.
    ///
    /// For `sceKernelDlsym`, which hashes the name at the call and looks it up here (D517).
    /// Separate from [`Self::exports_bytes`], which names symbols for a human; this carries no
    /// names.
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

    /// The hash suffix this service is configured with, handed to the kernel with the exports
    /// because the hashing happens where the name arrives.
    #[must_use]
    pub fn nid_suffix(&self) -> &[u8] {
        self.hasher.suffix_bytes()
    }

    /// What a module provides, read without executing it.
    ///
    /// The other half of [`Self::survey_bytes`]: a survey answers "what does this need", this
    /// answers "what can it answer for somebody else".
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
                // Named from the database where the module spelled a bare NID, as a survey names an
                // import.
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
        if let Some(hex) = symbol.strip_prefix("0x") {
            if let Ok(raw) = u64::from_str_radix(hex, 16) {
                return Nid::from_raw(raw);
            }
        }
        self.hasher.hash(symbol)
    }
}

/// Where a placed module's initialisers live, as runtime addresses.
///
/// [`None`] when the module has no dynamic table, or no `DT_INIT` and an empty `DT_INIT_ARRAY`: a
/// module with nothing to run records no start. `DT_INIT_ARRAY` is a link-relative address and a
/// title's module links at zero, so the runtime address is `base + vaddr`; a module linked
/// elsewhere would need its load bias instead.
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
    use super::{LibrarySettings, Path, Service, ServiceConfig, scripted_pad, slot_ranges};
    use orbistoun_input::{Pads, Source};

    /// A `Pads` with one port driven by a script at `name`, for exercising [`scripted_pad`].
    fn pads_playing(name: &str) -> Pads {
        let mut pads = Pads::default();
        pads.ports[0].source = Source::Script {
            path: name.to_owned(),
        };
        pads
    }

    /// A run with no scripted port installs nothing.
    #[test]
    fn no_scripted_port_is_no_script() {
        let found = scripted_pad(&Pads::default(), Path::new(".")).expect("the default is fine");
        assert!(found.is_none(), "a keyboard-only run names no script");
    }

    /// A named script is read relative to the configuration and validated before the run.
    #[test]
    fn a_named_script_is_read_relative_to_the_config() {
        let dir = tempfile::tempdir().expect("a temp config directory");
        std::fs::write(
            dir.path().join("press-start.toml"),
            "[[step]]\nat_ms = 500\nbuttons = [\"start\"]\n\n[[step]]\nat_ms = 700\nbuttons = []\n",
        )
        .expect("the script file is written");

        let (resolved, script) = scripted_pad(&pads_playing("press-start.toml"), dir.path())
            .expect("a valid script loads")
            .expect("the scripted port is found");
        assert_eq!(resolved, dir.path().join("press-start.toml"));
        assert_eq!(script.len(), 2, "both steps are read");
    }

    /// A script file that is not there fails the run.
    #[test]
    fn a_missing_script_file_fails_the_run() {
        let dir = tempfile::tempdir().expect("a temp config directory");
        let why = scripted_pad(&pads_playing("absent.toml"), dir.path())
            .expect_err("a missing file is refused");
        assert!(
            why.to_string().contains("absent.toml"),
            "the error names the file: {why}"
        );
    }

    /// A script that does not describe a possible run fails the run, carrying
    /// [`orbistoun_input::script`]'s own refusal.
    #[test]
    fn an_invalid_script_fails_the_run() {
        let dir = tempfile::tempdir().expect("a temp config directory");
        std::fs::write(
            dir.path().join("backwards.toml"),
            "[[step]]\nat_ms = 700\n\n[[step]]\nat_ms = 500\n",
        )
        .expect("the script file is written");
        let why = scripted_pad(&pads_playing("backwards.toml"), dir.path())
            .expect_err("steps out of order are refused");
        assert!(
            why.to_string().contains("backwards.toml") && why.to_string().contains("after"),
            "the error names the file and the reason: {why}"
        );
    }

    /// The main executable owns slot zero, whatever else is loaded beside it.
    #[test]
    fn the_main_executable_owns_slot_zero() {
        let ranges = slot_ranges(&[("eboot", 583), ("Il2CppUserAssemblies", 247)]);
        assert_eq!(ranges[0].offset, 0);
        assert_eq!(ranges[0].count, 583);
    }

    /// A second module starts exactly where the first ended: an overlap would bind two modules'
    /// symbols to one stub.
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

    /// Every subsystem is registered.
    #[test]
    fn every_subsystem_is_registered() {
        // A subsystem added to the workspace but not wired into `Service::new` would be invisible
        // in every shim.
        let s = service();
        assert_eq!(s.declared_count(), s.declared_symbols().len());
        assert!(s.declared_count() >= 37, "expected the declared surface");
    }

    /// Declared symbols cover every library.
    #[test]
    fn declared_symbols_cover_every_library() {
        let s = service();
        let libs: std::collections::BTreeSet<_> = s
            .declared_symbols()
            .into_iter()
            .map(|d| d.library)
            .collect();
        // Asserted against the module list rather than a literal.
        assert_eq!(libs.len(), super::symbols::modules().len(), "got {libs:?}");
    }

    /// Declared symbols are deterministically ordered.
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

    /// A missing hash suffix is reported rather than hidden.
    #[test]
    fn a_missing_suffix_is_reported_rather_than_hidden() {
        // Without a suffix the names are right and the hashes meaningless, which must be visible. A
        // missing suffix is asked for explicitly, because the default is the working configuration
        // (D082).
        let with = service();
        assert!(with.nids_are_real());
        let without = Service::new(ServiceConfig {
            nid_suffix: Vec::new(),
            ..ServiceConfig::default()
        });
        assert!(!without.nids_are_real());
    }

    /// The default configuration resolves something.
    #[test]
    fn the_default_configuration_can_actually_resolve_something() {
        // A default that resolved nothing would make every implementation unreachable.
        let service = Service::new(ServiceConfig::default());
        assert!(
            service.nids_are_real(),
            "the default must be the working configuration"
        );
    }

    /// Titles are directories holding an entry file.
    #[test]
    fn titles_are_directories_holding_an_entry_file() {
        // A title is a directory holding the entry file.
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

        // Sorted, so the library keeps its order between runs.
        assert_eq!(names, vec!["AAAA-app0", "BBBB-app0"]);
        assert!(found[0].module.ends_with(super::TITLE_ENTRY_FILE));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A title is named by what it publishes, falling back to its folder.
    #[test]
    fn a_title_is_named_by_what_it_publishes_and_falls_back_to_its_folder() {
        // `display_name` never answers a blank.
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

        // Homebrew and loose dumps carry no metadata and are still titles.
        assert!(found[1].metadata.is_none());
        assert_eq!(found[1].display_name(), "BBBB-app0");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// A packed system version reads as decimal digits, not hex.
    #[test]
    fn a_packed_system_version_reads_as_decimal_digits_not_as_hex() {
        // Binary-coded decimal: 0x1260 is 12.60, not the 18.96 of ordinary hex.
        let packed = |v: u64| super::decode_system_version(&serde_json::Value::from(v));
        assert_eq!(packed(0x1260_0000_0000_0000).as_deref(), Some("12.60"));
        assert_eq!(packed(0x0310_0000_0000_0000).as_deref(), Some("3.10"));
        assert_eq!(packed(0x0100_0000_0000_0000).as_deref(), Some("1.00"));
        assert_eq!(packed(0x1001_0000_0000_0000).as_deref(), Some("10.01"));

        // A byte that is not two decimal digits is refused.
        assert_eq!(packed(0x1A00_0000_0000_0000), None);
    }

    /// A title naming a language it does not carry still gets a name.
    #[test]
    fn a_title_naming_a_language_it_does_not_carry_still_gets_a_name() {
        // A title naming itself in another language still gets a name.
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

    /// Unreadable metadata costs a label and nothing else.
    #[test]
    fn unreadable_metadata_costs_a_label_and_nothing_else() {
        // Malformed JSON does not take the title out of the library.
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

    /// A missing library folder is an error rather than an empty library.
    #[test]
    fn a_missing_library_folder_is_an_error_rather_than_an_empty_library() {
        // An empty list would say "you own no titles", not "that folder does not exist".
        let missing = std::env::temp_dir().join("orbistoun-no-such-library");
        let _ = std::fs::remove_dir_all(&missing);
        assert!(service().discover_titles(&missing).is_err());
    }

    /// A missing configuration file is the defaults.
    #[test]
    fn a_missing_configuration_file_is_the_defaults_rather_than_an_error() {
        // The ordinary case needs no file.
        let missing = Path::new("no-such-file-anywhere.toml");
        let loaded = super::FileConfig::load(missing).expect("a missing file is fine");
        assert_eq!(
            loaded.entry,
            orbistoun_loader::process::EntrySettings::default()
        );
    }

    /// A malformed configuration file fails rather than falling back.
    #[test]
    fn a_malformed_configuration_file_fails_rather_than_falling_back() {
        // A typo in a setting must not silently revert to the default and read as "no effect".
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

    /// The library folder survives a restart.
    #[test]
    fn the_library_folder_survives_a_restart() {
        // The starting value is relative, so it has to persist to mean the same folder every
        // launch.
        let mut chosen = super::FileConfig::default();
        chosen.library.root = r"D:\games\ps".to_owned();
        chosen.library.run_limit_seconds = 45;

        let text = chosen.to_toml().expect("serialises");
        let back: super::FileConfig = toml::from_str(&text).expect("reads back");
        assert_eq!(back.library.root, r"D:\games\ps");
        assert_eq!(back.library.run_limit_seconds, 45);
    }

    /// A configuration naming one setting is valid.
    #[test]
    fn a_configuration_naming_one_setting_is_valid() {
        // Every field defaults, so a file can say the one thing being tried.
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

    /// The default policy serialises to editable TOML.
    #[test]
    fn the_default_policy_serialises_to_editable_toml() {
        let toml = service().default_policy_toml().expect("serialises");
        assert!(toml.contains("default_return"), "got {toml}");
        assert!(
            toml.contains("unimplemented"),
            "loud by default, not Ok: {toml}"
        );
    }

    /// Surveying a non-container fails.
    #[test]
    fn surveying_a_non_container_fails_honestly() {
        // Not an empty result: an empty import list reads as "needs nothing" (D010).
        let s = service();
        let err = s.survey_bytes(&[0_u8; 64]).expect_err("not a container");
        assert!(matches!(err, super::ServiceError::Survey(_)));
    }

    /// Surveying a missing path names the path.
    #[test]
    fn surveying_a_missing_path_names_the_path() {
        let s = service();
        let err = s
            .survey_path(Path::new("no/such/file.bin"))
            .expect_err("missing");
        assert!(err.to_string().contains("no/such/file.bin"), "got {err}");
    }

    /// Reporting is optional and off by default.
    #[test]
    fn reporting_is_optional_and_off_by_default() {
        // A unit test or one-shot inspection writes nothing to a user's disk.
        assert!(ServiceConfig::default().paths.is_none());
    }

    /// A first run reports no diff.
    #[test]
    fn a_first_run_reports_no_diff_and_that_is_information_not_failure() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let file = tmp.path().join("guest.bin");
        std::fs::write(&file, [0_u8; 64]).expect("write");

        let s = service();
        // Not a container, so surveying fails rather than reporting nothing.
        assert!(s.survey_and_report(&file, 1_700_000_000_000).is_err());
    }

    /// The title hash identifies content, not location.
    #[test]
    fn the_title_hash_identifies_content_not_location() {
        // Two paths, same bytes: the same title, so a moved file keeps its history.
        let a = super::content_hash(b"identical");
        let b = super::content_hash(b"identical");
        assert_eq!(a, b);
        assert_ne!(a, super::content_hash(b"different"));
    }

    /// NID lookup matches the registry.
    #[test]
    fn nid_lookup_matches_the_registry() {
        let s = service();
        // A declared symbol resolves through the same hasher the registry used.
        let declared = s.declared_symbols();
        let first = &declared[0];
        assert_eq!(s.nid_for(&first.symbol).as_raw(), first.nid);
    }
    /// A relative library root is taken from the data root.
    #[test]
    fn relative_library_root_is_taken_from_the_data_root() {
        let settings = LibrarySettings::default();
        let resolved = settings.resolve(Path::new(r"C:\data\orbistoun"));
        assert_eq!(resolved, Path::new(r"C:\data\orbistoun").join("titles"));
    }

    /// An absolute library root is used as given.
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

    /// A missing library says which folder was missing.
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
