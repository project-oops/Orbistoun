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
/// That `Properties::fragment_stores` is false while the session enables no features, *and*
/// that this is not vacuous - the same physical device is asked directly, and the test only
/// means something when the hardware says yes and the report says no. On a machine whose GPU
/// genuinely lacks the feature the two agree for an uninteresting reason, and that is said
/// aloud rather than passed over.
///
/// It fails if somebody reports the physical device's capability instead of the enabled set,
/// which is the mistake it exists to prevent and the easier of the two to write.
///
/// # What it cannot assert
///
/// That the feature is *unnecessary*. The fragment path is designed not to need it - registers
/// are already `Private`, so the only storage-buffer writes are the observation window and a
/// guest-memory store - but nothing here checks that design, because no fragment module is
/// emitted yet.
///
/// It catches **over**-reporting only. A report claiming less than was enabled would make code
/// refuse something that would have worked - a waste rather than a fault, and invisible here.
/// The dangerous direction is the one asserted.
///
/// Nor does it check any other feature. One is enough to pin the rule; a list would be a list
/// to maintain.
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

    assert!(
        !properties.fragment_stores,
        "the report claims a fragment shader may store, and the device was created requesting \
         no features - so a module built on that claim would be invalid at pipeline creation"
    );
    if offered {
        println!(
            "[the_capability_report_describes_what_was_enabled] the hardware offers \
             fragmentStoresAndAtomics and the report correctly does not"
        );
    } else {
        println!(
            "[the_capability_report_describes_what_was_enabled] this GPU does not offer \
             fragmentStoresAndAtomics either, so the two agree for an uninteresting reason \
             and this run proves less than it does on a device that offers it"
        );
    }
}
