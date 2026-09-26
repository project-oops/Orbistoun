//! The POSIX-named half of the platform, delegated to what already implements it.
//!
//! A title imports `pthread_create` from `libScePosix` and `scePthreadCreate` from
//! `libkernel`: two names for one behaviour, and a NID is the hash of a name, so the POSIX
//! spelling resolves only if it is declared. Each served name resolves to the same function
//! pointer as its vendor twin, looked up from the implementing crate at startup, so the two
//! cannot drift; arities come from the vendor declaration (D349). Success paths coincide
//! (both zero). A failure returns this project's placeholder rather than an invented errno,
//! so a guest testing `!= 0` takes its error path and one switching on errno values falls to
//! its default branch.

use orbistoun_core::GuestFn;
use orbistoun_hle::guest_module;

guest_module! {
    "libScePosix" {
        // Every name here is imported by a title from this library. Served names take the arity of
        // the vendor function they delegate to; the rest are provisional.
        "close" => 1,
        "open" => 3,
        "pthread_attr_destroy" => 1,
        "pthread_attr_getschedparam" => 2,
        "pthread_attr_init" => 1,
        "pthread_attr_setschedparam" => 2,
        "pthread_attr_setstacksize" => 2,
        "pthread_cond_broadcast" => 1,
        "pthread_cond_destroy" => 1,
        // Two arguments, not the three its vendor twin takes: `scePthreadCondInit` ends in a name
        // and the POSIX call does not.
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
        // `(thread, policy, const struct sched_param *)` in both spellings, so they delegate.
        "pthread_getschedparam" => 3,
        "pthread_setschedparam" => 3,
        "read" => 3,
        // Imported by most open-toolchain payloads; the POSIX spelling of a call already served.
        "write" => 3,
        // The time calls, implemented in `orbistoun-libc` and declared here, where a title imports
        // them (D367).
        "clock_gettime" => 2,
        "gettimeofday" => 2,
        // The sockets, implemented in `orbistoun-fs` beside the descriptor table they share with
        // files (D371). `accept`, `listen` and `getpeername` are declared by inference from their
        // imported siblings, recorded as an inference in the knowledge file.
        "socket" => 3,
        "bind" => 3,
        "listen" => 2,
        "accept" => 3,
        "connect" => 3,
        "setsockopt" => 5,
        "getsockopt" => 5,
        "getsockname" => 3,
        "getpeername" => 3,
        "send" => 4,
        "recv" => 4,
        "shutdown" => 2,
        // Waiting, and printing an address: imported from this library, implemented in
        // `orbistoun-fs` beside the descriptor table (D367).
        "select" => 5,
        "inet_ntop" => 4,
        // The timed acquisitions. The POSIX ones take an absolute deadline as their last argument
        // and the `_np` pair a relative span, so each has its own entry point.
        "pthread_mutex_timedlock" => 2,
        "posix_pthread_mutex_timedlock" => 2,
        "pthread_rwlock_timedrdlock" => 2,
        "posix_pthread_rwlock_timedrdlock" => 2,
        "pthread_rwlock_timedwrlock" => 2,
        "posix_pthread_rwlock_timedwrlock" => 2,
        "sem_timedwait" => 2,
        "posix_sem_timedwait" => 2,
        "sem_reltimedwait_np" => 2,
        "posix_sem_reltimedwait_np" => 2,
        "sem_getvalue" => 2,
        "posix_sem_getvalue" => 2,
        "pthread_cond_reltimedwait_np" => 3,
        "posix_pthread_cond_reltimedwait_np" => 3,
        // The POSIX timed condition wait and the once-only initialiser.
        // `pthread_cond_timedwait` takes an absolute deadline as its third argument.
        "pthread_cond_timedwait" => 3,
        "posix_pthread_cond_timedwait" => 3,
        "pthread_once" => 2,
        "posix_pthread_once" => 2,
        // The barrier and read-write lock attribute accessors, and the scheduling hints.
        // POSIX arities; the `_np` pair is FreeBSD's non-portable kind accessor.
        "pthread_barrierattr_init" => 1,
        "pthread_barrierattr_destroy" => 1,
        "pthread_barrierattr_getpshared" => 2,
        "pthread_barrierattr_setpshared" => 2,
        "pthread_rwlockattr_init" => 1,
        "posix_pthread_rwlockattr_init" => 1,
        "pthread_rwlockattr_destroy" => 1,
        "posix_pthread_rwlockattr_destroy" => 1,
        "pthread_rwlockattr_getpshared" => 2,
        "posix_pthread_rwlockattr_getpshared" => 2,
        "pthread_rwlockattr_setpshared" => 2,
        "posix_pthread_rwlockattr_setpshared" => 2,
        "pthread_rwlockattr_gettype_np" => 2,
        "posix_pthread_rwlockattr_gettype_np" => 2,
        "pthread_rwlockattr_settype_np" => 2,
        "posix_pthread_rwlockattr_settype_np" => 2,
        "pthread_yield" => 0,
        "posix_pthread_yield" => 0,
        "sched_yield" => 0,
        "pthread_getconcurrency" => 0,
        "pthread_setconcurrency" => 1,
        // The attribute accessors, implemented under their POSIX names in
        // `orbistoun-kernel` beside the attribute object they read and write.
        "pthread_attr_getguardsize" => 2,
        "posix_pthread_attr_getguardsize" => 2,
        "pthread_attr_getinheritsched" => 2,
        "posix_pthread_attr_getinheritsched" => 2,
        "pthread_attr_getschedpolicy" => 2,
        "posix_pthread_attr_getschedpolicy" => 2,
        "pthread_attr_getscope" => 2,
        "posix_pthread_attr_getscope" => 2,
        "pthread_attr_setscope" => 2,
        "posix_pthread_attr_setscope" => 2,
        "pthread_mutexattr_getpshared" => 2,
        "posix_pthread_mutexattr_getpshared" => 2,
        "pthread_mutexattr_setpshared" => 2,
        "posix_pthread_mutexattr_setpshared" => 2,
        "pthread_mutexattr_getprioceiling" => 2,
        "posix_pthread_mutexattr_getprioceiling" => 2,
        "pthread_mutexattr_setprioceiling" => 2,
        "posix_pthread_mutexattr_setprioceiling" => 2,
        "pthread_condattr_getclock" => 2,
        "posix_pthread_condattr_getclock" => 2,
        "pthread_condattr_setclock" => 2,
        "posix_pthread_condattr_setclock" => 2,
        "pthread_condattr_getpshared" => 2,
        "posix_pthread_condattr_getpshared" => 2,
        "pthread_condattr_setpshared" => 2,
        "posix_pthread_condattr_setpshared" => 2,
        "pthread_condattr_destroy" => 1,
        "posix_pthread_condattr_destroy" => 1,
        "posix_pthread_equal" => 2,
        // The `posix_`-prefixed family: the same POSIX functions under the names this library also
        // exports them by, each with its unprefixed twin's arity (D475).
        "posix_clock_gettime" => 2,
        "posix_close" => 1,
        "posix_fstat" => 2,
        "posix_fsync" => 1,
        "posix_ftruncate" => 2,
        "posix_getpid" => 0,
        "posix_gettimeofday" => 2,
        "posix_kevent" => 6,
        "posix_kqueue" => 0,
        "posix_lseek" => 3,
        "posix_mkdir" => 2,
        "posix_mmap" => 6,
        "posix_mprotect" => 3,
        "posix_munmap" => 2,
        "posix_nanosleep" => 2,
        "posix_open" => 3,
        "posix_pread" => 4,
        "posix_pthread_attr_destroy" => 1,
        "posix_pthread_attr_getdetachstate" => 2,
        "posix_pthread_attr_getschedparam" => 2,
        "posix_pthread_attr_getstacksize" => 2,
        "posix_pthread_attr_init" => 1,
        "posix_pthread_attr_setdetachstate" => 2,
        "posix_pthread_attr_setguardsize" => 2,
        "posix_pthread_attr_setinheritsched" => 2,
        "posix_pthread_attr_setschedparam" => 2,
        "posix_pthread_attr_setschedpolicy" => 2,
        "posix_pthread_attr_setstacksize" => 2,
        "posix_pthread_cond_broadcast" => 1,
        "posix_pthread_cond_destroy" => 1,
        "posix_pthread_cond_init" => 2,
        "posix_pthread_cond_signal" => 1,
        "posix_pthread_cond_wait" => 2,
        "posix_pthread_condattr_init" => 1,
        "posix_pthread_create" => 4,
        "posix_pthread_detach" => 1,
        "posix_pthread_exit" => 1,
        "posix_pthread_getspecific" => 1,
        "posix_pthread_join" => 2,
        "posix_pthread_key_create" => 2,
        "posix_pthread_key_delete" => 1,
        "posix_pthread_mutex_destroy" => 1,
        "posix_pthread_mutex_init" => 2,
        "posix_pthread_mutex_lock" => 1,
        "posix_pthread_mutex_trylock" => 1,
        "posix_pthread_mutex_unlock" => 1,
        "posix_pthread_mutexattr_destroy" => 1,
        "posix_pthread_mutexattr_getprotocol" => 2,
        "posix_pthread_mutexattr_gettype" => 2,
        "posix_pthread_mutexattr_init" => 1,
        "posix_pthread_mutexattr_setprotocol" => 2,
        "posix_pthread_mutexattr_settype" => 2,
        "posix_pthread_self" => 0,
        "posix_pthread_setspecific" => 2,
        "posix_pwrite" => 4,
        "posix_pwritev" => 4,
        "posix_read" => 3,
        "posix_rename" => 2,
        "posix_rmdir" => 1,
        "posix_select" => 5,
        "posix_sem_destroy" => 1,
        "posix_sem_init" => 3,
        "posix_sem_post" => 1,
        "posix_sem_trywait" => 1,
        "posix_sem_wait" => 1,
        "posix_signal" => 2,
        "posix_sleep" => 1,
        "posix_stat" => 2,
        "posix_write" => 3,
        // Descriptor and mapping calls, implemented under their POSIX names beside the file calls
        // in `orbistoun-fs`. Arities are the POSIX signatures. `flock`, `msync`, `getrlimit`,
        // `getsockopt` and the `getdents` family are refused rather than guessed.
        "creat" => 2,
        "readv" => 3,
        "writev" => 3,
        "preadv" => 4,
        "pwritev" => 4,
        "fsync" => 1,
        "fdatasync" => 1,
        "getpagesize" => 0,
        "madvise" => 3,
        // Byte order, one operation under four names. One argument each.
        "htonl" => 1, "htons" => 1, "ntohl" => 1, "ntohs" => 1,
        // Delegated to the named vendor twin, whose arity matches the POSIX one.
        "inet_pton" => 3,
        "mprotect" => 3,
        "pthread_attr_getdetachstate" => 2,
        "pthread_attr_getstacksize" => 2,
        "pthread_attr_setdetachstate" => 2,
        "pthread_attr_setguardsize" => 2,
        "pthread_attr_setinheritsched" => 2,
        "pthread_attr_setschedpolicy" => 2,
        // POSIX arities: the rwlock init takes a lock and attributes, the barrier init a barrier,
        // attributes and a count. Neither takes the name their vendor twins do.
        "pthread_rwlock_init" => 2,
        "pthread_barrier_init" => 3,
        "pthread_barrier_destroy" => 1,
        "pthread_barrier_wait" => 1,
        "pthread_condattr_init" => 1,
        "pthread_mutex_trylock" => 1,
        "pthread_mutexattr_getprotocol" => 2,
        "pthread_mutexattr_gettype" => 2,
        "pthread_rwlock_destroy" => 1,
        "pthread_rwlock_rdlock" => 1,
        "pthread_rwlock_tryrdlock" => 1,
        "pthread_rwlock_trywrlock" => 1,
        "pthread_rwlock_unlock" => 1,
        "pthread_rwlock_wrlock" => 1,
        // Imported here, and delegating exactly as `read` and `close` do.
        "lseek" => 3,
        "munmap" => 2,
        "mmap" => 6,
        // FreeBSD's own underscored spellings, which its C library uses internally so that a
        // program replacing `open` does not break `fopen`. The same function, so the same delegate.
        "_open" => 3,
        "_close" => 1,
        "_read" => 3,
        // Thread calls with no vendor-named twin, written under their POSIX names in
        // `orbistoun-kernel` beside the thread registry (D367).
        "pthread_detach" => 1,
        "pthread_exit" => 1,
        // `fstat` is imported here; `stat`, `lstat` and the directory calls are declared in `libc`,
        // where FreeBSD puts them (D367).
        "fstat" => 2,
        // Imported and not served: no vendor-named equivalent is implemented. Most are sockets.
        // The registry takes the last declaration of a name, so a row left here after its name is
        // served shadows the real arity; `no_name_is_declared_twice` catches it.
        "pthread_equal" => 2,
        // Thread-specific-data keys, written under their POSIX names in `orbistoun-kernel` beside
        // the thread registry (D453).
        "pthread_getspecific" => 1,
        "pthread_key_create" => 2,
        "pthread_key_delete" => 1,
        "pthread_setspecific" => 2,
        "recvfrom" => 0,
        // POSIX unnamed semaphores, written under their POSIX names in `orbistoun-kernel` beside
        // the vendor semaphore calls.
        "sem_init" => 3,
        "sem_wait" => 1,
        "sem_trywait" => 1,
        "sem_post" => 1,
        "sem_destroy" => 1,
        "sched_get_priority_max" => 0,
        "sched_get_priority_min" => 0,
        "sendto" => 0,
    }
}

