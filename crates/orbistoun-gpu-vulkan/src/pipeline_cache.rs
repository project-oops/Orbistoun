//! Compiled pipelines kept per title across runs, one checked entry a pipeline (D994, amending
//! D744).
//!
//! A translated module is megabytes of SPIR-V and the driver takes seconds to compile one, so what
//! it made of each is kept for the next run. Each pipeline is built through a pipeline cache of its
//! own, and that cache's data is kept as one entry: a file named by the pipeline's key - its SPIR-V,
//! its state - under a directory named by the device and driver that made it. An entry is handed
//! back to the driver only whole and unchanged: its envelope names the device, the driver version,
//! the pipeline-cache UUID and the key, and holds the data's length and hash, all checked before
//! the driver sees a byte. Anything else - a missing, short, corrupt or foreign entry, a driver
//! that refuses it - compiles the pipeline afresh and writes the entry again.
//!
//! The driver may still fail on data it made itself: loading one title's whole cache, hundreds of
//! megabytes, has been reported to stop a driver. An entry is a single pipeline's, and a marker
//! file stands beside it while the driver loads it; a run that finds a marker left from the last
//! drops that entry, so one bad entry costs one run and one compile, never every boot.

use ash::vk;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{OnceLock, PoisonError};
use std::time::SystemTime;

/// An entry file's first bytes.
const MAGIC: [u8; 8] = *b"ORBPIPE\0";
/// The envelope's layout; another value is not read.
const FORMAT: u32 = 1;
/// The envelope's length: magic, format, vendor, device, driver version, UUID, key, the data's
/// length and its hash.
const ENVELOPE: usize = 8 + 4 + 4 + 4 + 4 + 16 + 16 + 8 + 8;
/// The most bytes one entry's data may hold: no pipeline's cache data comes near it, and a length
/// field past it is a corrupt one.
const MOST_DATA: u64 = 1 << 30;
/// The most bytes a title's entries for one driver are kept to; the entries used longest ago are
/// dropped past it.
const MOST_KEPT: u64 = 4 << 30;
/// How many bytes of entries a run writes between two bounds of the store's size.
const BOUND_EVERY: u64 = 1 << 30;
/// The length of a pipeline cache's version-one header: its own length, its version, the vendor
/// and device ids and the pipeline-cache UUID (`VkPipelineCacheHeaderVersionOne`).
const HEADER_BYTES: usize = 32;
/// An entry file's extension, and the marker's beside it while the driver loads it.
const ENTRY: &str = "bin";
const LOADING: &str = "loading";
/// The file D744 kept one title's whole cache in, beside the directory: dropped unread.
const MONOLITHIC: &str = "bin";

/// Which device and driver made a pipeline's data: the only ones it is handed back to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Identity {
    /// `VkPhysicalDeviceProperties::vendorID`.
    pub vendor: u32,
    /// `deviceID`.
    pub device: u32,
    /// `driverVersion`.
    pub driver: u32,
    /// `pipelineCacheUUID`.
    pub uuid: [u8; 16],
}

impl Identity {
    /// The identity of the device `properties` describe.
    #[must_use]
    pub fn of(properties: &vk::PhysicalDeviceProperties) -> Self {
        Self {
            vendor: properties.vendor_id,
            device: properties.device_id,
            driver: properties.driver_version,
            uuid: properties.pipeline_cache_uuid,
        }
    }

    /// The directory name its entries are kept under.
    fn directory(&self) -> String {
        let uuid = self.uuid.iter().fold(String::new(), |mut text, byte| {
            let _ = write!(text, "{byte:02x}");
            text
        });
        format!(
            "{:04x}-{:04x}-{:08x}-{uuid}",
            self.vendor, self.device, self.driver
        )
    }
}

/// What one pipeline is, for its entry: a hash of its SPIR-V and one of the state it was built
/// with, which the caller forms from everything else the create info names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Key {
    /// A content hash of its modules' words, in order.
    pub spirv: u64,
    /// The caller's hash of its state.
    pub state: u64,
}

