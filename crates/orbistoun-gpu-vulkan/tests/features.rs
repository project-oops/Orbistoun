//! What the capability report claims, against what the device was created with.
//!
//! A Vulkan feature is off until a device is created asking for it, so what the device offers and
//! what this process may use can differ. Reporting the first would claim a capability nothing
//! backs: a module built on it would fail at pipeline creation.

use ash::vk;
use orbistoun_gpu_vulkan::compute::{Availability, probe};

/// The report describes the device that exists, not the one that could have been created.
///
/// `Properties::fragment_stores` says what the session enabled, checked against the same physical
/// device asked directly, so reporting the physical capability instead fails. A guest's pixel
/// shader stores to guest memory, so the session requests `fragmentStoresAndAtomics` where the
/// device offers it. `tools/validate-device.sh` asks a validator about every feature at once.
#[test]
fn the_capability_report_describes_what_was_enabled() {
    let Availability::Available { properties } = probe() else {
        println!("[the_capability_report_describes_what_was_enabled] SKIPPED - no device");
        return;
    };

    // SAFETY: loading the Vulkan loader, which `probe` above already opened successfully, so the
    // library is present and the process is not tearing down. The features are then asked of the
    // same physical device the session opened.
    let entry = unsafe { ash::Entry::load() }.expect("the loader is present, a device was found");
    let app = vk::ApplicationInfo::default().api_version(vk::make_api_version(0, 1, 3, 0));
    let create = vk::InstanceCreateInfo::default().application_info(&app);
    // SAFETY: the create info outlives the call and the entry is loaded.
    let instance = unsafe { entry.create_instance(&create, None) }.expect("an instance");
    // SAFETY: the instance is live.
    let devices = unsafe { instance.enumerate_physical_devices() }.expect("physical devices");
    let offered = devices.iter().any(|physical| {
        // SAFETY: the handle came from this instance's own enumeration.
        let features = unsafe { instance.get_physical_device_features(*physical) };
        features.fragment_stores_and_atomics == vk::TRUE
    });
    // SAFETY: nothing created from this instance outlives it; the loop above copied values.
    unsafe { instance.destroy_instance(None) };

    assert_eq!(
        properties.fragment_stores, offered,
        concat!(
            "the report and the device disagree about fragmentStoresAndAtomics. The session ",
            "asks for it exactly where the hardware offers it, so a report saying otherwise is ",
            "either reading the physical device's capabilities instead of the enabled set, or ",
            "claiming something a module would be refused for relying on"
        )
    );
    if offered {
        println!(concat!(
            "[the_capability_report_describes_what_was_enabled] the hardware offers ",
            "fragmentStoresAndAtomics, the session enabled it, and the report says so"
        ));
    } else {
        println!(concat!(
            "[the_capability_report_describes_what_was_enabled] this GPU does not offer ",
            "fragmentStoresAndAtomics, so the session did not enable it and the report says ",
            "so - a guest pixel shader that writes memory cannot run here at all"
        ));
    }
}

/// What this device says about block-compressed sampling, reported rather than assumed.
///
/// A device may say no, which makes a decoder a requirement rather than a fallback. Vulkan consumes
/// BC data natively where supported. Printed so a run on a new machine records its answer.
#[test]
fn whether_this_device_samples_compressed_textures_is_recorded() {
    let Availability::Available { properties } = probe() else {
        println!(
            "[whether_this_device_samples_compressed_textures_is_recorded] SKIPPED - no device"
        );
        return;
    };
    println!(
        "[whether_this_device_samples_compressed_textures_is_recorded] {} reports textureCompressionBC = {}",
        properties.device, properties.compressed_textures
    );
}
