//! What the capability report claims, against what the device was actually created with.
//!
//! # The distinction this is about
//!
//! A Vulkan feature is off until a device is created asking for it. So there are two different
//! answers to "can a fragment shader write a storage buffer here": what the silicon offers, and
//! what this process may use. **They differ on this machine** - an RTX 5070 Ti reports
//! `fragmentStoresAndAtomics` as available, and orbistoun creates its device requesting no
//! features at all (D552).
//!
//! Reporting the first would be a capability nothing backs: a translated fragment module built
//! on it would be invalid at pipeline creation, and the report would have said it was fine.

use ash::vk;
use orbistoun_gpu_vulkan::compute::{Availability, probe};

/// **The report describes the device that exists, not the one that could have been created.**
///
/// # What this asserts
///
/// That `Properties::fragment_stores` says what the session actually enabled - no more and no
/// less - checked against the same physical device asked directly.
///
/// It fails if somebody reports the physical device's capability instead of the enabled set,
/// which is the mistake it exists to prevent and the easier of the two to write.
///
/// # This used to assert the opposite, and the change is the point
///
/// The session requested nothing at all, on the reasoning that the fragment path was designed
/// not to need `fragmentStoresAndAtomics` (D552): a translated module keeps its registers in
/// `Private` storage, so the only storage-buffer writes were the observation window - which a
/// fragment module skips - and a guest-memory store, which nothing emitted.
///
/// A guest's pixel shader emits one. The GL cube's writes a canary word every frame, and when
/// that shader was first offered to a driver the validation layer named this feature among
/// five errors the tests could not see (worklog 554). So the session asks for it where the
/// device offers it, and this asserts the report tracks that rather than a fixed answer.
///
/// # What it cannot assert
///
/// That the feature is *sufficient*, or that any module needs no others. One feature is enough
/// to pin the rule that a report describes the enabled set; a list would be a list to maintain,
/// and `tools/validate-device.sh` asks a validator about all of them at once.
#[test]
fn the_capability_report_describes_what_was_enabled() {
    let Availability::Available { properties } = probe() else {
        println!("[the_capability_report_describes_what_was_enabled] SKIPPED - no device");
        return;
    };

    // What the silicon offers, asked of the same physical device the session opened.
    // SAFETY: loading the Vulkan loader. `probe` above already opened it successfully, so the
    // library is present and the process is not mid-teardown.
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
    // SAFETY: nothing created from this instance outlives it - the loop above copied values.
    unsafe { instance.destroy_instance(None) };

    assert_eq!(
        properties.fragment_stores, offered,
        "the report and the device disagree about fragmentStoresAndAtomics. The session asks 
         for it exactly where the hardware offers it, so a report saying otherwise is either 
         reading the physical device's capabilities instead of the enabled set, or claiming 
         something a module would be refused for relying on"
    );
    if offered {
        println!(
            "[the_capability_report_describes_what_was_enabled] the hardware offers 
             fragmentStoresAndAtomics, the session enabled it, and the report says so"
        );
    } else {
        println!(
            "[the_capability_report_describes_what_was_enabled] this GPU does not offer 
             fragmentStoresAndAtomics, so the session did not enable it and the report says 
             so - a guest pixel shader that writes memory cannot run here at all"
        );
    }
}
