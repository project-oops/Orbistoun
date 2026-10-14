//! `_sceKernelSetThreadDtors`: the runner libc registers is entered once by each spawned thread
//! as it ends, on that thread, before a join returns, with the thread's exit value; a second
//! registration replaces the first (obSCEne `060-module/libc-startup-hooks`, REQ-td01 and REQ-rf02).

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

/// Each entry of a runner: which runner, the host thread it ran on, and its first argument.
static ENTERED: Mutex<Vec<(&str, String, u64)>> = Mutex::new(Vec::new());

fn record(runner: &'static str, argument: u64) {
    let name = std::thread::current().name().unwrap_or("").to_owned();
    ENTERED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push((runner, name, argument));
}

/// Stands in for libc's destructor runner.
extern "sysv64" fn first_runner(argument: u64) {
    record("first", argument);
}

/// A runner registered after the first.
extern "sysv64" fn second_runner(argument: u64) {
    record("second", argument);
}

/// The thread body: returns its argument as its exit value.
extern "sysv64" fn body(argument: u64) -> u64 {
    argument
}

fn run_thread(name: &[u8], exit_value: u64) -> u64 {
    let mut handle = 0_u64;
    let handle_at = std::ptr::from_mut(&mut handle).expose_provenance() as u64;
    let entry = body as extern "sysv64" fn(u64) -> u64 as usize as u64;
    assert_eq!(
        call(
            "scePthreadCreate",
            &[handle_at, 0, entry, exit_value, name.as_ptr() as u64]
        ),
        0
    );
    let mut exit = 0_u64;
    let exit_at = std::ptr::from_mut(&mut exit).expose_provenance() as u64;
    assert_eq!(call("scePthreadJoin", &[handle, exit_at]), 0);
    exit
}

fn runner_address(runner: extern "sysv64" fn(u64)) -> u64 {
    runner as usize as u64
}

#[test]
fn each_ending_thread_enters_the_registered_runner_once_with_its_exit_value() {
    assert_eq!(
        call("_sceKernelSetThreadDtors", &[runner_address(first_runner)]),
        0
    );
    assert_eq!(
        run_thread(b"first\0", 0x55),
        0x55,
        "the exit value still reaches the joiner"
    );
    assert_eq!(run_thread(b"second\0", 0x66), 0x66);

    // A second registration replaces the first: only the later runner is entered.
    assert_eq!(
        call("_sceKernelSetThreadDtors", &[runner_address(second_runner)]),
        0
    );
    assert_eq!(run_thread(b"third\0", 0x77), 0x77);

    let entered = ENTERED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    assert_eq!(
        entered,
        [
            ("first", "guest:first".to_owned(), 0x55),
            ("first", "guest:second".to_owned(), 0x66),
            ("second", "guest:third".to_owned(), 0x77),
        ],
        "once per thread, on the ending thread, with its exit value, before its join returned"
    );
}
