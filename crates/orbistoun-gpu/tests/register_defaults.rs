//! `sceAgcGetRegisterDefaults2` and `...Internal` answer the descriptors hardware answers, walked the
//! way PPSA02664 walks them (image+0x3dc26): the count at +0x38, the records at +0x30, and for a
//! record's `loc`, table `loc & 3`, slot `(loc & 0x3fc) / 4`, then the bytes at the slot's entry.
//! Expected values are rows of obSCEne sweep 20260927-185507, check
//! `166-agc/register-defaults-walk-4e9b`.

use orbistoun_core::GUEST_ARG_REGISTERS;

fn call(name: &str, version: u64) -> u64 {
    let (_, f) = orbistoun_gpu::agc::implementations()
        .iter()
        .find(|(n, _)| *n == name)
        .unwrap_or_else(|| panic!("{name} is implemented"));
    let mut args = [0_u64; GUEST_ARG_REGISTERS];
    args[0] = version;
    f(&args)
}

/// `length` bytes at `address`, which the answer's own memory holds.
fn bytes(address: u64, length: usize) -> Vec<u8> {
    assert_ne!(address, 0, "a null read");
    // SAFETY: every address read here is one the descriptor under test hands out.
    unsafe { std::slice::from_raw_parts(address as *const u8, length) }.to_vec()
}

fn u32_at(address: u64) -> u32 {
    u32::from_le_bytes(bytes(address, 4).try_into().expect("four bytes"))
}

fn u64_at(address: u64) -> u64 {
    u64::from_le_bytes(bytes(address, 8).try_into().expect("eight bytes"))
}

/// Record `index`'s id and the 16 bytes a title reads for it.
fn record(descriptor: u64, index: u64) -> (u32, Vec<u8>) {
    let record = u64_at(descriptor + 0x30) + index * 12;
    let loc = u32_at(record + 4);
    let table = u64_at(descriptor + u64::from(loc & 3) * 8);
    let entry = u64_at(table + u64::from(loc & 0x3fc) * 2);
    (u32_at(record), bytes(entry, 16))
}

#[test]
fn version_8_walks_to_the_measured_values() {
    let descriptor = call("sceAgcGetRegisterDefaults2", 8);
    // `defaults2-ver-0x8`: count 0x7f, table lengths (0x1e9, 0x9f, 0x37, 0), the fourth table null.
    assert_eq!(u32_at(descriptor + 0x38), 0x7f);
    let lengths: Vec<u32> = (0..4).map(|t| u32_at(descriptor + 0x20 + t * 4)).collect();
    assert_eq!(lengths, [0x1e9, 0x9f, 0x37, 0]);
    assert_eq!(u64_at(descriptor + 0x18), 0, "no fourth table");
    // `defaults2-ver-0x8-rec126-id0x36ac8a6`: loc 0xc044e, entry-16b.
    let (id, value) = record(descriptor, 126);
    assert_eq!(id, 0x036a_c8a6);
    assert_eq!(value, bytes_of("5c020000000000005d02000000000000"));
}

#[test]
fn version_0xd_starts_with_the_record_a_title_searches_for() {
    let descriptor = call("sceAgcGetRegisterDefaults2", 0xd);
    assert_eq!(u32_at(descriptor + 0x38), 0x89);
    // The first record is 0xe24f806d, the id PPSA28061 searches this descriptor for.
    assert_eq!(record(descriptor, 0).0, 0xe24f_806d);
    // A second call answers the same descriptor, as the library's static one is.
    assert_eq!(call("sceAgcGetRegisterDefaults2", 0xd), descriptor);
}

#[test]
fn the_inner_variant_has_all_four_tables_for_0xd() {
    let descriptor = call("sceAgcGetRegisterDefaults2Internal", 0xd);
    assert_eq!(u32_at(descriptor + 0x38), 0x18);
    for table in 0..4 {
        assert_ne!(u64_at(descriptor + table * 8), 0, "table {table}");
    }
}

/// A version no probe walked is refused by name, not answered with another version's defaults.
#[test]
fn an_unmeasured_version_is_refused() {
    assert_eq!(
        call("sceAgcGetRegisterDefaults2", 0xe),
        u64::from(orbistoun_core::GuestError::Unimplemented.as_raw())
    );
}

fn bytes_of(hex: &str) -> Vec<u8> {
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("hex"))
        .collect()
}
