//! `libSceAmpr`'s command-buffer object, driven through the table the guest links against, as
//! obSCEne measured it: `reports/report-1791275954.txt` lines 8674-8876 (`166-agc/ampr-apr-file-read`,
//! REQ a5b1), each object `0xcc`-filled before the call.

use orbistoun_core::GUEST_ARG_REGISTERS;
use orbistoun_gpu::ampr;

/// Calls one function through the dispatch table, by the name a guest imports.
fn call(name: &str, args: [u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (_, f) = ampr::implementations()
        .iter()
        .find(|(n, _)| *n == name)
        .unwrap_or_else(|| panic!("{name} is not wired"));
    f(&args)
}

/// A `0xcc`-filled 0x200-byte object, as the probe's.
fn object() -> Box<[u8; 0x200]> {
    Box::new([0xcc; 0x200])
}

/// `ampr-cb-ctor-title`: the constructor zeroes the object's first 24 bytes, leaves the rest and
/// the buffer argument alone, and answers the object.
#[test]
fn the_constructor_zeroes_the_head_and_answers_the_object() {
    let mut obj = object();
    let buffer = object();
    let at = obj.as_mut_ptr() as u64;
    let rc = call(
        "sceAmprCommandBufferConstructor",
        [at, at, buffer.as_ptr() as u64, 0, 0, 0x47d],
    );
    assert_eq!(rc, at);
    assert!(obj[..24].iter().all(|&b| b == 0), "the first 24 bytes");
    assert!(obj[24..].iter().all(|&b| b == 0xcc), "nothing past them");
    assert!(buffer.iter().all(|&b| b == 0xcc), "the buffer untouched");
}

/// `ampr-cb-set-buffer-1000` and `-10`: a buffer of 0x1000 bytes is bound - its size as a 32-bit
/// word at +12, its address at +16 - and answers 0; one of 0x10 is refused with `0x80020010` and
/// changes nothing. `ampr-cb-reset-after-2`: a reset answers 0 and leaves the binding.
#[test]
fn set_buffer_binds_a_large_buffer_and_refuses_a_small_one() {
    let mut obj = object();
    let at = obj.as_mut_ptr() as u64;
    call("sceAmprCommandBufferConstructor", [at, at, 0, 0, 0, 0x47d]);
    let buffer = object();
    let address = buffer.as_ptr() as u64;

    let mut expected = [0xcc_u8; 0x200];
    expected[..24].fill(0);
    expected[12..16].copy_from_slice(&0x1000_u32.to_le_bytes());
    expected[16..24].copy_from_slice(&address.to_le_bytes());
    assert_eq!(
        call(
            "sceAmprCommandBufferSetBuffer",
            [at, address, 0x1000, 0, 0, 0]
        ),
        0
    );
    assert_eq!(obj[..], expected[..]);

    assert_eq!(
        call(
            "sceAmprCommandBufferSetBuffer",
            [at, address + 8, 0x10, 0, 0, 0]
        ),
        0x8002_0010
    );
    assert_eq!(obj[..], expected[..], "a refused buffer changes nothing");

    assert_eq!(call("sceAmprCommandBufferReset", [at, 0, 0, 0, 0, 0]), 0);
    assert_eq!(obj[..], expected[..], "a reset keeps the binding");
}

/// `ampr-sb40-*` (obSCEne `reports/report-1791286625.txt` 8770-9240, REQ sb40): every size from
/// 0x20 to 0x800 a multiple of 0x20 binds, PPSA21564's 0x400 among them with its non-zero `a4`;
/// 0x401 is refused with `0x80020016` and binds nothing.
#[test]
fn set_buffer_binds_aligned_sizes_and_refuses_an_odd_one() {
    for (size, a4, rc) in [
        (0x400, 1, 0),
        (0x400, 0, 0),
        (0x20, 0, 0),
        (0x40, 0, 0),
        (0x80, 0, 0),
        (0x100, 0, 0),
        (0x200, 0, 0),
        (0x800, 0, 0),
        (0x401, 0, 0x8002_0016),
    ] {
        let mut obj = object();
        let at = obj.as_mut_ptr() as u64;
        call("sceAmprCommandBufferConstructor", [at, at, 0, 0, 0, 0x47d]);
        let buffer = object();
        let address = buffer.as_ptr() as u64;
        let a4 = if a4 == 1 { address + 0x100 } else { 0 };
        assert_eq!(
            call(
                "sceAmprCommandBufferSetBuffer",
                [at, address, size, 0, a4, 0]
            ),
            rc,
            "size {size:#x}"
        );
        let mut expected = [0xcc_u8; 0x200];
        expected[..24].fill(0);
        if rc == 0 {
            expected[12..16].copy_from_slice(&(size as u32).to_le_bytes());
            expected[16..24].copy_from_slice(&address.to_le_bytes());
        }
        assert_eq!(obj[..], expected[..], "size {size:#x}");
    }
}
