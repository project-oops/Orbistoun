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

/// `sceFontSupportSystemFonts(library)` and `sceFontSupportExternalFonts(library, 0, 0)` each store
/// a pointer in the library - at `+0xa0` and `+0xa8` - and answer 0; asked again on the same library
/// they answer `0x80460021` and change nothing; a null library answers `0x80460004` (REQ-fm04,
/// `reports/hardware/20261009-151440-eboot.obs.log`, `130-layout/font-create-library-ft`).
#[test]
fn font_support_is_set_up_once_per_library_as_measured() {
    let selection = call("sceFontSelectLibraryFt", [0, 0, 0, 0, 0, 0]);
    let mut memory = [0_u8; 0x40];
    let mut library = 0_u64;
    let out = std::ptr::addr_of_mut!(library) as usize as u64;
    assert_eq!(
        call(
            "sceFontCreateLibraryWithEdition",
            [memory.as_mut_ptr() as usize as u64, selection, 0, out, 0, 0],
        ),
        0
    );
    let word = |at: u64| {
        // SAFETY: inside the library block the create answered.
        unsafe { std::ptr::read((library + at) as usize as *const u64) }
    };
    assert_eq!(
        call("sceFontSupportSystemFonts", [library, 0, 0, 0, 0, 0]),
        0
    );
    let system = word(0xa0);
    assert_ne!(system, 0, "a pointer at +0xa0");
    assert_eq!(word(0xa8), 0, "external not yet");
    assert_eq!(
        call("sceFontSupportExternalFonts", [library, 0, 0, 0, 0, 0]),
        0
    );
    assert_ne!(word(0xa8), 0, "a pointer at +0xa8");
    assert_eq!(
        call("sceFontSupportSystemFonts", [library, 0, 0, 0, 0, 0]),
        0x8046_0021
    );
    assert_eq!(
        call("sceFontSupportExternalFonts", [library, 0, 0, 0, 0, 0]),
        0x8046_0021
    );
    assert_eq!(word(0xa0), system, "the second call changed nothing");
    assert_eq!(call("sceFontSupportSystemFonts", [0; 6]), 0x8046_0004);
}

/// What the attach test's malloc callback hands out: a 1 MiB block, or nothing.
static BLOCK: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// How many times the malloc callback was asked, and the last size.
static ASKED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The font memory interface's malloc, `(context, size)`, as the guest's would be.
extern "sysv64" fn malloc(_context: u64, size: u64) -> u64 {
    ASKED.store(size, std::sync::atomic::Ordering::SeqCst);
    BLOCK.load(std::sync::atomic::Ordering::SeqCst)
}

/// `sceFontAttachDeviceCacheBuffer(library, 0, size)` takes its block from the font memory's
/// malloc callback and answers 0 for any block it is given, ordinary heap memory included, setting
/// the library's byte `+8`; `0x80460010` when the callback answers null or the size is 2 MiB, and
/// `0x80460004` for a null library (REQ-fm09, `reports/hardware/20261011-083754-eboot.obs.log`
/// 4953-4974; size 0 answers `0x80460002`, REQ-fm05).
#[test]
fn a_cache_buffer_attaches_from_the_memory_interface_as_measured() {
    use std::sync::atomic::Ordering;
    let selection = call("sceFontSelectLibraryFt", [0, 0, 0, 0, 0, 0]);
    let interface: [u64; 4] = [
        malloc as extern "sysv64" fn(u64, u64) -> u64 as usize as u64,
        0,
        0,
        0,
    ];
    let mut memory = [0_u8; 0x40];
    let memory_at = memory.as_mut_ptr() as usize as u64;
    assert_eq!(
        call(
            "sceFontMemoryInit",
            [
                memory_at,
                0,
                0x80_0000,
                interface.as_ptr() as usize as u64,
                0x77,
                0
            ],
        ),
        0
    );
    let mut library = 0_u64;
    let out = std::ptr::addr_of_mut!(library) as usize as u64;
    assert_eq!(
        call(
            "sceFontCreateLibraryWithEdition",
            [memory_at, selection, 0, out, 0, 0]
        ),
        0
    );
    let attach = |library: u64, size: u64| {
        call(
            "sceFontAttachDeviceCacheBuffer",
            [library, 0, size, 0, 0, 0],
        )
    };

    assert_eq!(attach(0, 0x10_0000), 0x8046_0004, "null library");
    assert_eq!(attach(library, 0), 0x8046_0002, "size 0");
    assert_eq!(attach(library, 0x20_0000), 0x8046_0010, "2 MiB");

    BLOCK.store(0, Ordering::SeqCst);
    assert_eq!(
        attach(library, 0x10_0000),
        0x8046_0010,
        "the callback answered null"
    );
    assert_eq!(
        ASKED.load(Ordering::SeqCst),
        0x10_0000,
        "asked for the size"
    );

    let heap = vec![0_u8; 0x10_0010];
    BLOCK.store(heap.as_ptr() as usize as u64 + 0x10, Ordering::SeqCst);
    assert_eq!(
        attach(library, 0x10_0000),
        0,
        "an ordinary 16-aligned heap block"
    );
    // SAFETY: the library block the create answered.
    let attached = unsafe { std::ptr::read((library + 8) as usize as *const u8) };
    assert_eq!(attached, 1, "byte +8 set");
    drop(heap);
}
