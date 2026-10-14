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
        // (obj)
        "sceAmprCommandBufferGetCurrentOffset" => 1,
        "sceAmprCommandBufferReset" => 6,
        "sceAmprCommandBufferSetBuffer" => 6,
        // (obj, address, value, 0), as PPSA04263's wrapper calls it.
        "sceAmprCommandBufferWriteAddress_04_00" => 6,
        // Known by NID only: answers the kernel's EINVAL and writes nothing, with every argument a
        // buffer and with every argument zero (obSCEne census, `200-census/libSceAmpr/0xb5866f7aedc94e43`).
        "0xb5866f7aedc94e43" => 6,
    }
}

/// libSceAmpr's NID `0xb5866f7aedc94e43`, which PPSA28061 calls once: answers [`UNALIGNED`], the
/// kernel's `EINVAL`, and writes nothing, with every argument a buffer and with every argument zero,
/// as obSCEne's census measured it (`200-census/libSceAmpr/0xb5866f7aedc94e43`, sweep
/// 20261007-113500).
fn census_invalid(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    UNALIGNED
}

/// How many leading bytes of the object the constructor zeroes (`ctor-obj`, `extent 24`).
const OBJECT_HEAD: usize = 24;
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
        ("0xb5866f7aedc94e43", census_invalid),
        (
            "sceAmprCommandBufferConstructor",
            command_buffer_constructor,
        ),
        ("sceAmprCommandBufferSetBuffer", command_buffer_set_buffer),
        (
            "sceAmprCommandBufferWriteAddress_04_00",
            command_buffer_write_address,
        ),
        ("sceAmprCommandBufferReset", command_buffer_reset),
        (
            "sceAmprCommandBufferGetCurrentOffset",
            command_buffer_get_current_offset,
        ),
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

// The command format and the object's counters are the kernel's, which carries the commands out
// on submit (D834).
use orbistoun_kernel::apr::{BUFFER_AT, COUNT_AT, SIZE_AT, USED_AT, write_address_command};

/// `sceAmprCommandBufferWriteAddress_04_00(obj, address, value, ..)`: appends the command that
/// writes `value` to `address` when the buffer runs - nothing is written to `address` now - and
/// answers 0, the object's used bytes (+4) and command count (+8) moved on; an `address` not
/// 8-aligned is refused with `0x80020016`, nothing appended (REQ-aw01, REQ-aw02). A command that
/// would not fit the bound buffer is unmeasured and answers the placeholder.
fn command_buffer_write_address(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (object, address, value) = (args[0], args[1], args[2] as u32);
    if address % 8 != 0 {
        return UNALIGNED;
    }
    let placeholder = u64::from(GuestError::Unimplemented.as_raw());
    // SAFETY: the guest's own constructed object, at its measured fields.
    let used = unsafe { guest::read_u32(object + USED_AT) };
    // SAFETY: as above.
    let count = unsafe { guest::read_u32(object + COUNT_AT) };
    // SAFETY: as above.
    let size = unsafe { guest::read_u32(object + SIZE_AT) };
    // SAFETY: as above.
    let buffer = unsafe { guest::read_u64(object + BUFFER_AT) };
    let (Some(used), Some(count), Some(size), Some(buffer)) = (used, count, size, buffer) else {
        return placeholder;
    };
    let bytes: Vec<u8> = write_address_command(address, value)
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect();
    let Ok(length) = u32::try_from(bytes.len()) else {
        return placeholder;
    };
    if buffer == 0 || used.saturating_add(length) > size {
        return placeholder;
    }
    // SAFETY: the bound buffer, `size` bytes, of which the command takes the next `length`.
    let appended = unsafe { guest::write_bytes(buffer + u64::from(used), &bytes) };
    // SAFETY: the object's measured counters.
    let counted = appended && unsafe { guest::write_u32(object + USED_AT, used + length) };
    // SAFETY: as above.
    if !counted || !unsafe { guest::write_u32(object + COUNT_AT, count + 1) } {
        return placeholder;
    }
    0
}

/// `sceAmprCommandBufferGetCurrentOffset(obj)`: the bytes the bound buffer's commands take, the
/// object's +4 - 0 on a freshly bound buffer, 0xc after one write-address of 0x12345678
/// (REQ-aw03, `000-boot/ampr-command-buffer-queries`). Measured as a 32-bit answer; the high half
/// of `rax` is answered zero. An object that cannot be read answers 0, which PPSA04263 traps on
/// rather than reading as a size.
fn command_buffer_get_current_offset(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // SAFETY: the guest's own constructed object, at its measured field.
    unsafe { guest::read_u32(args[0] + USED_AT) }.map_or(0, u64::from)
}

