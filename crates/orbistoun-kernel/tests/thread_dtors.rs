//! `_sceKernelSetThreadDtors`: the runner libc registers is entered once by each spawned thread
//! as it ends, on that thread, before a join returns.

use std::sync::Mutex;

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

/// The host thread names the runner was entered on, in order.
static ENTERED_ON: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Stands in for libc's destructor runner, which takes no arguments.
extern "sysv64" fn record_dtors() {
    let name = std::thread::current().name().unwrap_or("").to_owned();
    ENTERED_ON
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(name);
}

/// The thread body: returns at once.
extern "sysv64" fn body(_argument: u64) -> u64 {
    7
}

fn run_thread(name: &[u8]) -> u64 {
    let mut handle = 0_u64;
    let handle_at = std::ptr::from_mut(&mut handle).expose_provenance() as u64;
    let entry = body as extern "sysv64" fn(u64) -> u64 as usize as u64;
    assert_eq!(
        call(
            "scePthreadCreate",
            &[handle_at, 0, entry, 0, name.as_ptr() as u64]
        ),
        0
    );
    let mut exit = 0_u64;
    let exit_at = std::ptr::from_mut(&mut exit).expose_provenance() as u64;
    assert_eq!(call("scePthreadJoin", &[handle, exit_at]), 0);
    exit
}

#[test]
fn each_ending_thread_enters_the_registered_runner_once() {
    let runner = record_dtors as extern "sysv64" fn() as usize as u64;
    assert_eq!(call("_sceKernelSetThreadDtors", &[runner]), 0);

    assert_eq!(
        run_thread(b"first\0"),
        7,
        "the exit value still reaches the joiner"
    );
    assert_eq!(run_thread(b"second\0"), 7);

    let entered = ENTERED_ON
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    assert_eq!(
        entered,
        ["guest:first", "guest:second"],
        "once per thread, on the ending thread, before its join returned"
    );
}
