//! Compiled pipelines kept per title across runs (D744).
//!
//! A translated module is megabytes of SPIR-V, and the driver takes seconds to compile one. The
//! modules are the same from one run to the next (D113), so what the driver made of them is kept
//! too: one pipeline cache, loaded when the device opens and written back as it grows - at most
//! every two seconds (`KEEP_EVERY`), and once more when the run ends ([`flush`]).

use ash::vk;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

/// Where the title's compiled pipelines are kept, when the host named a place.
static KEEP_AT: OnceLock<PathBuf> = OnceLock::new();

/// The session's pipeline cache and how many bytes of it were last written.
static KEPT: OnceLock<Mutex<Kept>> = OnceLock::new();

struct Kept {
    cache: vk::PipelineCache,
    written: usize,
    /// When it was last written, so a run compiling a pipeline a draw does not write the whole
    /// cache after each.
    at: Option<std::time::Instant>,
}

/// The least time between two writes of the cache while a run goes on. Neverball compiled a
/// pipeline every few draws, and writing its cache after each - fifteen milliseconds a time - was a
/// tenth of its run.
const KEEP_EVERY: std::time::Duration = std::time::Duration::from_secs(2);

/// Keeps compiled pipelines at `path`; the first place named is the one used. A device already
/// open has its cache opened now: the worker names the place once it knows the title, and by then
/// the device can be open already - which left every run since without a cache, each compute
/// pipeline compiled afresh at 257 ms (STKT00001).
pub fn keep_pipelines_at(path: PathBuf) {
    let _ = KEEP_AT.set(path);
    if let Some(session) = crate::compute::opened_session() {
        let session = session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        open(&session.instance, session.physical, &session.device);
    }
}

/// The length of a pipeline cache's version-one header: its own length, its version, the vendor
/// and device ids and the pipeline-cache UUID (`VkPipelineCacheHeaderVersionOne`).
const HEADER_BYTES: usize = 32;

/// Whether `data` is a cache this device made: a version-one header naming this device's vendor,
/// device and pipeline-cache UUID. Anything else is not handed to the driver.
fn made_here(data: &[u8], properties: &vk::PhysicalDeviceProperties) -> bool {
    let word = |at: usize| {
        data.get(at..at + 4)
            .map(|bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    };
    word(0).is_some_and(|length| length as usize >= HEADER_BYTES)
        && word(4) == Some(vk::PipelineCacheHeaderVersion::ONE.as_raw() as u32)
        && word(8) == Some(properties.vendor_id)
        && word(12) == Some(properties.device_id)
        && data.get(16..32) == Some(&properties.pipeline_cache_uuid[..])
}

/// Opens the session's pipeline cache, from the kept file where it is this device's. Nothing is
/// opened when no place was named.
pub(crate) fn open(instance: &ash::Instance, physical: vk::PhysicalDevice, device: &ash::Device) {
    let Some(path) = KEEP_AT.get() else {
        return;
    };
    if KEPT.get().is_some() {
        return;
    }
    // SAFETY: the physical device came from this live instance.
    let properties = unsafe { instance.get_physical_device_properties(physical) };
    let data = std::fs::read(path)
        .ok()
        .filter(|data| made_here(data, &properties))
        .unwrap_or_default();
    let info = vk::PipelineCacheCreateInfo::default().initial_data(&data);
    // SAFETY: the create info and the data it names outlive the call.
    let Ok(cache) = (unsafe { device.create_pipeline_cache(&info, None) }) else {
        return;
    };
    // What the driver kept of the file, not the file's length: a driver drops entries it no longer
    // uses, so a cache kept from older modules loads smaller than its file. Measured against the
    // file, it never grew past it and was never written again - STKT00001's stayed five days old
    // while every run compiled each compute pipeline afresh, 257 ms apiece.
    let _ = KEPT.set(Mutex::new(Kept {
        cache,
        written: cache_size(device, cache).unwrap_or(0),
        at: None,
    }));
}

/// How many bytes the driver would write for `cache` now.
fn cache_size(device: &ash::Device, cache: vk::PipelineCache) -> Option<usize> {
    let mut size = 0;
    // SAFETY: the cache was created on this device and is live for the process; a null data
    // pointer asks only for the size, which is written to a live local.
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

/// The cache a pipeline is created through: the session's, or none.
pub(crate) fn cache() -> vk::PipelineCache {
    KEPT.get()
        .and_then(|kept| kept.lock().ok().map(|kept| kept.cache))
        .unwrap_or_default()
}

/// Writes the cache back when it grew since it was last written: after a pipeline was compiled
/// rather than found. Through a temporary file, so a run stopped mid-write leaves the last whole
/// one.
///
/// Its size is asked first and its data fetched only when that grew: a title's cache runs to tens
/// of megabytes, and copying it out after every pipeline built - most of them found in it - cost
/// more than building them (6 ms a draw, STKT00001 generating mipmaps).
pub(crate) fn keep(device: &ash::Device) {
    write_back(device, false);
}

/// Writes the cache back now where it grew since it was last written, however recently that was:
/// at the end of a run, so the pipelines its last seconds compiled are kept too.
pub fn flush() {
    if let Some(session) = crate::compute::opened_session() {
        let session = session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        write_back(&session.device, true);
    }
}

/// [`keep`], or with `now` [`flush`].
fn write_back(device: &ash::Device, now: bool) {
    let (Some(path), Some(kept)) = (KEEP_AT.get(), KEPT.get()) else {
        return;
    };
    let Ok(mut kept) = kept.lock() else {
        return;
    };
    if !now && kept.at.is_some_and(|at| at.elapsed() < KEEP_EVERY) {
        return;
    }
    if cache_size(device, kept.cache).is_none_or(|size| size <= kept.written) {
        return;
    }
    kept.at = Some(std::time::Instant::now());
    orbistoun_gpu::perf::span(orbistoun_gpu::perf::Span::KeepPipelineCache, || {
        // SAFETY: the cache was created on this device and is live for the process.
        let Ok(data) = (unsafe { device.get_pipeline_cache_data(kept.cache) }) else {
            return;
        };
        let partial = path.with_extension("partial");
        if std::fs::write(&partial, &data).is_ok() && std::fs::rename(&partial, path).is_ok() {
            kept.written = data.len();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{HEADER_BYTES, made_here};
    use ash::vk;

    /// Only a cache whose header names this device's vendor, device and UUID is handed back.
    #[test]
    fn only_this_devices_cache_is_loaded() {
        let mut properties = vk::PhysicalDeviceProperties {
            vendor_id: 0x10de,
            device_id: 0x2c05,
            pipeline_cache_uuid: [7; 16],
            ..Default::default()
        };
        let mut data = Vec::new();
        for word in [HEADER_BYTES as u32, 1, 0x10de, 0x2c05] {
            data.extend(word.to_le_bytes());
        }
        data.extend([7u8; 16]);
        data.extend([0xaa; 64]);
        assert!(made_here(&data, &properties));
        let mut other = data.clone();
        other[16] = 8;
        assert!(!made_here(&other, &properties), "another UUID");
        assert!(!made_here(&data[..20], &properties), "a short header");
        properties.device_id = 0x2c02;
        assert!(!made_here(&data, &properties), "another device");
    }
}
