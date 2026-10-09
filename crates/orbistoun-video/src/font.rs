//! `libSceFont` - the platform's font renderer. Its font memory object and library are implemented
//! as obSCEne measured them in PPSA21564's own call shape (REQ-fm01,
//! `reports/hardware/20261007-082134-eboot.obs.log` lines 4339-4377; REQ-fm02,
//! `reports/hardware/20261007-102530-eboot.obs.log` lines 4379-4409); the renderer itself is not.

/// `libSceFontFt`, which selects the FreeType-backed implementation a library is created over.
pub mod ft {
    use orbistoun_hle::guest_module;

    guest_module! {
        "libSceFontFt" {
            "sceFontSelectLibraryFt" => 1,
        }
    }
}

use orbistoun_core::GUEST_ARG_REGISTERS;
use orbistoun_hle::guest_module;
use orbistoun_mem::guest;

guest_module! {
    "libSceFont" {
        "sceFontMemoryInit" => 6,
        "sceFontMemoryTerm" => 1,
        // (memory, selection, edition, &library)
        "sceFontCreateLibraryWithEdition" => 4,
        // (&library)
        "sceFontDestroyLibrary" => 1,
        // (library)
        "sceFontSupportSystemFonts" => 1,
        // (library, 0, 0)
        "sceFontSupportExternalFonts" => 3,
    }
}

const OK: u64 = 0;

/// A null font memory object (`arm2-null-obj`).
const INVALID_ARGUMENT: u64 = 0x8046_0002;

/// The first four bytes `sceFontMemoryInit` writes, ahead of the size (`obj` row 0): byte 1 is
/// `0x0f` until `sceFontMemoryTerm` clears it; what it means is not established.
const HEADER: [u8; 4] = [0x00, 0x0f, 0x00, 0x00];

/// The bytes `sceFontMemoryInit` writes (`extent 64`).
const OBJECT_BYTES: usize = 0x40;

/// `sceFontMemoryInit(object, base, size, interface, mspace, destroy, ...)`: fills the 64-byte
/// object - the header, the size at +4, then the base, the mspace, the memory interface and the
/// destroy callback as quadwords from +8, zero after - and answers 0, a size of zero included. A
/// null object answers `0x80460002`. The seventh argument was zero when measured, and the bytes
/// after the callback with it.
pub(crate) fn font_memory_init(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let [object, base, size, interface, mspace, destroy] = *args;
    if object == 0 {
        return INVALID_ARGUMENT;
    }
    let mut bytes = [0_u8; OBJECT_BYTES];
    bytes[..4].copy_from_slice(&HEADER);
    bytes[4..8].copy_from_slice(&(size as u32).to_le_bytes());
    for (at, word) in [(8, base), (16, mspace), (24, interface), (32, destroy)] {
        bytes[at..at + 8].copy_from_slice(&word.to_le_bytes());
    }
    // SAFETY: the guest's font memory object, whose 64 bytes the call fills.
    if !unsafe { guest::write_bytes(object, &bytes) } {
        return INVALID_ARGUMENT;
    }
    OK
}

/// `sceFontMemoryTerm(object)`: calls the destroy callback with the object, clears byte 1, and
/// answers 0 (`arm3b-term`: `cb-calls 1`, `cb-arg` the object, `obj-term-cb 0000`). A null callback
/// is not called: what the library does with one is unmeasured.
pub(crate) fn font_memory_term(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let object = args[0];
    // SAFETY: the guest's font memory object, read through the checked accessor.
    let destroy = unsafe { guest::read_u64(object + 32) }.unwrap_or(0);
    if destroy != 0 {
        // SAFETY: the guest's own function, which it handed `sceFontMemoryInit` to call here.
        unsafe { orbistoun_kernel::thread::call_guest(destroy, [object, 0, 0]) };
    }
    // SAFETY: the object's first two bytes, which the call clears.
    unsafe { guest::write_bytes(object, &[0, 0]) };
    OK
}

/// A zeroed block of guest memory of at least `len` bytes, owned here and never freed (D151: the
/// library's own objects, which the guest may read through but whose contents are not known).
fn zeroed_block(len: u64) -> Option<u64> {
    let mut args = [0_u64; GUEST_ARG_REGISTERS];
    args[1] = len.next_multiple_of(0x4000);
    args[2] = 0x3; // PROT_READ | PROT_WRITE
    args[3] = 0x1000 | 0x2; // MAP_ANON | MAP_PRIVATE
    args[4] = u64::MAX;
    let at = orbistoun_kernel::mmap(&args);
    (at != u64::MAX && at != 0).then_some(at)
}

