//! What one guest call costs, round trip - measured, not asserted.
//!
//! A timing depends on the machine and on what else runs, so this test is `#[ignore]`d and
//! prints; run it by name with `--ignored --nocapture`. It is its own binary because every
//! table it installs is process-global.

use orbistoun_core::{GUEST_ARG_REGISTERS, GUEST_PAGE_SIZE, GuestFn};
use orbistoun_thunk::{ThunkTable, dispatch};

type GuestCall = extern "sysv64" fn(u64, u64, u64, u64, u64, u64) -> u64;

fn callable(address: u64) -> GuestCall {
    let pointer: *const () =
        std::ptr::with_exposed_provenance(usize::try_from(address).expect("fits"));
    // SAFETY: `address` came from `ThunkTable::address_of`, the start of a stub the table wrote
    // and mapped read-execute, encoded to the System V convention this type declares.
    unsafe { std::mem::transmute::<*const (), GuestCall>(pointer) }
}

/// Far from the other integration binaries' tables.
const TABLE_BASE: u64 = 0x0000_5100_0000;

fn answer(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    args[0].wrapping_add(1)
}

/// Prints the round-trip cost of one implemented guest call.
#[test]
#[ignore = "a measurement, printed rather than asserted"]
fn one_implemented_call_round_trip() {
    let handlers: Vec<Option<GuestFn>> = vec![Some(answer)];
    dispatch::install_handlers(handlers);
    let table = ThunkTable::build(TABLE_BASE, 1, GUEST_PAGE_SIZE).expect("build the table");
    let call = callable(table.address_of(0).expect("in range"));
    let calls = 2_000_000u64;
    let started = std::time::Instant::now();
    let mut sum = 0u64;
    for i in 0..calls {
        sum = sum.wrapping_add(call(i, 0, 0, 0, 0, 0));
    }
    let each = started.elapsed().as_nanos() / u128::from(calls);
    println!("one implemented guest call: {each} ns round trip ({sum})");
}
