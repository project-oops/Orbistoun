//! A file-backed `mmap` holds the file's bytes: POSIX.1-2008 `mmap(2)` maps `len` bytes of the
//! file at `fd` from `offset`, and anything past the file's end within the last page reads as zero.
//! PPSA25872's Il2Cpp runtime maps `global-metadata.dat` this way and reads it as zeros otherwise.
//! Its own test binary, because the reader is installed once for the process.

use orbistoun_core::GUEST_ARG_REGISTERS;

/// The file behind descriptor 7: byte `i` is `i % 251`, 0x3000 bytes long.
const FILE_LEN: u64 = 0x3000;

fn fake_read(fd: u64, into: &mut [u8], offset: u64) -> Option<usize> {
    if fd != 7 {
        return None;
    }
    let available = FILE_LEN.saturating_sub(offset) as usize;
    let n = into.len().min(available);
    for (i, byte) in into[..n].iter_mut().enumerate() {
        *byte = ((offset as usize + i) % 251) as u8;
    }
    Some(n)
}

fn map(len: u64, flags: u64, fd: u64, offset: u64) -> u64 {
    let mut args = [0_u64; GUEST_ARG_REGISTERS];
    args[1] = len;
    args[2] = 1; // PROT_READ
    args[3] = flags;
    args[4] = fd;
    args[5] = offset;
    orbistoun_kernel::mmap(&args)
}

#[test]
fn a_file_mapping_holds_the_file_and_an_anonymous_one_is_zero() {
    orbistoun_kernel::install_file_read(fake_read);
    let at = map(0x4000, 0x1, 7, 0x1000);
    assert_ne!(at, !0, "the map succeeds");
    // SAFETY: the mapping just made is 0x4000 readable bytes.
    let bytes = unsafe { std::slice::from_raw_parts(at as usize as *const u8, 0x4000) };
    assert_eq!(bytes[0], (0x1000 % 251) as u8, "from the offset");
    assert_eq!(
        bytes[0x1fff],
        ((0x1000 + 0x1fff) % 251) as u8,
        "to the file's end"
    );
    assert!(bytes[0x2000..].iter().all(|&b| b == 0), "zero past the end");

    // MAP_ANON (0x1000): no file, all zero.
    let anon = map(0x2000, 0x1000, !0, 0);
    // SAFETY: the mapping just made is 0x2000 readable bytes.
    let zero = unsafe { std::slice::from_raw_parts(anon as usize as *const u8, 0x2000) };
    assert!(zero.iter().all(|&b| b == 0));

    // A descriptor the reader does not know is refused rather than answered with zeros.
    assert_eq!(map(0x1000, 0x1, 9, 0), !0);
}