/// The bytes of a library object the measurement dumped (`lib-bytes`, 0x100).
const LIBRARY_BYTES: u64 = 0x100;

/// `sceFontSelectLibraryFt(value)`: 0 answers a pointer to the FreeType selection, 1 answers null
/// (`arm1-select`). The selection is the library's; here a zeroed block, made once (D151). Any
/// other value is unmeasured and answers null.
pub(crate) fn font_select_library_ft(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    static SELECTION: std::sync::OnceLock<Option<u64>> = std::sync::OnceLock::new();
    if args[0] != 0 {
        return 0;
    }
    SELECTION.get_or_init(|| zeroed_block(0x40)).unwrap_or(0)
}

/// `sceFontCreateLibraryWithEdition(memory, selection, edition, &library)`: writes 0 to `library`
/// first, then - over a font memory object - a library pointer, and answers 0, any edition
/// (`arm2-create`, `arm3-zero-ed`). A null memory object answers `0x80460002` (`arm3-null-mem`).
/// The hardware draws the library's blocks through the memory object's interface; here the
/// library is a zeroed block of its measured size, owned here (D151).
pub(crate) fn font_create_library_with_edition(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (memory, out) = (args[0], args[3]);
    // SAFETY: the guest's out-parameter, written through the checked accessor.
    unsafe { guest::write_u64(out, 0) };
    if memory == 0 {
        return INVALID_ARGUMENT;
    }
    let Some(library) = zeroed_block(LIBRARY_BYTES) else {
        return u64::from(orbistoun_core::GuestError::NoMemory.as_raw());
    };
    // SAFETY: as above.
    unsafe { guest::write_u64(out, library) };
    OK
}

/// What a support call answers on a library it has already set up (`0x80460021`, measured).
const ALREADY_SUPPORTED: u64 = 0x8046_0021;
/// What a support call answers for a null library (`0x80460004`, the census's zero arguments).
const NO_LIBRARY: u64 = 0x8046_0004;
/// Where the library holds the system fonts' support object, and the external fonts'.
const SYSTEM_SUPPORT_AT: u64 = 0xa0;
const EXTERNAL_SUPPORT_AT: u64 = 0xa8;

/// Sets up one kind of font support on a library: a pointer to a support object stored at `slot`
/// and 0 answered, or `0x80460021` and nothing changed when the slot already holds one. As
/// REQ-fm04 measured on a real library (`20261009-151440-eboot.obs.log`, five bytes changed at
/// `+0xa0` or `+0xa8`, a repeat refused). The object is the library's own, a zeroed block here
/// (D151).
fn support_fonts(library: u64, slot: u64) -> u64 {
    if library == 0 {
        return NO_LIBRARY;
    }
    // SAFETY: the guest's library block, read through the checked accessor.
    match unsafe { guest::read_u64(library + slot) } {
        Some(0) => {}
        Some(_) => return ALREADY_SUPPORTED,
        None => return NO_LIBRARY,
    }
    let Some(object) = zeroed_block(LIBRARY_BYTES) else {
        return u64::from(orbistoun_core::GuestError::NoMemory.as_raw());
    };
    // SAFETY: as above, the slot just read.
    unsafe { guest::write_u64(library + slot, object) };
    OK
}

/// `sceFontSupportSystemFonts(library)`: the system fonts' support, at `+0xa0`.
pub(crate) fn font_support_system_fonts(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    support_fonts(args[0], SYSTEM_SUPPORT_AT)
}

/// `sceFontSupportExternalFonts(library, 0, 0)`: the external fonts' support, at `+0xa8`.
pub(crate) fn font_support_external_fonts(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    support_fonts(args[0], EXTERNAL_SUPPORT_AT)
}

/// `sceFontDestroyLibrary(&library)`: clears `library` and answers 0 (`arm4-destroy`). The block
/// is not reused.
pub(crate) fn font_destroy_library(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // SAFETY: the guest's library pointer, cleared through the checked accessor.
    unsafe { guest::write_u64(args[0], 0) };
    OK
}
