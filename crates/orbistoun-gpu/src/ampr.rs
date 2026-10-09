//! `libSceAmpr` - command-buffer construction for asynchronous processing, placed beside the
//! graphics submission path by name association.
//!
//! The command-buffer object's construction, binding and reset are implemented as obSCEne measured
//! them (`reports/report-1791275954.txt` lines 8674-8876, `166-agc/ampr-apr-file-read`, REQ a5b1);
//! `sceAmprAprCommandBufferReadFile` adds its read to a queue installed from above (D782).
//!
//! Every arity is `6`, the trampoline's full capture, not a claim about how many arguments a
//! function takes: a wrong arity only degrades a trace, while a wrong name is unreachable.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError, GuestFn};
use orbistoun_hle::guest_module;
use orbistoun_mem::guest;

guest_module! {
    "libSceAmpr" {
        "sceAmprAprCommandBufferConstructor" => 6,
        // (buffer, buffer + 0x18, buffer + 0x20, id, into, length) and the offset on the stack, as
        // PPSA04263's wrapper calls it.
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

/// Adds a file read to a command buffer, installed from above because the files a resolve named
/// belong to `libkernel`'s asynchronous file path (D536): `(buffer, id, into, length, offset)` to
/// whether it was added.
type ReadQueue = fn(u64, u64, u64, u64, u64) -> bool;

/// The installed queue, if anything installed one.
static READ_QUEUE: std::sync::OnceLock<ReadQueue> = std::sync::OnceLock::new();

/// Installs what `sceAmprAprCommandBufferReadFile` adds its reads with.
pub fn on_read_file(queue: ReadQueue) {
    let _ = READ_QUEUE.set(queue);
}

/// Implementations this module provides for `libSceAmpr`.
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        (
            "sceAmprCommandBufferConstructor",
            command_buffer_constructor,
        ),
        ("sceAmprCommandBufferSetBuffer", command_buffer_set_buffer),
        ("sceAmprCommandBufferReset", command_buffer_reset),
        (
            "sceAmprAprCommandBufferConstructor",
            apr_command_buffer_constructor,
        ),
        (
            "sceAmprAprCommandBufferReadFile",
            apr_command_buffer_read_file,
        ),
    ]
}

/// `sceAmprAprCommandBufferReadFile(buffer, buffer + 0x18, buffer + 0x20, id, into, length,
/// offset)`: adds the read of `length` bytes from `offset` in the file a resolve gave the 32-bit
/// `id`, into `into`, to the command buffer, and answers 0; the submit carries it out (D782,
/// assumed: no probe on firmware 12.40 holds a real identifier). The shape is PPSA04263's wrapper,
/// which passes two fields of the object after it and the offset as a seventh argument. An
/// identifier no resolve gave, or no installed queue, answers the placeholder.
fn apr_command_buffer_read_file(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let spilled = orbistoun_thunk::stack_arguments();
    let offset = if spilled == 0 {
        0
    } else {
        // SAFETY: the dispatch published this as the word above the return address the guest's
        // call pushed, on the calling thread's live stack.
        unsafe { guest::read_u64(spilled) }.unwrap_or_default()
    };
    let id = args[3] & u64::from(u32::MAX);
    match READ_QUEUE.get() {
        Some(queue) if queue(args[0], id, args[4], args[5], offset) => 0,
        _ => u64::from(GuestError::InvalidArgument.as_raw()),
    }
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

/// `sceAmprAprCommandBufferConstructor(first, second, third, ..)`: answers the first pointer and
/// leaves it alone, and zeroes the first eight bytes of the second and third (`apr-cb-ctor-3ptr`,
/// REQ apr1, `reports/hardware/20261007-082134-eboot.obs.log` 7568-7674, measured with the rest
/// zero). PPSA25872 and PPSA21564 pass three pointers so, with other words after them. A second or
/// third that is not writable answers null, never an error code in a pointer's place.
fn apr_command_buffer_constructor(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    for at in [args[1], args[2]] {
        // SAFETY: a guest object the constructor writes the head of, through the checked accessor.
        if !unsafe { guest::write_bytes(at, &[0; 8]) } {
            return 0;
        }
    }
    args[0]
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
