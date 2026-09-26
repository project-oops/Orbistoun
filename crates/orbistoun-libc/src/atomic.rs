//! The C11 atomic operations the platform's C runtime calls out to.
//!
//! The platform's C runtime ships out-of-line versions of `<stdatomic.h>` operations, named
//! `_Atomic_<operation>_<width>`, with ordinary C11 semantics and the object passed by
//! address. Each takes a memory-order argument; these use `SeqCst` throughout, which conforms
//! to every weaker order, rather than decoding an enumeration whose numbering is not
//! established here.

use std::sync::atomic::{AtomicU32, Ordering};

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};

/// The width these operate on, in bytes, as the `_4` in each name says.
const WIDTH: u64 = 4;

/// The atomic object at a guest address, if the guest gave a usable one.
///
/// C11 requires an atomic object to be suitably aligned (6.2.8). A misaligned address answers
/// [`None`] rather than forming a misaligned reference, which would be undefined here too.
fn object(address: u64) -> Option<&'static AtomicU32> {
    if address == 0 || address % WIDTH != 0 {
        return None;
    }
    // SAFETY: a guest-supplied atomic object under the identity mapping, just checked for the
    // alignment C11 requires. `AtomicU32` has the layout of `u32`, and the reference does not
    // outlive the call.
    Some(unsafe { &*(address as *const AtomicU32) })
}

/// `_Atomic_load_4(object, order)` - reads an atomic object.
///
/// Reference: ISO/IEC 9899:2011 7.17.7.2 (`atomic_load_explicit`).
fn atomic_load_4(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    object(args[0]).map_or(0, |cell| u64::from(cell.load(Ordering::SeqCst)))
}

/// `_Atomic_fetch_add_4(object, operand, order)` - adds, and answers the previous value.
///
/// Reference: ISO/IEC 9899:2011 7.17.7.5.
fn atomic_fetch_add_4(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let operand = args[1] as u32;
    object(args[0]).map_or(0, |cell| {
        u64::from(cell.fetch_add(operand, Ordering::SeqCst))
    })
}

/// `_Atomic_fetch_sub_4(object, operand, order)` - subtracts, and answers the previous value.
///
/// Reference: ISO/IEC 9899:2011 7.17.7.5.
fn atomic_fetch_sub_4(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let operand = args[1] as u32;
    object(args[0]).map_or(0, |cell| {
        u64::from(cell.fetch_sub(operand, Ordering::SeqCst))
    })
}

/// `_Atomic_compare_exchange_weak_4(object, expected, desired, success, failure)`.
///
/// Reference: ISO/IEC 9899:2011 7.17.7.4. On failure the object's actual value is written back
/// through `expected`, which lets the caller's retry loop terminate. The weak form may fail
/// spuriously, and its callers already loop.
fn atomic_compare_exchange_weak_4(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (expected_at, desired) = (args[1], args[2] as u32);
    let Some(cell) = object(args[0]) else {
        return 0;
    };
    let Some(expected_cell) = object(expected_at) else {
        return 0;
    };
    let expected = expected_cell.load(Ordering::SeqCst);
    match cell.compare_exchange_weak(expected, desired, Ordering::SeqCst, Ordering::SeqCst) {
        Ok(_) => 1,
        Err(actual) => {
            expected_cell.store(actual, Ordering::SeqCst);
            0
        }
    }
}

/// Everything here, by symbol name.
pub(crate) fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("_Atomic_load_4", atomic_load_4),
        ("_Atomic_fetch_add_4", atomic_fetch_add_4),
        ("_Atomic_fetch_sub_4", atomic_fetch_sub_4),
        (
            "_Atomic_compare_exchange_weak_4",
            atomic_compare_exchange_weak_4,
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(f: GuestFn, args: [u64; 3]) -> u64 {
        let mut full = [0_u64; GUEST_ARG_REGISTERS];
        full[..3].copy_from_slice(&args);
        f(&full)
    }

    /// A four-byte aligned object this test owns.
    fn cell(value: u32) -> AtomicU32 {
        AtomicU32::new(value)
    }

    #[test]
    fn a_load_answers_what_is_there() {
        let it = cell(7);
        let at = std::ptr::from_ref(&it) as u64;
        assert_eq!(call(atomic_load_4, [at, 0, 0]), 7);
    }

    /// Fetch-add answers the previous value, not the new one, or every ticket lock breaks.
    #[test]
    fn fetch_add_answers_the_previous_value() {
        let it = cell(10);
        let at = std::ptr::from_ref(&it) as u64;
        assert_eq!(call(atomic_fetch_add_4, [at, 5, 0]), 10, "the value before");
        assert_eq!(it.load(Ordering::SeqCst), 15, "and the object is updated");
    }

    #[test]
    fn fetch_sub_answers_the_previous_value() {
        let it = cell(10);
        let at = std::ptr::from_ref(&it) as u64;
        assert_eq!(call(atomic_fetch_sub_4, [at, 4, 0]), 10);
        assert_eq!(it.load(Ordering::SeqCst), 6);
    }

    /// A failed exchange writes the actual value back through `expected`.
    #[test]
    fn a_failed_exchange_reports_the_actual_value_back() {
        let it = cell(3);
        let expected = cell(99);
        let (at, exp_at) = (
            std::ptr::from_ref(&it) as u64,
            std::ptr::from_ref(&expected) as u64,
        );
        let mut full = [0_u64; GUEST_ARG_REGISTERS];
        full[..3].copy_from_slice(&[at, exp_at, 42]);
        assert_eq!(
            atomic_compare_exchange_weak_4(&full),
            0,
            "99 is not 3, so it must not exchange"
        );
        assert_eq!(
            expected.load(Ordering::SeqCst),
            3,
            "and the caller is told what is actually there"
        );
        assert_eq!(it.load(Ordering::SeqCst), 3, "the object is untouched");
    }

    /// The succeeding case, so the guard above is known to discriminate.
    #[test]
    fn a_matching_exchange_stores_the_desired_value() {
        let it = cell(3);
        let expected = cell(3);
        let (at, exp_at) = (
            std::ptr::from_ref(&it) as u64,
            std::ptr::from_ref(&expected) as u64,
        );
        let mut full = [0_u64; GUEST_ARG_REGISTERS];
        full[..3].copy_from_slice(&[at, exp_at, 42]);
        // The weak form may fail spuriously, so a single call is retried the way its callers do.
        let mut exchanged = 0;
        for _ in 0..1000 {
            exchanged = atomic_compare_exchange_weak_4(&full);
            if exchanged == 1 {
                break;
            }
        }
        assert_eq!(exchanged, 1);
        assert_eq!(it.load(Ordering::SeqCst), 42);
    }

    /// A misaligned object answers zero without touching memory or forming a misaligned
    /// reference.
    #[test]
    fn a_misaligned_object_is_refused_rather_than_dereferenced() {
        let backing = cell(0);
        let at = std::ptr::from_ref(&backing) as u64;
        assert!(object(at + 1).is_none(), "one byte in is not four-aligned");
        assert!(object(0).is_none(), "and null is not an object");
        assert_eq!(call(atomic_load_4, [at + 1, 0, 0]), 0);
    }
}
