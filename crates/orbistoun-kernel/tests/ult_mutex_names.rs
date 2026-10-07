//! `sceUltMutexLock` and `sceUltMutexUnlock` - the exports without the leading underscore - take
//! the mutex `_sceUltMutexCreate` made, as `_sceUltMutexLock` and `_sceUltMutexUnlock` do.
//! PPSA28061 locks through them 10916 times in a run, and each answered the placeholder.

use orbistoun_core::GUEST_ARG_REGISTERS;

fn call(name: &str, args: [u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (_, handler) = orbistoun_kernel::implementations()
        .iter()
        .find(|(n, _)| *n == name)
        .unwrap_or_else(|| panic!("{name} is implemented"));
    handler(&args)
}

#[test]
fn the_plain_names_lock_and_unlock_a_created_mutex() {
    let mut object = [0_u8; 0x100];
    let mutex = object.as_mut_ptr() as usize as u64;
    let name = b"leaderboard\0";
    assert_eq!(
        call(
            "_sceUltMutexCreate",
            [mutex, name.as_ptr() as usize as u64, 0, 0, 0, 0]
        ),
        0
    );
    assert_eq!(call("sceUltMutexLock", [mutex, 0, 0, 0, 0, 0]), 0);
    assert_eq!(call("sceUltMutexUnlock", [mutex, 0, 0, 0, 0, 0]), 0);
    assert_ne!(
        call("sceUltMutexUnlock", [mutex, 0, 0, 0, 0, 0]),
        0,
        "a release with nothing held is refused"
    );
}