impl Key {
    /// The key of a pipeline from `modules` built with state hashed to `state`.
    #[must_use]
    pub fn of(modules: &[&[u32]], state: u64) -> Self {
        let mut hasher = orbistoun_gpu::ContentHasher::new(modules.iter().map(|m| m.len()).sum());
        for module in modules {
            for word in *module {
                hasher.word(*word);
            }
        }
        Self {
            spirv: hasher.finish(),
            state,
        }
    }

    fn bytes(self) -> [u8; 16] {
        let mut bytes = [0; 16];
        bytes[..8].copy_from_slice(&self.spirv.to_le_bytes());
        bytes[8..].copy_from_slice(&self.state.to_le_bytes());
        bytes
    }

    fn file(self, extension: &str) -> String {
        format!("{:016x}{:016x}.{extension}", self.spirv, self.state)
    }
}

/// A hash of an entry's data, its length among what it covers.
fn data_hash(data: &[u8]) -> u64 {
    let mut hasher = orbistoun_gpu::ContentHasher::new(data.len());
    hasher.bytes(data);
    let whole = data.len() - data.len() % 4;
    let mut tail = [0; 4];
    tail[..data.len() - whole].copy_from_slice(&data[whole..]);
    hasher.word(u32::from_le_bytes(tail));
    hasher.finish()
}

/// `data` in its envelope, for `identity` and `key`.
#[must_use]
pub fn enveloped(identity: Identity, key: Key, data: &[u8]) -> Vec<u8> {
    let mut file = Vec::with_capacity(ENVELOPE + data.len());
    file.extend(MAGIC);
    for word in [FORMAT, identity.vendor, identity.device, identity.driver] {
        file.extend(word.to_le_bytes());
    }
    file.extend(identity.uuid);
    file.extend(key.bytes());
    file.extend((data.len() as u64).to_le_bytes());
    file.extend(data_hash(data).to_le_bytes());
    file.extend(data);
    file
}

/// Why an entry is not handed to the driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// Shorter than its envelope, or than the length it names.
    Short,
    /// Another magic or format.
    NotAnEntry,
    /// Made by another device or driver.
    Foreign,
    /// Another pipeline's.
    OtherKey,
    /// Its data does not hash as its envelope says.
    Corrupt,
    /// Its data is not a pipeline cache this device made.
    NotThisDevices,
}

/// The data in `file` where it is a whole, unchanged entry for `identity` and `key`: every field
/// of the envelope checked, the data's length and hash, and the driver's own header within it.
///
/// # Errors
///
/// Why it is refused.
pub fn opened(file: &[u8], identity: Identity, key: Key) -> Result<&[u8], Refused> {
    let header = file.get(..ENVELOPE).ok_or(Refused::Short)?;
    let word = |at: usize| {
        u32::from_le_bytes([header[at], header[at + 1], header[at + 2], header[at + 3]])
    };
    let long = |at: usize| {
        let mut bytes = [0; 8];
        bytes.copy_from_slice(&header[at..at + 8]);
        u64::from_le_bytes(bytes)
    };
    if header[..8] != MAGIC || word(8) != FORMAT {
        return Err(Refused::NotAnEntry);
    }
    let made = Identity {
        vendor: word(12),
        device: word(16),
        driver: word(20),
        uuid: header[24..40].try_into().map_err(|_| Refused::Short)?,
    };
    if made != identity {
        return Err(Refused::Foreign);
    }
    if header[40..56] != key.bytes() {
        return Err(Refused::OtherKey);
    }
    let length = long(56);
    if length > MOST_DATA || file.len() as u64 != ENVELOPE as u64 + length {
        return Err(Refused::Short);
    }
    let data = &file[ENVELOPE..];
    if data_hash(data) != long(64) {
        return Err(Refused::Corrupt);
    }
    if !made_by(data, identity) {
        return Err(Refused::NotThisDevices);
    }
    Ok(data)
}