/// Which POSIX name is served by which vendor-named function.
///
/// A table rather than a naming rule: the names mostly transform mechanically
/// (`pthread_mutex_lock` to `scePthreadMutexLock`), and the exceptions would be served the
/// wrong function silently.
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
    // Three that do not delegate to their vendor twin: each vendor call ends in a name the
    // POSIX one has no argument for, so delegating would read a register the caller never set.
    // They are written under their POSIX names in `orbistoun-kernel`.
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
    ("pthread_getschedparam", "scePthreadGetschedparam"),
    ("pthread_setschedparam", "scePthreadSetschedparam"),
    ("read", "sceKernelRead"),
    ("write", "sceKernelWrite"),
    // Entries where both names are the same: POSIX functions with no vendor-named twin,
    // implemented under their own name in `orbistoun-libc`. The entry binds the declaration
    // here to that code, and the test below refuses one that names nothing.
    ("clock_gettime", "clock_gettime"),
    ("gettimeofday", "gettimeofday"),
    // The sockets, same-named for the same reason: no vendor-named twin, and a POSIX
    // implementation beside the descriptor table.
    ("socket", "socket"),
    ("bind", "bind"),
    ("listen", "listen"),
    ("accept", "accept"),
    ("connect", "connect"),
    ("setsockopt", "setsockopt"),
    ("getsockopt", "getsockopt"),
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
    // The read-write locks, barriers and remaining attribute accessors, each delegating to the
    // vendor function that implements it, with arities checked against the POSIX signature.
    // `scePthreadBarrierInit` and `scePthreadRwlockInit` take a trailing name argument, so
    // their POSIX spellings have their own entry points below.
    ("inet_pton", "__inet_pton"),
    ("mprotect", "sceKernelMprotect"),
    (
        "pthread_attr_getdetachstate",
        "scePthreadAttrGetdetachstate",
    ),
    ("pthread_attr_getstacksize", "scePthreadAttrGetstacksize"),
    (
        "pthread_attr_setdetachstate",
        "scePthreadAttrSetdetachstate",
    ),
    ("pthread_attr_setguardsize", "scePthreadAttrSetguardsize"),
    (
        "pthread_attr_setinheritsched",
        "scePthreadAttrSetinheritsched",
    ),
    (
        "pthread_attr_setschedpolicy",
        "scePthreadAttrSetschedpolicy",
    ),
    // The two inits with a POSIX-signature entry point of their own, so neither reads the
    // trailing name argument its vendor twin takes.
    ("pthread_rwlock_init", "posix_pthread_rwlock_init"),
    ("pthread_barrier_init", "posix_pthread_barrier_init"),
    ("pthread_barrier_destroy", "scePthreadBarrierDestroy"),
    ("pthread_barrier_wait", "scePthreadBarrierWait"),
    ("pthread_condattr_init", "scePthreadCondattrInit"),
    ("pthread_mutex_trylock", "scePthreadMutexTrylock"),
    (
        "pthread_mutexattr_getprotocol",
        "scePthreadMutexattrGetprotocol",
    ),
    ("pthread_mutexattr_gettype", "scePthreadMutexattrGettype"),
    ("pthread_rwlock_destroy", "scePthreadRwlockDestroy"),
    ("pthread_rwlock_rdlock", "scePthreadRwlockRdlock"),
    ("pthread_rwlock_tryrdlock", "scePthreadRwlockTryrdlock"),
    ("pthread_rwlock_trywrlock", "scePthreadRwlockTrywrlock"),
    ("pthread_rwlock_unlock", "scePthreadRwlockUnlock"),
    ("pthread_rwlock_wrlock", "scePthreadRwlockWrlock"),
    // The `posix_`-prefixed family, each delegating to the unprefixed function that implements
    // it. The semantics follow the POSIX analogue; the failure convention is assumed (D475).
    ("posix_clock_gettime", "clock_gettime"),
    ("posix_close", "sceKernelClose"),
    ("posix_fstat", "fstat"),
    ("posix_fsync", "fsync"),
    ("posix_ftruncate", "ftruncate"),
    ("posix_getpid", "getpid"),
    ("posix_gettimeofday", "gettimeofday"),
    ("posix_kevent", "kevent"),
    ("posix_kqueue", "kqueue"),
    ("posix_lseek", "sceKernelLseek"),
    ("posix_mkdir", "mkdir"),
    ("posix_mmap", "mmap"),
    ("posix_mprotect", "sceKernelMprotect"),
    ("posix_munmap", "sceKernelMunmap"),
    ("posix_nanosleep", "nanosleep"),
    ("posix_open", "sceKernelOpen"),
    ("posix_pread", "pread"),
    ("posix_pthread_attr_destroy", "scePthreadAttrDestroy"),
    (
        "posix_pthread_attr_getdetachstate",
        "scePthreadAttrGetdetachstate",
    ),
    (
        "posix_pthread_attr_getschedparam",
        "scePthreadAttrGetschedparam",
    ),
    (
        "posix_pthread_attr_getstacksize",
        "scePthreadAttrGetstacksize",
    ),
    ("posix_pthread_attr_init", "scePthreadAttrInit"),
    (
        "posix_pthread_attr_setdetachstate",
        "scePthreadAttrSetdetachstate",
    ),
    (
        "posix_pthread_attr_setguardsize",
        "scePthreadAttrSetguardsize",
    ),
    (
        "posix_pthread_attr_setinheritsched",
        "scePthreadAttrSetinheritsched",
    ),
    (
        "posix_pthread_attr_setschedparam",
        "scePthreadAttrSetschedparam",
    ),
    (
        "posix_pthread_attr_setschedpolicy",
        "scePthreadAttrSetschedpolicy",
    ),
    (
        "posix_pthread_attr_setstacksize",
        "scePthreadAttrSetstacksize",
    ),
    ("posix_pthread_cond_broadcast", "scePthreadCondBroadcast"),
    ("posix_pthread_cond_destroy", "scePthreadCondDestroy"),
    ("posix_pthread_cond_init", "pthread_cond_init"),
    ("posix_pthread_cond_signal", "scePthreadCondSignal"),
    ("posix_pthread_cond_wait", "scePthreadCondWait"),
    ("posix_pthread_condattr_init", "scePthreadCondattrInit"),
    ("posix_pthread_create", "pthread_create"),
    ("posix_pthread_detach", "pthread_detach"),
    ("posix_pthread_exit", "pthread_exit"),
    ("posix_pthread_getspecific", "pthread_getspecific"),
    ("posix_pthread_join", "scePthreadJoin"),
    ("posix_pthread_key_create", "pthread_key_create"),
    ("posix_pthread_key_delete", "pthread_key_delete"),
    ("posix_pthread_mutex_destroy", "scePthreadMutexDestroy"),
    ("posix_pthread_mutex_init", "pthread_mutex_init"),
    ("posix_pthread_mutex_lock", "scePthreadMutexLock"),
    ("posix_pthread_mutex_trylock", "scePthreadMutexTrylock"),
    ("posix_pthread_mutex_unlock", "scePthreadMutexUnlock"),
    (
        "posix_pthread_mutexattr_destroy",
        "scePthreadMutexattrDestroy",
    ),
    (
        "posix_pthread_mutexattr_getprotocol",
        "scePthreadMutexattrGetprotocol",
    ),
    (
        "posix_pthread_mutexattr_gettype",
        "scePthreadMutexattrGettype",
    ),
    ("posix_pthread_mutexattr_init", "scePthreadMutexattrInit"),
    (
        "posix_pthread_mutexattr_setprotocol",
        "scePthreadMutexattrSetprotocol",
    ),
    (
        "posix_pthread_mutexattr_settype",
        "scePthreadMutexattrSettype",
    ),
    ("posix_pthread_self", "scePthreadSelf"),
    ("posix_pthread_setspecific", "pthread_setspecific"),
    ("posix_pwrite", "pwrite"),
    ("posix_pwritev", "pwritev"),
    ("posix_read", "sceKernelRead"),
    ("posix_rename", "rename"),
    ("posix_rmdir", "rmdir"),
    ("posix_select", "select"),
    ("posix_sem_destroy", "sem_destroy"),
    ("posix_sem_init", "sem_init"),
    ("posix_sem_post", "sem_post"),
    ("posix_sem_trywait", "sem_trywait"),
    ("posix_sem_wait", "sem_wait"),
    ("posix_signal", "signal"),
    ("posix_sleep", "sleep"),
    ("posix_stat", "stat"),
    ("posix_write", "sceKernelWrite"),
    // Descriptor and mapping calls: no vendor twin, POSIX-named implementations in the
    // filesystem layer.
    ("creat", "creat"),
    ("readv", "readv"),
    ("writev", "writev"),
    ("preadv", "preadv"),
    ("pwritev", "pwritev"),
    ("fsync", "fsync"),
    ("fdatasync", "fdatasync"),
    ("getpagesize", "getpagesize"),
    ("madvise", "madvise"),
    // The attribute accessors: POSIX-named implementations in the kernel, and the
    // `posix_`-prefixed spelling of each pointing at the same one.
    ("pthread_attr_getguardsize", "pthread_attr_getguardsize"),
    (
        "posix_pthread_attr_getguardsize",
        "pthread_attr_getguardsize",
    ),
    (
        "pthread_attr_getinheritsched",
        "pthread_attr_getinheritsched",
    ),
    (
        "posix_pthread_attr_getinheritsched",
        "pthread_attr_getinheritsched",
    ),
    ("pthread_attr_getschedpolicy", "pthread_attr_getschedpolicy"),
    (
        "posix_pthread_attr_getschedpolicy",
        "pthread_attr_getschedpolicy",
    ),
    ("pthread_attr_getscope", "pthread_attr_getscope"),
    ("posix_pthread_attr_getscope", "pthread_attr_getscope"),
    ("pthread_attr_setscope", "pthread_attr_setscope"),
    ("posix_pthread_attr_setscope", "pthread_attr_setscope"),
    (
        "pthread_mutexattr_getpshared",
        "pthread_mutexattr_getpshared",
    ),
    (
        "posix_pthread_mutexattr_getpshared",
        "pthread_mutexattr_getpshared",
    ),
    (
        "pthread_mutexattr_setpshared",
        "pthread_mutexattr_setpshared",
    ),
    (
        "posix_pthread_mutexattr_setpshared",
        "pthread_mutexattr_setpshared",
    ),
    (
        "pthread_mutexattr_getprioceiling",
        "pthread_mutexattr_getprioceiling",
    ),
    (
        "posix_pthread_mutexattr_getprioceiling",
        "pthread_mutexattr_getprioceiling",
    ),
    (
        "pthread_mutexattr_setprioceiling",
        "pthread_mutexattr_setprioceiling",
    ),
    (
        "posix_pthread_mutexattr_setprioceiling",
        "pthread_mutexattr_setprioceiling",
    ),
    ("pthread_condattr_getclock", "pthread_condattr_getclock"),
    (
        "posix_pthread_condattr_getclock",
        "pthread_condattr_getclock",
    ),
    ("pthread_condattr_setclock", "pthread_condattr_setclock"),
    (
        "posix_pthread_condattr_setclock",
        "pthread_condattr_setclock",
    ),
    ("pthread_condattr_getpshared", "pthread_condattr_getpshared"),
    (
        "posix_pthread_condattr_getpshared",
        "pthread_condattr_getpshared",
    ),
    ("pthread_condattr_setpshared", "pthread_condattr_setpshared"),
    (
        "posix_pthread_condattr_setpshared",
        "pthread_condattr_setpshared",
    ),
    ("pthread_condattr_destroy", "pthread_condattr_destroy"),
    ("posix_pthread_condattr_destroy", "pthread_condattr_destroy"),
    ("pthread_equal", "pthread_equal"),
    ("posix_pthread_equal", "pthread_equal"),
    // The barrier and read-write lock attribute accessors, and the scheduling hints: POSIX-named
    // implementations in the kernel, beside the locks they configure.
    ("pthread_barrierattr_init", "pthread_barrierattr_init"),
    ("pthread_barrierattr_destroy", "pthread_barrierattr_destroy"),
    (
        "pthread_barrierattr_getpshared",
        "pthread_barrierattr_getpshared",
    ),
    (
        "pthread_barrierattr_setpshared",
        "pthread_barrierattr_setpshared",
    ),
    ("pthread_rwlockattr_init", "pthread_rwlockattr_init"),
    ("posix_pthread_rwlockattr_init", "pthread_rwlockattr_init"),
    ("pthread_rwlockattr_destroy", "pthread_rwlockattr_destroy"),
    (
        "posix_pthread_rwlockattr_destroy",
        "pthread_rwlockattr_destroy",
    ),
    (
        "pthread_rwlockattr_getpshared",
        "pthread_rwlockattr_getpshared",
    ),
    (
        "posix_pthread_rwlockattr_getpshared",
        "pthread_rwlockattr_getpshared",
    ),
    (
        "pthread_rwlockattr_setpshared",
        "pthread_rwlockattr_setpshared",
    ),
    (
        "posix_pthread_rwlockattr_setpshared",
        "pthread_rwlockattr_setpshared",
    ),
    (
        "pthread_rwlockattr_gettype_np",
        "pthread_rwlockattr_gettype_np",
    ),
    (
        "posix_pthread_rwlockattr_gettype_np",
        "pthread_rwlockattr_gettype_np",
    ),
    (
        "pthread_rwlockattr_settype_np",
        "pthread_rwlockattr_settype_np",
    ),
    (
        "posix_pthread_rwlockattr_settype_np",
        "pthread_rwlockattr_settype_np",
    ),
    ("pthread_yield", "pthread_yield"),
    ("posix_pthread_yield", "pthread_yield"),
    ("sched_yield", "sched_yield"),
    ("pthread_getconcurrency", "pthread_getconcurrency"),
    ("pthread_setconcurrency", "pthread_setconcurrency"),
    // The POSIX timed condition wait and the once-only initialiser: POSIX-named implementations
    // in the kernel, beside the condition variables they use.
    // The timed acquisitions, each to the entry point of its own spelling.
    ("pthread_mutex_timedlock", "pthread_mutex_timedlock"),
    ("posix_pthread_mutex_timedlock", "pthread_mutex_timedlock"),
    ("pthread_rwlock_timedrdlock", "pthread_rwlock_timedrdlock"),
    (
        "posix_pthread_rwlock_timedrdlock",
        "pthread_rwlock_timedrdlock",
    ),
    ("pthread_rwlock_timedwrlock", "pthread_rwlock_timedwrlock"),
    (
        "posix_pthread_rwlock_timedwrlock",
        "pthread_rwlock_timedwrlock",
    ),
    ("sem_timedwait", "sem_timedwait"),
    ("posix_sem_timedwait", "sem_timedwait"),
    ("sem_reltimedwait_np", "sem_reltimedwait_np"),
    ("posix_sem_reltimedwait_np", "sem_reltimedwait_np"),
    ("sem_getvalue", "sem_getvalue"),
    ("posix_sem_getvalue", "sem_getvalue"),
    (
        "pthread_cond_reltimedwait_np",
        "pthread_cond_reltimedwait_np",
    ),
    (
        "posix_pthread_cond_reltimedwait_np",
        "pthread_cond_reltimedwait_np",
    ),
    ("pthread_cond_timedwait", "pthread_cond_timedwait"),
    ("posix_pthread_cond_timedwait", "pthread_cond_timedwait"),
    ("pthread_once", "pthread_once"),
    ("posix_pthread_once", "pthread_once"),
    // Byte order: no vendor twin, POSIX-named implementations beside the sockets that use
    // them. Delegating to themselves is how a POSIX-named implementation is reached.
    ("htonl", "htonl"),
    ("htons", "htons"),
    ("ntohl", "ntohl"),
    ("ntohs", "ntohs"),
    // POSIX unnamed semaphores: no vendor twin, POSIX-named implementations in the kernel.
    ("sem_init", "sem_init"),
    ("sem_wait", "sem_wait"),
    ("sem_trywait", "sem_trywait"),
    ("sem_post", "sem_post"),
    ("sem_destroy", "sem_destroy"),
];