/// `sceAmprCommandBufferReset(obj)`: answers 0 and, measured after a binding, leaves the object as
/// it was - the bound buffer and its size kept. With commands in the buffer it rewinds it: the
/// bytes used (+4) and the command count (+8) to zero, so the next command is written from the
/// buffer's start (D877, guest-observed: PPSA04263 resets a buffer it filled and writes again,
/// where an unrewound one overflows). An object that cannot be written answers the placeholder.
fn command_buffer_reset(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let object = args[0];
    // SAFETY: the guest's own constructed object, at its measured fields.
    let used = unsafe { guest::write_u32(object + USED_AT, 0) };
    // SAFETY: as above.
    if !used || !unsafe { guest::write_u32(object + COUNT_AT, 0) } {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    0
}

#[cfg(test)]
mod tests {
    /// The NID-only call links to the NID the guest imports and answers what the census measured,
    /// whatever it is passed.
    #[test]
    fn the_census_nid_links_and_answers_as_measured() {
        let mut registry = orbistoun_hle::Registry::new(
            orbistoun_nid::NidHasher::new(*b"test-suffix"),
            orbistoun_hle::StubPolicy::default(),
        );
        registry.register(super::MODULE);
        assert!(
            registry
                .resolve(orbistoun_nid::Nid::from_raw(0xb586_6f7a_edc9_4e43))
                .is_some()
        );
        let (_, answer) = super::implementations()
            .iter()
            .find(|(name, _)| *name == "0xb5866f7aedc94e43")
            .expect("implemented");
        assert_eq!(
            answer(&[0; orbistoun_core::GUEST_ARG_REGISTERS]),
            0x8002_0016
        );
        assert_eq!(
            answer(&[0xa1; orbistoun_core::GUEST_ARG_REGISTERS]),
            0x8002_0016
        );
    }

    /// The write-address command as a console appended it (REQ-aw01, REQ-aw02,
    /// `report-1791833977.txt` 658-693): a header word - `0x05`, a flags byte, the address's bits
    /// 47:32 - the address's low word, and the value shifted down two where that is not zero, its
    /// low two bits in the flags; the object's used bytes at +4 and count at +8 move on. A slot that
    /// is not 8-aligned is refused with `0x80020016`, nothing appended.
    #[test]
    fn a_write_address_command_is_appended_as_the_console_appends_it() {
        use orbistoun_core::GUEST_ARG_REGISTERS;
        let mut buffer = [0xcc_u8; 0x40];
        let mut object = [0_u8; 0x40];
        object[12..16].copy_from_slice(&0x40_u32.to_le_bytes());
        object[16..24].copy_from_slice(&(buffer.as_mut_ptr() as u64).to_le_bytes());
        let obj = object.as_mut_ptr() as u64;
        let write = |address: u64, value: u64| {
            let mut args = [0_u64; GUEST_ARG_REGISTERS];
            args[..4].copy_from_slice(&[obj, address, value, 0]);
            super::command_buffer_write_address(&args)
        };
        assert_eq!(write(0x1_916f40, 0x1234_5678), 0);
        assert_eq!(write(0x7_eeff_95b0, 0x8765_4321), 0);
        assert_eq!(write(0x7_eeff_95b0, 3), 0);
        assert_eq!(write(0x7_eeff_954c, 0x8765_4321), 0x8002_0016, "4-aligned");
        let words: Vec<u32> = buffer[..0x24]
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        assert_eq!(
            words,
            [
                0x0000_4205,
                0x0191_6f40,
                0x048d_159e,
                0x0007_5205,
                0xeeff_95b0,
                0x21d9_50c8,
                0x0007_3105,
                0xeeff_95b0,
                0xcccc_cccc
            ]
        );
        let word = |at: usize| u32::from_le_bytes(object[at..at + 4].try_into().expect("four"));
        assert_eq!((word(4), word(8)), (0x20, 3), "used bytes and commands");
    }

    /// The current offset is 0 on a freshly bound buffer and 0xc after one write-address of
    /// 0x12345678, as the console answered (REQ-aw03).
    #[test]
    fn the_current_offset_is_the_bytes_the_commands_take() {
        use orbistoun_core::GUEST_ARG_REGISTERS;
        let mut buffer = [0xcc_u8; 0x1000];
        let mut object = [0xa5_u8; 0x40];
        let obj = object.as_mut_ptr() as u64;
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = obj;
        assert_eq!(super::command_buffer_constructor(&args), obj);
        args[..3].copy_from_slice(&[obj, buffer.as_mut_ptr() as u64, 0x1000]);
        assert_eq!(super::command_buffer_set_buffer(&args), 0);
        args[1..3].copy_from_slice(&[0, 0]);
        assert_eq!(super::command_buffer_get_current_offset(&args), 0);
        let mut slot = [0xcc_u8; 8];
        args[..4].copy_from_slice(&[obj, slot.as_mut_ptr() as u64, 0x1234_5678, 0]);
        assert_eq!(super::command_buffer_write_address(&args), 0);
        assert_eq!(super::command_buffer_get_current_offset(&args), 0xc);
    }
}
