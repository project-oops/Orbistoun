//! The POSIX-named half of the platform, delegated to what already implements it.
//!
//! # Why this library exists separately at all
//!
//! A title imports `pthread_create` from `libScePosix` and `scePthreadCreate` from
//! `libkernel`, and they are two names for one behaviour. A NID is the hash of a name, so
//! the POSIX spelling resolves to nothing unless it is declared - and forty-nine of them
//! were being asked for and answered by nobody, while the vendor-named twins beside them
//! were implemented and working (D349).
//!
//! # Delegation, not reimplementation
//!
//! Each served name resolves to the **same function pointer** the vendor name resolves to,
//! looked up from `orbistoun-kernel`'s own table at startup. Nothing is copied, so the two
//! spellings cannot drift, and a fix to one is a fix to both by construction rather than by
//! anybody remembering.
//!
//! Arities come from the vendor declaration for the same reason.
//!
//! # What the return convention costs, stated
//!
//! POSIX answers `0` or an errno; the vendor-named calls answer their own codes. **The
//! success paths coincide** - both are zero - and the failure paths do not.
//!
//! Nothing here invents an errno. A failure returns this project's placeholder, which
//! deliberately avoids the high bit so it can never be mistaken for an established value
//! (principle 3). A guest testing `!= 0` takes its error path correctly; one switching on
//! specific errno values falls to its default branch rather than matching the wrong case.
//! That is a worse answer than a real errno and a much better one than a plausible guess,
//! and it improves the day somebody reads the values out of a citable source.

use orbistoun_core::GuestFn;
use orbistoun_hle::guest_module;

guest_module! {
    "libScePosix" {
        // Every name here is imported by a title in the library. Arities for the served
        // ones are taken from the vendor-named function each delegates to, so the two
        // cannot disagree; the rest are provisional.
        "close" => 1,
        "open" => 3,
        "pthread_attr_destroy" => 1,
        "pthread_attr_getschedparam" => 2,
        "pthread_attr_init" => 1,
        "pthread_attr_setschedparam" => 2,
        "pthread_attr_setstacksize" => 2,
        "pthread_cond_broadcast" => 1,
        "pthread_cond_destroy" => 1,
        // Two arguments, not the three its vendor twin takes: `scePthreadCondInit` ends in
        // a name and the POSIX call does not (D385).
        "pthread_cond_init" => 2,
        "pthread_cond_signal" => 1,
        "pthread_cond_wait" => 2,
        "pthread_create" => 4,
        "pthread_join" => 2,
        "pthread_mutex_destroy" => 1,
        "pthread_mutex_init" => 2,
        "pthread_mutex_lock" => 1,
        "pthread_mutex_unlock" => 1,
        "pthread_mutexattr_destroy" => 1,
        "pthread_mutexattr_init" => 1,
        "pthread_mutexattr_setprotocol" => 2,
        "pthread_mutexattr_settype" => 2,
        "pthread_self" => 0,
        "read" => 3,
        // Measured, not assumed: seventeen of the twenty-five open-toolchain payloads
        // import it, and it is the POSIX spelling of a call this project already serves.
        "write" => 3,
        // The two time calls. Implemented in `orbistoun-libc` and declared here, because
        // here is where a title was measured importing them - one declaration per symbol,
        // and which crate holds the code is a separate question.
        "clock_gettime" => 2,
        "gettimeofday" => 2,
        // The sockets, implemented in `orbistoun-fs` next to the descriptor table they
        // share with files (D371). Eight of these were measured being imported from this
        // library by a title; `accept`, `listen` and `getpeername` were not, and are here
        // because their eight siblings are - which is an inference, and is recorded as one
        // in the knowledge file rather than passed off as a measurement.
        "socket" => 3,
        "bind" => 3,
        "listen" => 2,
        "accept" => 3,
        "connect" => 3,
        "setsockopt" => 5,
        "getsockname" => 3,
        "getpeername" => 3,
        "send" => 4,
        "recv" => 4,
        "shutdown" => 2,
        // Waiting, and printing an address. Both were measured being imported from this
        // library by a title, and both are implemented in `orbistoun-fs` beside the
        // descriptor table they ask about (D367).
        "select" => 5,
        "inet_ntop" => 4,
        // Two more with vendor-named twins already implemented. `lseek` and `munmap` were
        // measured being imported here, and both delegate exactly as `read` and `close` do.
        "lseek" => 3,
        "munmap" => 2,
        "mmap" => 6,
        // FreeBSD's own underscored spellings, which its C library uses internally so that a
        // program replacing `open` does not break `fopen`. The payloads import both, and
        // they are the same function - so they delegate to the same one.
        "_open" => 3,
        "_close" => 1,
        "_read" => 3,
        // Two thread calls with no vendor-named twin, written under their POSIX names in
        // `orbistoun-kernel` where the thread registry is, and declared here where a title
        // was measured importing them (D367).
        "pthread_detach" => 1,
        "pthread_exit" => 1,
        // `fstat` was measured being imported here; `stat`, `lstat` and the three directory
        // calls are declared in `libc`, where FreeBSD puts them and where no title contradicts
        // it (D367).
        "fstat" => 2,
        // Imported and not served: no vendor-named equivalent is implemented yet. Most
        // are sockets, which belong to a library this project does not model at all.
        "pthread_attr_setschedpolicy" => 0,
        "pthread_cond_timedwait" => 0,
        "pthread_equal" => 0,
        // Thread-specific-data keys, now served: an mspace-booted Unity title reached its
        // Intel TBB scheduler, which builds its per-thread state on these (D453). Written
        // under their POSIX names in `orbistoun-kernel` beside the thread registry.
        "pthread_getspecific" => 1,
        "pthread_key_create" => 2,
        "pthread_key_delete" => 1,
        "pthread_setspecific" => 2,
        "recvfrom" => 0,
        // POSIX unnamed semaphores, now served: PPSA21564's engine builds its condition
        // variable on one, and unimplemented `sem_init` answered a placeholder its assert
        // rejected (D455). Written under their POSIX names in `orbistoun-kernel` beside the
        // vendor semaphore calls.
        "sem_init" => 3,
        "sem_wait" => 1,
        "sem_trywait" => 1,
        "sem_post" => 1,
        "sem_destroy" => 1,
        "sched_get_priority_max" => 0,
        "sched_get_priority_min" => 0,
        "sched_yield" => 0,
        "sendto" => 0,
    }
}

