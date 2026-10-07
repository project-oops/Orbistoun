//! `sceKernelMapNamedFlexibleMemory(out, len, prot, flags, name)` is `sceKernelMapFlexibleMemory`
//! with a label, as `sceKernelMapNamedDirectMemory` is the direct mapping with one: the pages come
//! from the same budget and the name changes nothing a guest reads. PPSA28061's shipped web-API
//! module maps its heap so, and fails its initialization on a placeholder.

use orbistoun_core::GUEST_ARG_REGISTERS;

fn call(name: &str, args: [u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (_, handler) = orbistoun_kernel::implementations()
        .iter()
        .find(|(n, _)| *n == name)
        .unwrap_or_else(|| panic!("{name} is implemented"));
    handler(&args)
}

#[test]
fn a_named_flexible_mapping_is_a_flexible_mapping() {
    let label = b"SceCommon heap\0";
    let mut at = 0_u64;
    let out = std::ptr::addr_of_mut!(at) as usize as u64;
    let before = {
        let mut free = 0_u64;
        call(
            "sceKernelAvailableFlexibleMemorySize",
            [std::ptr::addr_of_mut!(free) as usize as u64, 0, 0, 0, 0, 0],
        );
        free
    };
    let rc = call(
        "sceKernelMapNamedFlexibleMemory",
        [out, 0x4000, 0x3, 0, label.as_ptr() as usize as u64, 0],
    );
    assert_eq!(rc, 0);
    assert_ne!(at, 0, "an address handed back");
    // SAFETY: the mapping just made is 0x4000 read-write bytes.
    let page = unsafe { std::slice::from_raw_parts_mut(at as usize as *mut u8, 0x4000) };
    page[0x3fff] = 7;
    assert_eq!(page[0x3fff], 7, "writable");
    let mut free = 0_u64;
    call(
        "sceKernelAvailableFlexibleMemorySize",
        [std::ptr::addr_of_mut!(free) as usize as u64, 0, 0, 0, 0, 0],
    );
    assert_eq!(before - free, 0x4000, "drawn from the flexible budget");
}
