//! `libSceAmpr` - command-buffer construction for asynchronous processing, placed beside the
//! graphics submission path by name association.
//!
//! The command-buffer object's construction, binding and reset are implemented as obSCEne measured
//! them (`reports/report-1791275954.txt` lines 8674-8876, `166-agc/ampr-apr-file-read`, REQ a5b1);
//! the `Apr` file-read calls are declared and not.
//!
//! Every arity is `6`, the trampoline's full capture, not a claim about how many arguments a
//! function takes: a wrong arity only degrades a trace, while a wrong name is unreachable.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError, GuestFn};
use orbistoun_hle::guest_module;
use orbistoun_mem::guest;

guest_module! {
    "libSceAmpr" {
        "sceAmprAprCommandBufferConstructor" => 6,
        "sceAmprAprCommandBufferReadFile" => 6,
        "sceAmprCommandBufferConstructor" => 6,
        "sceAmprCommandBufferReset" => 6,
        "sceAmprCommandBufferSetBuffer" => 6,
    }
}

/// How many leading bytes of the object the constructor zeroes (`ctor-obj`, `extent 24`).
const OBJECT_HEAD: usize = 24;
/// Where `sceAmprCommandBufferSetBuffer` writes the bound buffer's size, a 32-bit word.
const SIZE_AT: u64 = 12;
/// Where it writes the bound buffer's address.
const BUFFER_AT: u64 = 16;
/// The alignment every bound size measured has: 0x20 to 0x1000, each a multiple of it
/// (`ampr-sb40-size-*`, `ampr-cb-set-buffer-1000`).
const BOUND_ALIGNMENT: u64 = 0x20;
/// The largest buffer measured refused (`ampr-cb-set-buffer-10`).
const REFUSED_SIZE: u64 = 0x10;
/// What a buffer too small answers (`ampr-cb-set-buffer-10`).
const TOO_SMALL: u64 = 0x8002_0010;
/// What an odd size answers (`ampr-sb40-size-0x401`): the kernel's `EINVAL`.
const UNALIGNED: u64 = 0x8002_0016;

/// Implementations this module provides for `libSceAmpr`.
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        (
            "sceAmprCommandBufferConstructor",
            command_buffer_constructor,
        ),
        ("sceAmprCommandBufferSetBuffer", command_buffer_set_buffer),
        ("sceAmprCommandBufferReset", command_buffer_reset),
    ]
}

/// `sceAmprCommandBufferConstructor(obj, ...)`: zeroes the object's first 24 bytes and answers the
/// object, as a C++ constructor does. Measured with `(obj, obj, buf, 0, 0, 0x47d)`, PPSA21564's
/// shape: the buffer argument was left alone. An object that is not writable guest memory answers
/// null, never an error code in a pointer's place.
fn command_buffer_constructor(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let object = args[0];
    // SAFETY: the guest's own object, which it passes to be constructed in place.
    if !unsafe { guest::write_bytes(object, &[0; OBJECT_HEAD]) } {
        return 0;
    }
    object
}

/// `sceAmprCommandBufferSetBuffer(obj, buffer, size, ..)`: binds `size` bytes at `buffer` - the
/// size at +12, the address at +16 - and answers 0, for a size of at least 0x20 and a multiple of
/// it, whatever the fourth argument. A buffer of 0x10 bytes is refused with `0x80020010` and an odd
/// size above it with `0x80020016`, each changing nothing (REQ a5b1, sb40). Any other size - an even
/// one below 0x20 or not a multiple of it - is unmeasured and answers the placeholder.
fn command_buffer_set_buffer(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (object, buffer, size) = (args[0], args[1], args[2] & 0xffff_ffff);
    if size <= REFUSED_SIZE {
        return TOO_SMALL;
    }
    if size % 2 == 1 {
        return UNALIGNED;
    }
    if size % BOUND_ALIGNMENT != 0 {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    // SAFETY: the guest's own constructed object, at its measured fields.
    let bound = unsafe { guest::write_u32(object + SIZE_AT, size as u32) };
    // SAFETY: as above.
    if !bound || !unsafe { guest::write_u64(object + BUFFER_AT, buffer) } {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    0
}

/// `sceAmprCommandBufferReset(obj)`: answers 0 and, measured after a binding, leaves the object as
/// it was - the bound buffer and its size kept.
fn command_buffer_reset(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    0
}