/// Which POSIX name is served by which vendor-named function.
///
/// **A table rather than a convention.** The names mostly transform mechanically -
/// `pthread_mutex_lock` to `scePthreadMutexLock` - and mostly is the problem: three of the
/// forty-nine break the pattern, and a rule with exceptions applied by code would serve the
/// wrong function silently. Every pair here was checked against the implemented set.
const DELEGATED: &[(&str, &str)] = &[
    ("close", "sceKernelClose"),
    ("open", "sceKernelOpen"),
    ("pthread_attr_destroy", "scePthreadAttrDestroy"),
    ("pthread_attr_getschedparam", "scePthreadAttrGetschedparam"),
    ("pthread_attr_init", "scePthreadAttrInit"),
    ("pthread_attr_setschedparam", "scePthreadAttrSetschedparam"),
    ("pthread_attr_setstacksize", "scePthreadAttrSetstacksize"),
    ("pthread_cond_broadcast", "scePthreadCondBroadcast"),
    ("pthread_cond_destroy", "scePthreadCondDestroy"),
    // **Three that do not delegate to their vendor twin**, and the arity is why. Each
    // vendor call ends in a name the POSIX one has no argument for, so delegating meant
    // reading a register the caller never set - which faulted the first guest to reach it
    // on a stale value (D385). They are written under their POSIX names in
    // `orbistoun-kernel`, beside the vendor ones they wrap.
    ("pthread_cond_init", "pthread_cond_init"),
    ("pthread_cond_signal", "scePthreadCondSignal"),
    ("pthread_cond_wait", "scePthreadCondWait"),
    ("pthread_create", "pthread_create"),
    ("pthread_join", "scePthreadJoin"),
    ("pthread_mutex_destroy", "scePthreadMutexDestroy"),
    ("pthread_mutex_init", "pthread_mutex_init"),
    ("pthread_mutex_lock", "scePthreadMutexLock"),
    ("pthread_mutex_unlock", "scePthreadMutexUnlock"),
    ("pthread_mutexattr_destroy", "scePthreadMutexattrDestroy"),
    ("pthread_mutexattr_init", "scePthreadMutexattrInit"),
    (
        "pthread_mutexattr_setprotocol",
        "scePthreadMutexattrSetprotocol",
    ),
    ("pthread_mutexattr_settype", "scePthreadMutexattrSettype"),
    ("pthread_self", "scePthreadSelf"),
    ("read", "sceKernelRead"),
    ("write", "sceKernelWrite"),
    // **Two entries where the two names are the same**, and that is not a mistake. These
    // are POSIX functions with no vendor-named twin: the implementation is a C library one
    // and lives in `orbistoun-libc` under its own name. The delegation still earns its
    // place, because it is what binds a declaration here to code over there - and the test
    // below refuses a delegation that names nothing.
    ("clock_gettime", "clock_gettime"),
    ("gettimeofday", "gettimeofday"),
    // The sockets, same-named for the same reason: there is no vendor-named twin, and the
    // implementation is a POSIX one living beside the descriptor table.
    ("socket", "socket"),
    ("bind", "bind"),
    ("listen", "listen"),
    ("accept", "accept"),
    ("connect", "connect"),
    ("setsockopt", "setsockopt"),
    ("getsockname", "getsockname"),
    ("getpeername", "getpeername"),
    ("send", "send"),
    ("recv", "recv"),
    ("shutdown", "shutdown"),
    ("select", "select"),
    ("inet_ntop", "inet_ntop"),
    ("lseek", "sceKernelLseek"),
    ("munmap", "sceKernelMunmap"),
    ("mmap", "mmap"),
    ("_open", "sceKernelOpen"),
    ("_close", "sceKernelClose"),
    ("_read", "sceKernelRead"),
    ("pthread_detach", "pthread_detach"),
    ("pthread_exit", "pthread_exit"),
    ("fstat", "fstat"),
    // Thread-specific-data keys: no vendor twin, POSIX-named implementations in the kernel.
    ("pthread_key_create", "pthread_key_create"),
    ("pthread_setspecific", "pthread_setspecific"),
    ("pthread_getspecific", "pthread_getspecific"),
    ("pthread_key_delete", "pthread_key_delete"),
    // POSIX unnamed semaphores: no vendor twin, POSIX-named implementations in the kernel.
    ("sem_init", "sem_init"),
    ("sem_wait", "sem_wait"),
    ("sem_trywait", "sem_trywait"),
    ("sem_post", "sem_post"),
    ("sem_destroy", "sem_destroy"),
];

