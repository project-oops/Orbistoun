//! `libSceFont` - the platform's font renderer. Its font memory object is implemented as obSCEne
//! measured it in PPSA21564's own call shape (REQ-fm01,
//! `reports/hardware/20261007-082134-eboot.obs.log` lines 4339-4377,
//! `130-layout/font-memory-init`); the renderer itself is not.

use orbistoun_core::GUEST_ARG_REGISTERS;
use orbistoun_hle::guest_module;
use orbistoun_mem::guest;

guest_module! {
    "libSceFont" {
        "sceFontMemoryInit" => 6,
        "sceFontMemoryTerm" => 1,
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
