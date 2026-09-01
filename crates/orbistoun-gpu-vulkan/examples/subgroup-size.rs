//! Reports the device's subgroup size.
//!
//! The subgroup fidelity level materialises the guest's execution mask with a subgroup
//! ballot, and that is only correct when the hardware's subgroup is as wide as the
//! guest's wavefront - sixty-four lanes. Some hardware is thirty-two.
//!
//! This exists because that number decides whether the level can be *verified* here at
//! all, not merely whether it can run: the differential oracle compares it against the
//! wavefront model on the same shader, and a device that refuses the level cannot supply
//! one half of the comparison. Worth knowing before building it rather than after.

fn main() {
    // SAFETY: loading the Vulkan loader touches no state of ours and reports failure
    // rather than faulting.
    let Ok(entry) = (unsafe { ash::Entry::load() }) else {
        println!("no Vulkan loader on this machine");
        return;
    };

    let info = ash::vk::InstanceCreateInfo::default();
    // SAFETY: the create info is fully initialised and outlives the call.
    let Ok(instance) = (unsafe { entry.create_instance(&info, None) }) else {
        println!("no Vulkan instance could be created");
        return;
    };

    // SAFETY: the instance was created immediately above.
    let devices = unsafe { instance.enumerate_physical_devices() }.unwrap_or_default();
    for device in devices {
        let mut subgroup = ash::vk::PhysicalDeviceSubgroupProperties::default();
        let mut properties = ash::vk::PhysicalDeviceProperties2::default().push_next(&mut subgroup);
        // SAFETY: `properties` and the chained `subgroup` outlive the call, and the
        // chain is built by ash so its type tags are correct.
        unsafe { instance.get_physical_device_properties2(device, &mut properties) };

        let name = properties.properties.device_name_as_c_str();
        let name = name
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        println!(
            "{name}: subgroup size {}, stage flags {:#x}, operation flags {:#x}",
            subgroup.subgroup_size,
            subgroup.supported_stages.as_raw(),
            subgroup.supported_operations.as_raw()
        );
        println!(
            "  wavefront model needs 64; this device {}",
            if subgroup.subgroup_size == 64 {
                "can host the subgroup level"
            } else {
                "CANNOT - the subgroup level would be refused here"
            }
        );
    }

    // SAFETY: nothing was allocated from the instance that outlives it.
    unsafe { instance.destroy_instance(None) };
}