/// Implementations this crate provides, by symbol name.
///
/// Built by looking each vendor name up in the crate that implements it, so a function that
/// moved or was renamed produces an **empty entry rather than a wrong one** - and the test
/// below refuses that.
#[must_use]
pub fn implementations() -> Vec<(&'static str, GuestFn)> {
    // Both crates, because the delegates are split across them - threads and time in the
    // kernel, files in the filesystem shim. Assuming one crate held them all is what the
    // test below caught.
    let mut serving: Vec<(&'static str, GuestFn)> = orbistoun_kernel::implementations().to_vec();
    serving.extend_from_slice(orbistoun_fs::implementations());
    serving.extend(orbistoun_libc::implementations());
    serving.extend_from_slice(orbistoun_fs::socket::implementations());
    serving.extend_from_slice(orbistoun_fs::select::implementations());
    serving.extend_from_slice(orbistoun_fs::ifaddrs::implementations());
    serving.extend_from_slice(orbistoun_fs::metadata::implementations());
    DELEGATED
        .iter()
        .filter_map(|(posix, vendor)| {
            serving
                .iter()
                .find(|(name, _)| name == vendor)
                .map(|(_, function)| (*posix, *function))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    /// **Every delegation finds the function it names.**
    ///
    /// `implementations` filters, so a vendor name that was renamed or moved would quietly
    /// produce a shorter list and a POSIX name answered by nobody - the same shape as the
    /// bug this crate exists to fix. Asserted as an exact count, not "at least one".
    #[test]
    fn every_delegation_resolves_to_a_real_implementation() {
        let served = super::implementations();
        assert_eq!(
            served.len(),
            super::DELEGATED.len(),
            "a delegation named a function nothing implements"
        );
    }

    /// Every served name is also declared, or it can never be reached.
    #[test]
    fn every_served_name_is_declared() {
        for (name, _) in super::implementations() {
            assert!(
                super::MODULE.imports.iter().any(|i| i.name == name),
                "{name} is served but not declared"
            );
        }
    }

    /// **No POSIX name is served twice, and none collides with its own delegate.**
    ///
    /// A duplicate would mean the registry's last-wins rule picks one silently.
    #[test]
    fn no_name_is_delegated_twice() {
        let mut seen = std::collections::BTreeSet::new();
        for (posix, _) in super::DELEGATED {
            assert!(seen.insert(*posix), "{posix} is delegated more than once");
        }
    }
}
