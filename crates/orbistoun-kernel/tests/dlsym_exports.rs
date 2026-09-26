//! `sceKernelDlsym` answers for a symbol the guest's own binary exports (D517).
//!
//! The hash suffix the kernel resolves against is a `OnceLock` set once per process, first
//! setter wins, so this test has a binary of its own.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};

/// A suffix of this test's own, so the test's hash and the kernel's agree because they share
/// it, not because both use the shipped one.
const SUFFIX: &[u8] = b"a-test-suffix-for-hashing";

/// Where the pretend export lives. Nothing is executed, so any non-zero address that cannot be
/// mistaken for a real answer will do.
const EXPORT_AT: u64 = 0x0000_1234_5678_0000;

fn implementation(name: &str) -> GuestFn {
    orbistoun_kernel::implementations()
        .iter()
        .find(|(n, _)| *n == name)
        .map_or_else(
            || panic!("{name} is not implemented, so no guest can reach it"),
            |(_, f)| *f,
        )
}

/// A NUL-terminated guest string at a real address.
struct Text {
    /// Never read, and required: it owns the bytes `at` points into.
    _storage: Vec<u8>,
    at: u64,
}

impl Text {
    fn new(text: &str) -> Self {
        let mut storage = text.as_bytes().to_vec();
        storage.push(0);
        let at = storage.as_mut_ptr().expose_provenance() as u64;
        Self {
            _storage: storage,
            at,
        }
    }
}

/// A name the guest's own binary exports resolves; one nothing exports still does not.
///
/// This asserts the lookup and the out-parameter write, not that the address is executable:
/// no module is placed and the value is a marker. The negative half catches a lookup that
/// answers everything.
#[test]
fn a_symbol_the_guest_exports_resolves_and_one_nothing_exports_does_not() {
    let nid = orbistoun_nid::NidHasher::new(SUFFIX.to_vec())
        .hash("aTitlePrivateSymbol")
        .as_raw();
    orbistoun_kernel::note_guest_exports(SUFFIX, &[(nid, EXPORT_AT)]);

    let dlsym = implementation("sceKernelDlsym");
    let name = Text::new("aTitlePrivateSymbol");
    let mut out = 0_u64;
    let mut regs = [0_u64; GUEST_ARG_REGISTERS];
    regs[0] = 0; // a module handle: any non-negative one, since the name decides the answer
    regs[1] = name.at;
    regs[2] = std::ptr::from_mut(&mut out) as usize as u64;

    assert_eq!(
        dlsym(&regs),
        0,
        concat!(
            "a name the guest's own binary exports must resolve - it was answered ",
            "`Unimplemented` before, which told a title its own symbol did not exist"
        )
    );
    assert_eq!(
        out, EXPORT_AT,
        concat!(
            "and the address is written through the out-parameter, which is where the caller ",
            "reads it"
        )
    );

    let absent = Text::new("aNameNothingExports");
    let mut nothing = 0_u64;
    let mut regs = [0_u64; GUEST_ARG_REGISTERS];
    regs[1] = absent.at;
    regs[2] = std::ptr::from_mut(&mut nothing) as usize as u64;
    assert_ne!(
        dlsym(&regs),
        0,
        concat!(
            "a name nothing exports is still refused - a lookup that answers everything passes ",
            "the half above and is worse than no lookup"
        )
    );
    assert_eq!(
        nothing, 0,
        "and nothing is written through the out-parameter when there is no answer"
    );
    drop(name);
    drop(absent);
}
