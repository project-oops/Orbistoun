//! An import named for a dump is dumped twice per dumped call: as the guest made the call, and as
//! the call returned, with its answer. The second shows what the implementation wrote through an
//! out-parameter, which the first cannot.
//!
//! Its own binary, because the dispatch tables are process-global and installed once.

use orbistoun_core::{GUEST_ARG_REGISTERS, GUEST_PAGE_SIZE, GuestFn};
use orbistoun_thunk::{ThunkTable, dispatch};

/// How the guest sees a thunk: an ordinary System V function of six integers.
type GuestCall = extern "sysv64" fn(u64, u64, u64, u64, u64, u64) -> u64;

/// Reinterprets a thunk address as something callable.
fn callable(address: u64) -> GuestCall {
    let pointer: *const () =
        std::ptr::with_exposed_provenance(usize::try_from(address).expect("fits"));
    // SAFETY: `address` came from `ThunkTable::address_of`: the start of a stub the table wrote
    // and mapped read-execute, encoded to the System V convention this type declares. The table
    // outlives the call.
    unsafe { std::mem::transmute::<*const (), GuestCall>(pointer) }
}

/// A base in this crate's test range (D324), apart from the other integration binaries' tables.
const TABLE_BASE: u64 = orbistoun_mem::test_bases::FIRST_RANGE
    + orbistoun_mem::test_bases::crates::THUNK * orbistoun_mem::test_bases::RANGE_STRIDE
    + 0x18_0000_0000;

/// The named import, implemented.
const NAMED: usize = 0;
/// An implemented import nobody named.
const UNNAMED: usize = 1;
const IMPORTS: usize = 2;

/// What the named import answers.
const ANSWER: u64 = 0x77;
/// What it writes through its out-parameter.
const WRITTEN: u32 = 0x4433_2211;

/// Fills its out-parameter, as a status call does.
fn fills_out(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let out: *mut u32 = std::ptr::with_exposed_provenance_mut(args[0] as usize);
    // SAFETY: the test passes the address of a buffer it owns and keeps alive across the call.
    unsafe { out.write_unaligned(WRITTEN) };
    ANSWER
}

#[test]
fn a_named_import_is_dumped_again_as_it_returns() {
    let mut out = vec![0xcc_u8; 64];
    let out_at = out.as_mut_ptr().expose_provenance() as u64;

    let mut handlers: Vec<Option<GuestFn>> = vec![None; IMPORTS];
    handlers[NAMED] = Some(fills_out);
    handlers[UNNAMED] = Some(fills_out);
    dispatch::install_handlers(handlers);
    dispatch::install_readable_ranges(vec![(out_at, out.len() as u64)]);
    let mut forced = vec![false; IMPORTS];
    forced[NAMED] = true;
    dispatch::install_forced_dumps(forced);

    let table = ThunkTable::build(TABLE_BASE, IMPORTS, GUEST_PAGE_SIZE).expect("build the table");
    let at = |index: usize| callable(table.address_of(index).expect("in range"));

    assert_eq!(at(NAMED)(out_at, 5, 0, 0, 0, 0), ANSWER);
    assert_eq!(at(UNNAMED)(out_at, 5, 0, 0, 0, 0), ANSWER);

    let dumps = dispatch::argument_dumps();
    let pointer = |returned: bool| {
        dumps
            .iter()
            .find(|d| d.index as usize == NAMED && d.slot == 0 && d.returned.is_some() == returned)
            .unwrap_or_else(|| panic!("no dump of argument 0 with returned {returned}"))
    };

    let before = pointer(false);
    assert_eq!(before.address, out_at);
    assert_eq!(
        before.bytes[..4],
        [0xcc; 4],
        "at the call, the guest's bytes"
    );

    let after = pointer(true);
    assert_eq!(after.address, out_at, "the same argument");
    assert_eq!(
        after.returned,
        Some(ANSWER),
        "beside what the call answered"
    );
    assert_eq!(
        after.bytes[..4],
        WRITTEN.to_le_bytes(),
        "on return, what the implementation wrote there"
    );
    assert_eq!(after.bytes[4..8], [0xcc; 4], "and nothing past it");

    assert!(
        dumps.iter().all(|d| d.index as usize != UNNAMED),
        "an implemented import nobody named is not dumped at all"
    );
}
