//! libSceUlt mutexes as the console answers them (obSCEne `REQ-20261013T0911Z-u1t2`,
//! `000-boot/sce-ult`, FW 12.40): not recursive, with the library's own refusal codes.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};

fn implementation(name: &str) -> GuestFn {
    orbistoun_kernel::implementations()
        .iter()
        .find(|(n, _)| *n == name)
        .map_or_else(|| panic!("{name} is not implemented"), |(_, f)| *f)
}

fn call(name: &str, args: &[u64]) -> u64 {
    let mut regs = [0_u64; GUEST_ARG_REGISTERS];
    regs[..args.len()].copy_from_slice(args);
    implementation(name)(&regs)
}

/// An Ult mutex refuses its owner's second lock and a trylock while held with `0x80810006`, and an
/// unlock by a thread that does not hold it with `0x80810005`.
#[test]
fn an_ult_mutex_refuses_as_measured() {
    let mut mutex = [0_u8; 0x100];
    let at = mutex.as_mut_ptr() as u64;
    let name = b"probe_mtx\0";
    assert_eq!(
        call("_sceUltMutexCreate", &[at, name.as_ptr() as u64, 0, 0]),
        0
    );
    assert_eq!(call("sceUltMutexLock", &[at]), 0);
    assert_eq!(call("sceUltMutexLock", &[at]), 0x8081_0006, "self-relock");
    assert_eq!(call("_sceUltMutexTryLock", &[at]), 0x8081_0006, "held");
    assert_eq!(call("sceUltMutexUnlock", &[at]), 0);
    assert_eq!(call("sceUltMutexUnlock", &[at]), 0x8081_0005, "not held");
}

/// A runtime is created with no option block and with an initialised one; a zeroed block is
/// refused `0x80810004` (`u1t2` arm 1).
#[test]
fn a_runtime_option_block_needs_its_magic() {
    let name = b"sample runtime\0";
    let create = |opt: u64| {
        let mut runtime = [0_u8; 0x1000];
        call(
            "_sceUltUlthreadRuntimeCreate",
            &[
                runtime.as_mut_ptr() as u64,
                name.as_ptr() as u64,
                16,
                3,
                0,
                opt,
            ],
        )
    };
    assert_eq!(create(0), 0, "no option block");
    let zeroed = [0_u8; 0x80];
    assert_eq!(create(zeroed.as_ptr() as u64), 0x8081_0004, "zeroed");
    let mut initialised = [0_u8; 0x80];
    initialised[..12].copy_from_slice(b"ubilamtl\0cig");
    assert_eq!(create(initialised.as_ptr() as u64), 0, "initialised");
}
