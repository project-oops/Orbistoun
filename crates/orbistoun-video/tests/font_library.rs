//! `sceFontSelectLibraryFt`, `sceFontCreateLibraryWithEdition` and `sceFontDestroyLibrary` as
//! obSCEne measured them in PPSA21564's shape (REQ-fm02,
//! `reports/hardware/20261007-102530-eboot.obs.log` 4379-4409, `130-layout/font-create-library-ft`):
//! selection 0 answers a pointer and 1 null; creation over a font memory object writes a library
//! pointer and answers 0, over a null one writes 0 and answers `0x80460002`; destruction answers 0
//! and clears the library. What the library's blocks hold is the library's own (D151: a zeroed
//! block where the guest may read through it).

use orbistoun_core::GUEST_ARG_REGISTERS;

fn call(name: &str, args: [u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (_, handler) = orbistoun_video::implementations()
        .iter()
        .find(|(n, _)| *n == name)
        .unwrap_or_else(|| panic!("{name} is implemented"));
    handler(&args)
}

#[test]
fn a_library_is_selected_created_and_destroyed_as_measured() {
    let selection = call("sceFontSelectLibraryFt", [0, 0, 0, 0, 0, 0]);
    assert_ne!(selection, 0, "`sel0` a pointer");
    assert_eq!(
        call("sceFontSelectLibraryFt", [1, 0, 0, 0, 0, 0]),
        0,
        "`sel1` null"
    );

    let mut memory = [0_u8; 0x40];
    let mut library = 0xcccc_cccc_cccc_cccc_u64;
    let out = std::ptr::addr_of_mut!(library) as usize as u64;
    let create = |memory: u64| {
        call(
            "sceFontCreateLibraryWithEdition",
            [memory, selection, 0x0007_0010_0000_0000, out, 0, 0],
        )
    };
    assert_eq!(
        create(memory.as_mut_ptr() as usize as u64),
        0,
        "`arm2-create` rc"
    );
    assert_ne!(library, 0, "a library");
    assert_ne!(library, 0xcccc_cccc_cccc_cccc);
    // SAFETY: the block a library pointer names is readable.
    let head = unsafe { std::slice::from_raw_parts(library as usize as *const u8, 0x100) };
    assert!(head.iter().all(|&b| b == 0), "a zeroed block (D151)");

    assert_eq!(
        call("sceFontDestroyLibrary", [out, 0, 0, 0, 0, 0]),
        0,
        "`arm4-destroy`"
    );
    assert_eq!(library, 0, "`library-after` 0");

    library = 0xcccc_cccc_cccc_cccc;
    assert_eq!(create(0), 0x8046_0002, "`arm3-null-mem`");
    assert_eq!(library, 0, "written 0 first");
}
