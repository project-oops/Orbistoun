//! An AMD-only instruction the host lacks, carried out at its fault (D924), on purpose.
//!
//! A host that has SSE4a never faults on `extrq`, so the test plants `ud2` over a site and
//! registers the instruction it stands for, as a rehearsal does: the fault is raised for real,
//! and the vectored handler answers it from the thread's own registers. One test, because the
//! handler and the planted sites are process-wide.

#![cfg(all(windows, target_arch = "x86_64"))]

use std::arch::asm;

use orbistoun_loader::rewrite::{Field, Form, extrq, insertq, register_planted};
use orbistoun_mem::{AddressSpace, Protection};
use orbistoun_worker::report;

static RANGE: orbistoun_mem::test_bases::Range =
    orbistoun_mem::test_bases::Range::nth(orbistoun_mem::test_bases::crates::WORKER);

/// `xmm0` and `xmm1` loaded, a call to `entry`, and `xmm0` read back.
///
/// # Safety
///
/// `entry` must be code that returns with `ret`, touching no memory but its stack.
unsafe fn call(entry: u64, dst: u128, src: u128) -> u128 {
    let mut out = dst;
    let at = std::ptr::from_mut(&mut out);
    let from = std::ptr::from_ref(&src);
    // SAFETY: the caller guarantees `entry`; every register the call may change is declared.
    unsafe {
        asm!(
            "movdqu xmm0, [{a}]",
            "movdqu xmm1, [{s}]",
            "call {entry}",
            "movdqu [{a}], xmm0",
            a = in(reg) at,
            s = in(reg) from,
            entry = in(reg) entry,
            out("xmm0") _,
            out("xmm1") _,
            clobber_abi("C"),
        );
    }
    out
}

/// A planted `extrq xmm0, xmm1` and `insertq xmm0, xmm1, 8, 8` each fault, are carried out, and
/// resume past the instruction they stand for.
#[test]
fn a_faulting_sse4a_instruction_is_carried_out_and_stepped_past() {
    assert!(report::install(), "the handler is what answers the fault");
    let base = RANGE.take();
    let mut space = AddressSpace::new();
    space
        .reserve(base, 0x10000, Protection::READ_WRITE)
        .expect("reserves");
    // ud2 padded to each instruction's length, then `ret`.
    let extract = base;
    let insert = base + 0x100;
    let code: [(u64, &[u8]); 2] = [
        (extract, &[0x0f, 0x0b, 0xcc, 0xcc, 0xc3]),
        (insert, &[0x0f, 0x0b, 0xcc, 0xcc, 0xcc, 0xcc, 0xc3]),
    ];
    for (at, bytes) in code {
        // SAFETY: inside the reservation just made, which nothing else holds.
        unsafe {
            std::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                std::ptr::with_exposed_provenance_mut::<u8>(at as usize),
                bytes.len(),
            );
        }
    }
    space
        .protect(base, 0x10000, Protection::READ_EXECUTE)
        .expect("protects");
    register_planted(&[
        (
            extract,
            Form::Extrq {
                dst: 0,
                src: 1,
                field: Field::Register,
            },
            4,
        ),
        (
            insert,
            Form::Insertq {
                dst: 0,
                src: 1,
                field: Field::Immediate { len: 8, index: 8 },
            },
            6,
        ),
    ]);

    let dst = 0x1111_2222_3333_4444_u128 << 64 | 0x0123_4567_89ab_cdef;
    let control = 0x0c10; // length 16, index 12
    // SAFETY: the site is `ud2` the handler answers, then `ret`.
    let got = unsafe { call(extract, dst, control) };
    assert_eq!(got, extrq(dst, 16, 12));
    // SAFETY: as above.
    let got = unsafe { call(insert, dst, 0x5a) };
    assert_eq!(got, insertq(dst, 0x5a, 8, 8));
}
