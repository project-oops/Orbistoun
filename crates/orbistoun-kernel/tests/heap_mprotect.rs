//! `sceKernelMprotect` on heap memory, through the re-protection the layer above installs. Its own
//! test binary, since the installed hook is the process's.

use orbistoun_core::GUEST_ARG_REGISTERS;

/// The one range the stand-in heap owns.
const HEAP_PAGE: u64 = 0x1_2340_0000;

fn heap(address: u64, length: u64, _protection: orbistoun_mem::Protection) -> Option<bool> {
    (address == HEAP_PAGE && length == 0x4000).then_some(true)
}

fn mprotect(address: u64, length: u64, prot: u64) -> u64 {
    let (_, f) = orbistoun_kernel::implementations()
        .iter()
        .find(|(n, _)| *n == "sceKernelMprotect")
        .expect("sceKernelMprotect is implemented");
    let mut args = [0_u64; GUEST_ARG_REGISTERS];
    args[..3].copy_from_slice(&[address, length, prot]);
    f(&args)
}

/// A range the heap owns is re-protected and answers success; a range nothing covers is still
/// `EINVAL`, as before.
#[test]
fn heap_memory_is_re_protected_through_the_installed_heap() {
    const EINVAL: u64 = 0x8002_0016;
    assert_eq!(
        mprotect(HEAP_PAGE, 0x4000, 0),
        EINVAL,
        "no heap installed yet"
    );
    orbistoun_kernel::install_heap_protect(heap);
    assert_eq!(mprotect(HEAP_PAGE, 0x4000, 0), 0);
    assert_eq!(mprotect(HEAP_PAGE + 0x10_0000, 0x4000, 0), EINVAL);
}
