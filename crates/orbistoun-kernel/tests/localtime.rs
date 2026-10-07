//! `sceKernelConvertUtcToLocaltime(utc, &local, &timesec, &dst)` in the shape the console answers
//! it (obSCEne `050-time/convert-utc-to-localtime`, sweep 20261007-202010): `0`, the local time,
//! sixteen bytes of `timesec` (`t`, `west_sec`, `dst_sec`) and four of `dst`. The zone is UTC with
//! no daylight saving (D454), so every offset is zero.

use orbistoun_core::GUEST_ARG_REGISTERS;

fn call(name: &str, args: [u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (_, handler) = orbistoun_kernel::implementations()
        .iter()
        .find(|(n, _)| *n == name)
        .unwrap_or_else(|| panic!("{name} is implemented"));
    handler(&args)
}

#[test]
fn utc_converts_to_itself_with_no_offsets() {
    // The probe's own timestamp, 2026-10-06 12:00:00 UTC.
    let utc = 1_791_288_000_u64;
    let mut local = 0xcccc_cccc_cccc_cccc_u64;
    let mut timesec = [0xcc_u8; 32];
    let mut dst = [0xcc_u8; 32];
    let answer = call(
        "sceKernelConvertUtcToLocaltime",
        [
            utc,
            std::ptr::addr_of_mut!(local) as u64,
            timesec.as_mut_ptr() as u64,
            dst.as_mut_ptr() as u64,
            0,
            0,
        ],
    );
    assert_eq!(answer, 0);
    assert_eq!(local, utc, "no offset");
    assert_eq!(
        timesec[..16],
        [0; 16],
        "t (the zone's rule has held since the epoch), west_sec, dst_sec"
    );
    assert_eq!(timesec[16..], [0xcc; 16], "sixteen bytes and no more");
    assert_eq!(dst[..4], [0; 4]);
    assert_eq!(dst[4..], [0xcc; 28], "four bytes and no more");
}