/// Implementations this crate provides, by symbol name.
///
/// Built by looking each vendor name up in the crate that implements it, so a moved or
/// renamed function produces an empty entry rather than a wrong one, which the test below
/// refuses.
#[must_use]
pub fn implementations() -> Vec<(&'static str, GuestFn)> {
    // Both crates: the delegates are split across the kernel (threads, time) and the
    // filesystem shim (files).
    let mut serving: Vec<(&'static str, GuestFn)> = orbistoun_kernel::implementations().to_vec();
    serving.extend_from_slice(orbistoun_fs::implementations());
    serving.extend(orbistoun_libc::implementations());
    serving.extend_from_slice(orbistoun_fs::socket::implementations());
    serving.extend_from_slice(orbistoun_fs::select::implementations());
    serving.extend_from_slice(orbistoun_fs::ifaddrs::implementations());
    serving.extend_from_slice(orbistoun_fs::metadata::implementations());
    let found = |vendor: &str| {
        serving
            .iter()
            .find(|(name, _)| *name == vendor)
            .map(|(_, function)| *function)
    };
    let _ = FILE_TWINS.set(FILE_CALLS.map(|(_, vendor, _)| found(vendor)));
    DELEGATED
        .iter()
        .filter_map(|(posix, vendor)| {
            // The file calls fail the POSIX way, so they are served by a wrapper that translates
            // their twin's vendor code; everything else is the twin's own pointer.
            let translating = FILE_CALLS
                .iter()
                .find(|(name, _, _)| name == posix)
                .map(|(_, _, wrapper)| *wrapper);
            match translating {
                Some(wrapper) => found(vendor).map(|_| (*posix, wrapper)),
                None => found(vendor).map(|function| (*posix, function)),
            }
        })
        .collect()
}

/// The POSIX file calls whose failures are reported the POSIX way: `-1` with `errno` set.
///
/// Measured for `open` (a missing path gives `ENOENT`) and `close` (`close(-1)` gives `EBADF`):
/// the `open` and `close` records in `libScePosix.toml`. `read` and `write` are assumed to
/// follow the same convention.
const FILE_CALLS: [(&str, &str, GuestFn); 4] = [
    ("open", "sceKernelOpen", posix_open),
    ("close", "sceKernelClose", posix_close),
    ("read", "sceKernelRead", posix_read),
    ("write", "sceKernelWrite", posix_write),
];

/// The vendor twin each of [`FILE_CALLS`] delegates to, found once at registration.
static FILE_TWINS: std::sync::OnceLock<[Option<GuestFn>; 4]> = std::sync::OnceLock::new();

/// Calls file twin `index` and reports its failure the POSIX way.
fn file_call(index: usize, args: &[u64; orbistoun_core::GUEST_ARG_REGISTERS]) -> u64 {
    let twin = FILE_TWINS.get().and_then(|twins| twins[index]);
    match twin {
        Some(twin) => orbistoun_libc::posix_failure(twin(args)),
        // Unreachable, since a wrapper is registered only when its twin was found; answered with
        // the project's placeholder rather than a chosen errno.
        None => u64::from(orbistoun_core::GuestError::Unimplemented.as_raw()),
    }
}

/// `open(path, flags, mode)`, failing the POSIX way.
fn posix_open(args: &[u64; orbistoun_core::GUEST_ARG_REGISTERS]) -> u64 {
    file_call(0, args)
}

/// `close(fd)`, failing the POSIX way.
fn posix_close(args: &[u64; orbistoun_core::GUEST_ARG_REGISTERS]) -> u64 {
    file_call(1, args)
}

/// `read(fd, buf, n)`, failing the POSIX way.
fn posix_read(args: &[u64; orbistoun_core::GUEST_ARG_REGISTERS]) -> u64 {
    file_call(2, args)
}

/// `write(fd, buf, n)`, failing the POSIX way.
fn posix_write(args: &[u64; orbistoun_core::GUEST_ARG_REGISTERS]) -> u64 {
    file_call(3, args)
}

#[cfg(test)]
mod tests {
    /// Every delegation finds the function it names, asserted exactly, so a renamed vendor
    /// function cannot leave a POSIX name answered by nobody.
    #[test]
    fn every_delegation_resolves_to_a_real_implementation() {
        let served = super::implementations();
        // Names the offenders rather than only counting them.
        let missing: Vec<String> = super::DELEGATED
            .iter()
            .filter(|(posix, _)| !served.iter().any(|(name, _)| name == posix))
            .map(|(posix, vendor)| format!("{posix} -> {vendor}"))
            .collect();
        assert!(
            missing.is_empty(),
            "these delegations name functions nothing implements: {missing:?}"
        );
        assert_eq!(served.len(), super::DELEGATED.len());
    }

    /// `close(-1)` answers `-1` with `errno = EBADF`, not the vendor twin's `0x8002_0009`.
    #[test]
    fn a_posix_file_call_fails_with_errno_not_a_vendor_code() {
        /// `EBADF`, from FreeBSD `sys/sys/errno.h`, the value the hardware reports.
        const EBADF: i32 = 9;
        let served = super::implementations();
        let close = served
            .iter()
            .find(|(name, _)| *name == "close")
            .map(|(_, f)| *f)
            .expect("close is served");
        let mut args = [0u64; orbistoun_core::GUEST_ARG_REGISTERS];
        args[0] = u64::from(u32::MAX);
        assert_eq!(close(&args), 0xFFFF_FFFF, "-1 in a 32-bit register");
        let errno_at = orbistoun_libc::implementations()
            .into_iter()
            .find(|(name, _)| *name == "__error")
            .map(|(_, f)| f(&[0; orbistoun_core::GUEST_ARG_REGISTERS]))
            .expect("__error is served");
        // SAFETY: `__error` answers the address of this thread's `errno`, a live `i32`.
        let errno =
            unsafe { std::ptr::read(std::ptr::with_exposed_provenance::<i32>(errno_at as usize)) };
        assert_eq!(errno, EBADF);
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

    /// No POSIX name is served twice, and none collides with its own delegate.
    #[test]
    fn no_name_is_delegated_twice() {
        let mut seen = std::collections::BTreeSet::new();
        for (posix, _) in super::DELEGATED {
            assert!(seen.insert(*posix), "{posix} is delegated more than once");
        }
    }

    /// No name is declared twice: the registry keeps the last declaration, so a duplicate would
    /// silently shadow a live arity with a stale one. Arity feeds the trace and gap report, not
    /// dispatch, so the shadowing is otherwise invisible.
    #[test]
    fn no_name_is_declared_twice() {
        let mut seen = std::collections::BTreeSet::new();
        let repeated: Vec<&str> = super::MODULE
            .imports
            .iter()
            .filter(|i| !seen.insert(i.name))
            .map(|i| i.name)
            .collect();
        assert!(
            repeated.is_empty(),
            "declared more than once, so the last row silently wins: {repeated:?}"
        );
    }
}
