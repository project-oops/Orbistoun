//! Compiled pipelines kept per title across runs (D744).
//!
//! A translated module is megabytes of SPIR-V, and the driver takes seconds to compile one. The
//! modules are the same from one run to the next (D113), so what the driver made of them is kept
//! too: one pipeline cache, loaded when the device opens and written back after each pipeline it
//! had to compile.

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
}

/// Keeps compiled pipelines at `path`. Takes effect for a device opened after the call; the
/// first place named is the one used.
pub fn keep_pipelines_at(path: PathBuf) {
    let _ = KEEP_AT.set(path);
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
    let _ = KEPT.set(Mutex::new(Kept {
        cache,
        written: data.len(),
    }));
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
pub(crate) fn keep(device: &ash::Device) {
    let (Some(path), Some(kept)) = (KEEP_AT.get(), KEPT.get()) else {
        return;
    };
    let Ok(mut kept) = kept.lock() else {
        return;
    };
    // SAFETY: the cache was created on this device and is live for the process.
    let Ok(data) = (unsafe { device.get_pipeline_cache_data(kept.cache) }) else {
        return;
    };
    if data.len() <= kept.written {
        return;
    }
    let partial = path.with_extension("partial");
    if std::fs::write(&partial, &data).is_ok() && std::fs::rename(&partial, path).is_ok() {
        kept.written = data.len();
    }
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