/// Whether `data` is a pipeline cache `identity`'s device made: a version-one header naming its
/// vendor, device and pipeline-cache UUID, no longer than the data.
fn made_by(data: &[u8], identity: Identity) -> bool {
    let word = |at: usize| {
        data.get(at..at + 4)
            .map(|bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    };
    word(0).is_some_and(|length| length as usize >= HEADER_BYTES && length as usize <= data.len())
        && word(4) == Some(vk::PipelineCacheHeaderVersion::ONE.as_raw() as u32)
        && word(8) == Some(identity.vendor)
        && word(12) == Some(identity.device)
        && data.get(16..32) == Some(&identity.uuid[..])
}

/// The title's entries for this device, once the device opens.
struct Store {
    directory: PathBuf,
    identity: Identity,
    /// When the store opened, which every entry this run uses is stamped before ([`stamp`]).
    opened: SystemTime,
}

/// How many entries this run has stamped.
static STAMPED: AtomicU64 = AtomicU64::new(0);

/// Stamps `entry` as used now, for the size bound, which drops the entries with the oldest stamp
/// first ([`bound`]): this run's are stamped just before it opened the store, each a millisecond
/// before the one before it, so they come after every earlier run's, and among them the first a
/// run used - its boot's - come last.
fn stamp(store: &Store, entry: &Path) {
    let order = STAMPED.fetch_add(1, Ordering::Relaxed);
    let at = store
        .opened
        .checked_sub(std::time::Duration::from_millis(order))
        .unwrap_or(store.opened);
    let _ = std::fs::File::options()
        .write(true)
        .open(entry)
        .and_then(|file| file.set_modified(at));
}

/// Where the title's entries are kept, when the host named a place.
static KEEP_AT: OnceLock<PathBuf> = OnceLock::new();
/// The store, once the device opened.
static STORE: OnceLock<Store> = OnceLock::new();
/// Bytes of entries read and written this run.
static READ: AtomicU64 = AtomicU64::new(0);
static WRITTEN: AtomicU64 = AtomicU64::new(0);
/// A temporary file's distinct suffix within the process.
static SERIAL: AtomicU64 = AtomicU64::new(0);

/// Keeps compiled pipelines under `directory`, the title's; the first place named is the one used.
/// A device already open has its store opened now.
pub fn keep_pipelines_at(directory: PathBuf) {
    let _ = KEEP_AT.set(directory);
    if let Some(session) = crate::compute::opened_session() {
        let session = session.lock().unwrap_or_else(PoisonError::into_inner);
        open(&session.instance, session.physical, &session.device);
    }
}

/// Opens the store for the session's device: drops what earlier runs left that must not be read -
/// the entries whose markers say the driver stopped loading them, another driver's entries, D744's
/// single cache file - and keeps the rest within [`MOST_KEPT`].
pub(crate) fn open(instance: &ash::Instance, physical: vk::PhysicalDevice, _device: &ash::Device) {
    let Some(base) = KEEP_AT.get() else {
        return;
    };
    if STORE.get().is_some() {
        return;
    }
    // SAFETY: the physical device came from this live instance.
    let properties = unsafe { instance.get_physical_device_properties(physical) };
    let identity = Identity::of(&properties);
    let directory = base.join(identity.directory());
    if std::fs::create_dir_all(&directory).is_err() {
        tracing::warn!(
            "kept pipelines: {} cannot be made; none are kept",
            directory.display()
        );
        return;
    }
    let monolithic = base.with_extension(MONOLITHIC);
    if std::fs::remove_file(&monolithic).is_ok() {
        tracing::info!("kept pipelines: dropped the single cache file of D744 unread");
    }
    tidy(base, &directory);
    let _ = STORE.set(Store {
        directory,
        identity,
        opened: SystemTime::now(),
    });
}

/// Drops the entries a marker says the driver stopped loading, other drivers' directories, and the
/// entries used longest ago past [`MOST_KEPT`].
fn tidy(base: &Path, directory: &Path) {
    if let Ok(others) = std::fs::read_dir(base) {
        for other in others.flatten() {
            let path = other.path();
            if path.is_dir() && path != directory && std::fs::remove_dir_all(&path).is_ok() {
                tracing::info!(
                    "kept pipelines: dropped another driver's, {}",
                    path.display()
                );
            }
        }
    }
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        match path.extension().and_then(|e| e.to_str()) {
            Some(LOADING) => {
                let _ = std::fs::remove_file(path.with_extension(ENTRY));
                let _ = std::fs::remove_file(&path);
                tracing::warn!(
                    "kept pipelines: the last run stopped while the driver loaded {}; dropped, so it compiles afresh",
                    path.with_extension(ENTRY).display()
                );
            }
            Some(ENTRY) => {}
            // A temporary file a stopped run left.
            _ => {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
    bound(directory);
}

/// Drops the entries in `directory` used longest ago while they hold more than [`MOST_KEPT`]: at
/// the store's opening, and again each time a run has written another [`BOUND_EVERY`] bytes of
/// them, a run building thousands of pipelines writing gigabytes.
fn bound(directory: &Path) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    let mut kept: Vec<(SystemTime, u64, PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some(ENTRY)
            && let Ok(meta) = entry.metadata()
        {
            let used = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            kept.push((used, meta.len(), path));
        }
    }
    let mut total: u64 = kept.iter().map(|(_, bytes, _)| bytes).sum();
    kept.sort();
    for (_, bytes, path) in kept {
        if total <= MOST_KEPT {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total -= bytes;
        }
    }
}

/// How a pipeline was built (`built`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Built {
    /// From its kept entry, the driver compiling nothing.
    Found,
    /// Compiled, and its entry written.
    Compiled,
    /// Compiled with no store to keep it in, or where keeping it failed.
    Unkept,
}

/// Builds a pipeline with `create`, handed the pipeline cache to build it through: from the entry
/// `key` names where one is kept and whole, compiling afresh otherwise, and keeping what was
/// compiled. A refused entry, or one the driver fails to build from, is dropped and the pipeline
/// compiled from an empty cache. `None` for `key` builds without the store.
///
/// # Errors
///
/// The driver's error from the compile with no kept data, which is `create`'s own.
pub(crate) fn built<T>(
    device: &ash::Device,
    key: Option<Key>,
    mut create: impl FnMut(vk::PipelineCache) -> ash::prelude::VkResult<T>,
) -> (ash::prelude::VkResult<T>, Built) {
    let (Some(store), Some(key)) = (STORE.get(), key) else {
        return (create(vk::PipelineCache::null()), Built::Unkept);
    };
    let entry = store.directory.join(key.file(ENTRY));
    if let Some(data) = read_entry(&entry, store.identity, key) {
        if let Some((made, found)) = from_entry(device, store, key, &data, &mut create) {
            if found {
                stamp(store, &entry);
                return (Ok(made), Built::Found);
            }
            return (Ok(made), Built::Compiled);
        }
        let _ = std::fs::remove_file(&entry);
    }
    compiled(device, store, key, &mut create)
}

/// The data of the entry at `path` where it opens whole for `identity` and `key`; a refused entry
/// is dropped and said.
fn read_entry(path: &Path, identity: Identity, key: Key) -> Option<Vec<u8>> {
    let file = std::fs::read(path).ok()?;
    READ.fetch_add(file.len() as u64, Ordering::Relaxed);
    match opened(&file, identity, key) {
        Ok(data) => Some(data.to_vec()),
        Err(why) => {
            tracing::warn!(
                "kept pipelines: {} refused ({why:?}); it compiles afresh",
                path.display()
            );
            let _ = std::fs::remove_file(path);
            None
        }
    }
}

/// The pipeline built through a cache made from `data`, and whether the driver found it there:
/// the cache's data did not grow. Where it compiled instead, the entry is written again with what
/// the cache then holds. `None` where the cache cannot be made or the pipeline not built from it.
fn from_entry<T>(
    device: &ash::Device,
    store: &Store,
    key: Key,
    data: &[u8],
    create: &mut impl FnMut(vk::PipelineCache) -> ash::prelude::VkResult<T>,
) -> Option<(T, bool)> {
    let marker = store.directory.join(key.file(LOADING));
    std::fs::write(&marker, []).ok()?;
    let info = vk::PipelineCacheCreateInfo::default().initial_data(data);
    // SAFETY: the create info and the data it names outlive the call; the data is an entry this
    // device and driver wrote, checked whole.
    let cache = unsafe { device.create_pipeline_cache(&info, None) }.ok();
    let built = cache.and_then(|cache| {
        let before = cache_bytes(device, cache);
        let made = create(cache).ok();
        let after = cache_bytes(device, cache);
        let found = before.is_some() && after == before;
        if let Some(made) = made {
            if !found {
                keep(device, store, key, cache);
            }
            // SAFETY: the cache was created above, on this device, and nothing else holds it.
            unsafe { device.destroy_pipeline_cache(cache, None) };
            Some((made, found))
        } else {
            // SAFETY: as above.
            unsafe { device.destroy_pipeline_cache(cache, None) };
            None
        }
    });
    let _ = std::fs::remove_file(&marker);
    if built.is_none() {
        tracing::warn!(
            "kept pipelines: the driver built nothing from {}; it compiles afresh",
            key.file(ENTRY)
        );
    }
    built
}

/// The pipeline compiled through an empty cache of its own, whose data is then its entry.
fn compiled<T>(
    device: &ash::Device,
    store: &Store,
    key: Key,
    create: &mut impl FnMut(vk::PipelineCache) -> ash::prelude::VkResult<T>,
) -> (ash::prelude::VkResult<T>, Built) {
    let info = vk::PipelineCacheCreateInfo::default();
    // SAFETY: the create info outlives the call.
    let Ok(cache) = (unsafe { device.create_pipeline_cache(&info, None) }) else {
        return (create(vk::PipelineCache::null()), Built::Unkept);
    };
    let made = create(cache);
    let how = if made.is_ok() && keep(device, store, key, cache) {
        Built::Compiled
    } else {
        Built::Unkept
    };
    // SAFETY: the cache was created above, on this device, and nothing else holds it.
    unsafe { device.destroy_pipeline_cache(cache, None) };
    (made, how)
}

/// How many bytes the driver would write for `cache` now.
fn cache_bytes(device: &ash::Device, cache: vk::PipelineCache) -> Option<usize> {
    let mut size = 0;
    // SAFETY: the cache was created on this device and is live; a null data pointer asks only for
    // the size, which is written to a live local.
    let sized = unsafe {
        (device.fp_v1_0().get_pipeline_cache_data)(
            device.handle(),
            cache,
            &raw mut size,
            std::ptr::null_mut(),
        )
    };
    (sized == vk::Result::SUCCESS).then_some(size)
}

/// Writes `cache`'s data as `key`'s entry, through a temporary file so a stopped run leaves the
/// last whole one; whether it was written. Data this device would not read back is not kept.
fn keep(device: &ash::Device, store: &Store, key: Key, cache: vk::PipelineCache) -> bool {
    orbistoun_gpu::perf::span(orbistoun_gpu::perf::Span::KeepPipelineCache, || {
        write_entry(device, store, key, cache)
    })
}

/// [`keep`]'s work.
fn write_entry(device: &ash::Device, store: &Store, key: Key, cache: vk::PipelineCache) -> bool {
    // SAFETY: the cache was created on this device and is live.
    let Ok(data) = (unsafe { device.get_pipeline_cache_data(cache) }) else {
        return false;
    };
    if !made_by(&data, store.identity) || data.len() as u64 > MOST_DATA {
        return false;
    }
    let file = enveloped(store.identity, key, &data);
    let entry = store.directory.join(key.file(ENTRY));
    let temporary = store.directory.join(format!(
        "{}.{}-{}",
        key.file("partial"),
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ));
    let written =
        std::fs::write(&temporary, &file).is_ok() && std::fs::rename(&temporary, &entry).is_ok();
    if written {
        stamp(store, &entry);
        let bytes = file.len() as u64;
        let before = WRITTEN.fetch_add(bytes, Ordering::Relaxed);
        if before / BOUND_EVERY != (before + bytes) / BOUND_EVERY {
            bound(&store.directory);
        }
    } else {
        let _ = std::fs::remove_file(&temporary);
    }
    written
}

/// Bytes of kept pipelines this run read and wrote.
#[must_use]
pub fn traffic() -> (u64, u64) {
    (
        READ.load(Ordering::Relaxed),
        WRITTEN.load(Ordering::Relaxed),
    )
}

/// The bytes of pipeline-cache data the store holds in host memory between pipelines: none, each
/// entry's cache is destroyed once its pipeline is built.
#[must_use]
pub const fn held_bytes() -> u64 {
    0
}

/// The end of a run: says how many pipelines were built, how long they took and how many were
/// found kept, and how many bytes of entries were read and written.
pub fn flush() {
    crate::compile_watch::report();
    let (read, written) = traffic();
    tracing::info!("kept pipelines: {read} bytes read, {written} bytes written this run");
}

#[cfg(test)]
mod tests {
    use super::{ENTRY, HEADER_BYTES, Identity, Key, LOADING, Refused, enveloped, opened, tidy};
    use ash::vk;

    const IDENTITY: Identity = Identity {
        vendor: 0x10de,
        device: 0x2c05,
        driver: 0x9b_a1_40_00,
        uuid: [7; 16],
    };
    const KEY: Key = Key {
        spirv: 0x1234_5678_9abc_def0,
        state: 42,
    };

    /// Pipeline-cache data as `identity`'s driver writes it: a version-one header, then its own.
    fn cache_data(identity: Identity) -> Vec<u8> {
        let mut data = Vec::new();
        for word in [
            HEADER_BYTES as u32,
            vk::PipelineCacheHeaderVersion::ONE.as_raw() as u32,
            identity.vendor,
            identity.device,
        ] {
            data.extend(word.to_le_bytes());
        }
        data.extend(identity.uuid);
        data.extend((0..203u32).map(|byte| byte as u8));
        data
    }

    /// An entry opens to its data, unchanged, for its own device, driver and key.
    #[test]
    fn an_entry_opens_whole_for_its_own_device_and_key() {
        let data = cache_data(IDENTITY);
        let file = enveloped(IDENTITY, KEY, &data);
        assert_eq!(opened(&file, IDENTITY, KEY), Ok(&data[..]));
    }

    /// Every way an entry can be wrong refuses it before the driver sees a byte: cut short or run
    /// long, a byte changed, another driver version, device or UUID, another key, not an entry, or
    /// data that is not this device's cache.
    #[test]
    fn a_damaged_or_foreign_entry_is_refused() {
        let data = cache_data(IDENTITY);
        let file = enveloped(IDENTITY, KEY, &data);
        assert_eq!(
            opened(&file[..file.len() - 1], IDENTITY, KEY),
            Err(Refused::Short)
        );
        assert_eq!(opened(&file[..20], IDENTITY, KEY), Err(Refused::Short));
        let mut longer = file.clone();
        longer.push(0);
        assert_eq!(opened(&longer, IDENTITY, KEY), Err(Refused::Short));
        let mut changed = file.clone();
        let last = changed.len() - 1;
        changed[last] ^= 1;
        assert_eq!(opened(&changed, IDENTITY, KEY), Err(Refused::Corrupt));
        let newer = Identity {
            driver: IDENTITY.driver + 1,
            ..IDENTITY
        };
        assert_eq!(opened(&file, newer, KEY), Err(Refused::Foreign));
        let other_uuid = Identity {
            uuid: [8; 16],
            ..IDENTITY
        };
        assert_eq!(opened(&file, other_uuid, KEY), Err(Refused::Foreign));
        let other = Key { state: 43, ..KEY };
        assert_eq!(opened(&file, IDENTITY, other), Err(Refused::OtherKey));
        let mut not_an_entry = file.clone();
        not_an_entry[0] = b'X';
        assert_eq!(
            opened(&not_an_entry, IDENTITY, KEY),
            Err(Refused::NotAnEntry)
        );
        let foreign_data = cache_data(Identity {
            device: 0x2c02,
            ..IDENTITY
        });
        let file = enveloped(IDENTITY, KEY, &foreign_data);
        assert_eq!(opened(&file, IDENTITY, KEY), Err(Refused::NotThisDevices));
    }

    /// Opening the store drops what must not be read: the entry a marker says the driver stopped
    /// loading, with its marker; another driver's directory; a temporary file a stopped write left.
    /// Every other entry stays.
    #[test]
    fn opening_drops_a_stopped_load_another_driver_and_partial_writes() {
        let base =
            std::env::temp_dir().join(format!("orbistoun-pipeline-store-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let mine = base.join(IDENTITY.directory());
        let theirs = base.join(
            Identity {
                driver: 1,
                ..IDENTITY
            }
            .directory(),
        );
        std::fs::create_dir_all(&mine).expect("made");
        std::fs::create_dir_all(&theirs).expect("made");
        let kept = Key { state: 1, ..KEY };
        let stopped = Key { state: 2, ..KEY };
        for key in [kept, stopped] {
            std::fs::write(mine.join(key.file(ENTRY)), [1]).expect("written");
        }
        std::fs::write(mine.join(stopped.file(LOADING)), []).expect("written");
        std::fs::write(mine.join("half.partial.7-0"), [1]).expect("written");
        std::fs::write(theirs.join(kept.file(ENTRY)), [1]).expect("written");
        tidy(&base, &mine);
        assert!(mine.join(kept.file(ENTRY)).exists(), "an entry stays");
        assert!(
            !mine.join(stopped.file(ENTRY)).exists(),
            "the stopped one goes"
        );
        assert!(
            !mine.join(stopped.file(LOADING)).exists(),
            "with its marker"
        );
        assert!(
            !mine.join("half.partial.7-0").exists(),
            "a partial write goes"
        );
        assert!(!theirs.exists(), "another driver's entries go");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// The smallest compute module: `main`, one thread, nothing bound, returning.
    const EMPTY_COMPUTE: [u32; 31] = [
        0x0723_0203,
        0x0001_0000,
        0,
        5,
        0, // header: version 1.0, five ids
        0x0002_0011,
        1, // OpCapability Shader
        0x0003_000e,
        0,
        1, // OpMemoryModel Logical GLSL450
        0x0005_000f,
        5,
        3,
        0x6e69_616d,
        0, // OpEntryPoint GLCompute %3 "main"
        0x0006_0010,
        3,
        17,
        1,
        1,
        1, // OpExecutionMode %3 LocalSize 1 1 1
        0x0002_0013,
        1, // %1 = OpTypeVoid
        0x0003_0021,
        2,
        1, // %2 = OpTypeFunction %1
        0x0005_0036,
        1,
        3,
        0,
        2, // %3 = OpFunction %1 None %2
    ];
    const EMPTY_COMPUTE_END: [u32; 4] = [0x0002_00f8, 4, 0x0001_00fd, 0x0001_0038];

    /// On the device, through the store: a pipeline is compiled and its entry written, then found
    /// from it with nothing compiled. An entry with a byte changed, and one written by another
    /// driver version, are each refused, the pipeline compiled afresh and the entry written whole
    /// again - never handed to the driver.
    #[test]
    fn a_kept_pipeline_is_found_and_a_damaged_or_foreign_entry_compiles_afresh() {
        use super::{Built, STORE, built, enveloped, keep_pipelines_at};
        match crate::compute::probe() {
            crate::compute::Availability::Available { .. } => {}
            crate::compute::Availability::Unavailable { reason } => {
                println!("SKIPPED - no device: {reason}");
                return;
            }
        }
        let base =
            std::env::temp_dir().join(format!("orbistoun-kept-store-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        keep_pipelines_at(base.clone());
        let session = crate::compute::session().expect("a device");
        let session = session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        crate::pipeline_cache::open(&session.instance, session.physical, &session.device);
        let Some(store) = STORE.get() else {
            println!("SKIPPED - another test named the store's place first");
            return;
        };
        let device = &session.device;
        let words: Vec<u32> = EMPTY_COMPUTE
            .iter()
            .chain(&EMPTY_COMPUTE_END)
            .copied()
            .collect();
        let info = vk::ShaderModuleCreateInfo::default().code(&words);
        // SAFETY: the words outlive the call and are a complete module.
        let shader = unsafe { device.create_shader_module(&info, None) }.expect("a module");
        // SAFETY: an empty layout's create info outlives the call.
        let layout = unsafe {
            device.create_pipeline_layout(&vk::PipelineLayoutCreateInfo::default(), None)
        }
        .expect("a layout");
        let stage = vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::COMPUTE)
            .module(shader)
            .name(c"main");
        let infos = [vk::ComputePipelineCreateInfo::default()
            .stage(stage)
            .layout(layout)];
        let key = Key::of(&[&words], std::process::id().into());
        let build = || {
            let (made, how) = built(device, Some(key), |cache| {
                // SAFETY: the create info outlives the call and names live objects.
                unsafe { device.create_compute_pipelines(cache, &infos, None) }.map_err(|(_, e)| e)
            });
            for pipeline in made.expect("a pipeline") {
                // SAFETY: made above on this device, used for nothing.
                unsafe { device.destroy_pipeline(pipeline, None) };
            }
            how
        };
        let entry = store.directory.join(key.file(ENTRY));
        assert_eq!(build(), Built::Compiled, "a first build compiles");
        println!(
            "an entry of the smallest compute pipeline: {} bytes",
            std::fs::metadata(store.directory.join(key.file(ENTRY))).map_or(0, |m| m.len())
        );
        assert!(entry.exists(), "and keeps its entry");
        assert_eq!(build(), Built::Found, "a second is found in it");

        let whole = std::fs::read(&entry).expect("kept");
        let mut changed = whole.clone();
        let last = changed.len() - 1;
        changed[last] ^= 0x55;
        std::fs::write(&entry, &changed).expect("written");
        assert_eq!(build(), Built::Compiled, "a changed entry compiles afresh");
        assert!(
            opened(&std::fs::read(&entry).expect("kept"), store.identity, key).is_ok(),
            "and is written whole again"
        );

        let data = opened(&whole, store.identity, key).expect("whole").to_vec();
        let newer = Identity {
            driver: store.identity.driver.wrapping_add(1),
            ..store.identity
        };
        std::fs::write(&entry, enveloped(newer, key, &data)).expect("written");
        assert_eq!(
            build(),
            Built::Compiled,
            "another driver's entry compiles afresh"
        );
        assert_eq!(build(), Built::Found, "and is this driver's again");

        // SAFETY: made above on this device; no pipeline built from them is alive.
        unsafe { device.destroy_pipeline_layout(layout, None) };
        // SAFETY: as above.
        unsafe { device.destroy_shader_module(shader, None) };
        let _ = std::fs::remove_dir_all(&base);
    }
}
