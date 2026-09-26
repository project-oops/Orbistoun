//! The C library, as the guest calls it.
//!
//! A title told "not implemented" by `strlen` or `memset` carries on with the error code as
//! its answer, and the damage surfaces somewhere unrelated. ISO C and POSIX say precisely what
//! these functions do, and the target library is FreeBSD-derived, so both are citable. The
//! address space is identity-mapped, so a guest pointer is dereferenced directly and the guest
//! is trusted about its own arguments, as the real library trusts them: a bad pointer faults
//! here as it would there, and the fault reporter names the address.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};
mod arena;
mod atomic;
mod clock;
pub mod cstring;
mod ctype;
mod cxx;
mod locks;
pub mod math;
pub mod said;
mod scan;
pub mod streams;
mod varargs;

use orbistoun_hle::guest_module;

guest_module! {
    "libc" {
        // `std::call_once`'s engine. Declared here because a title imports it from libc, and
        // implemented in `orbistoun-kernel`, which owns running a guest callback on a fresh stack
        // (D367). Three integer arguments: the flag, the callback, and its context.
        "_ZSt13_Execute_onceRSt9once_flagPFiPvS1_PS1_ES1_" => 3,
        // The C-runtime threading primitives `std::mutex`, `std::condition_variable` and
        // `std::this_thread` lower onto. Declared here because a title imports them from libc, and
        // implemented in `orbistoun-kernel` beside the thread registry and `sync` primitives.
        // Arity: the object acted on, plus a type for `_Mtx_init`, a mutex for the condition waits,
        // and a deadline for the timed one. `_Xtime_get_ticks` takes none and answers the clock.
        "_Mtx_init" => 2,
        "_Mtx_destroy" => 1,
        "_Mtx_lock" => 1,
        "_Mtx_unlock" => 1,
        "_Mtx_trylock" => 1,
        "_Cnd_init" => 1,
        "_Cnd_destroy" => 1,
        "_Cnd_wait" => 2,
        "_Cnd_timedwait" => 3,
        "_Cnd_signal" => 1,
        "_Cnd_broadcast" => 1,
        "_Xtime_get_ticks" => 0,
        // The character-classification tables. Each answers a pointer the caller indexes, served in
        // `ctype` from a measured capture. No arguments.
        "_Getpctype" => 0,
        "_Getptolower" => 0,
        "_Getptoupper" => 0,
        "_Thrd_sleep" => 2,
        // The maths library. Arity is the count of floating-point arguments; none of these touches
        // an integer register (D268).
        "sqrt" => 1,
        "sqrtf" => 1,
        "fabs" => 1,
        "fabsf" => 1,
        "floor" => 1,
        "ceil" => 1,
        "trunc" => 1,
        "round" => 1,
        "fmod" => 2,
        "pow" => 2,
        "sin" => 1,
        "cos" => 1,
        "tan" => 1,
        "exp" => 1,
        "log" => 1,
        "log10" => 1,
        "log2" => 1,
        "asin" => 1,
        "acos" => 1,
        "atan" => 1,
        "atan2" => 2,
        "atan2f" => 2,
        "floorf" => 1,
        "ceilf" => 1,
        "truncf" => 1,
        "roundf" => 1,
        "fmodf" => 2,
        "powf" => 2,
        "sinf" => 1,
        "cosf" => 1,
        "tanf" => 1,
        "expf" => 1,
        "logf" => 1,
        "strtod" => 2,
        "strtof" => 2,
        // One float argument and two `float *` out-parameters, which arrive in integer registers.
        "sincosf" => 3,
        // Character classes and case folding. One `int` in, one `int` out.
        "isalpha" => 1, "isdigit" => 1, "isalnum" => 1, "isspace" => 1,
        "isupper" => 1, "islower" => 1, "ispunct" => 1, "isxdigit" => 1,
        "iscntrl" => 1, "isprint" => 1, "isgraph" => 1,
        "toupper" => 1, "tolower" => 1,
        // Searching within strings.
        "strstr" => 2, "strpbrk" => 2, "strspn" => 2, "strcspn" => 2,
        "strcasecmp" => 2, "strncasecmp" => 3,
        // Text to integer. The `strto*` family takes an end pointer and a base.
        "atoi" => 1, "atol" => 1, "atoll" => 1,
        "strtol" => 3, "strtoll" => 3, "strtoul" => 3, "strtoull" => 3,
        "abs" => 1, "labs" => 1, "llabs" => 1,
        "rand" => 0, "srand" => 1,
        "wcslen" => 1,
        // Single-precision and the remaining double entries, from ISO/IEC 9899 7.12.
        // `ldexp`/`frexp`/`modf`/`sincos` count their integer arguments: an exponent or an
        // out-parameter arrives in an integer register, not a floating-point one.
        "acosf" => 1, "asinf" => 1, "atanf" => 1, "cbrtf" => 1,
        "exp2" => 1, "exp2f" => 1, "log10f" => 1, "log2f" => 1, "tanhf" => 1,
        "nearbyintf" => 1, "hypotf" => 2,
        "frexp" => 2, "ldexp" => 2, "ldexpf" => 2, "modf" => 2, "modff" => 2,
        "sincos" => 3,
        // The bounded string functions: BSD's `strlcpy`/`strnstr`, and the C11 Annex K `_s` family
        // whose extra argument is the destination's size.
        "strlcpy" => 3, "strnstr" => 3,
        "wcscmp" => 2, "wcsncpy" => 3,
        "memcpy_s" => 4, "memmove_s" => 4, "memset_s" => 4,
        "strcat_s" => 3, "strncat_s" => 4, "strncpy_s" => 4, "wcsncpy_s" => 4,
        "wcsrchr" => 2,
        "snprintf" => 3, "sprintf" => 2,
        // The `va_list` forms. Fixed parameters only; the variadic half arrives through the list
        // (D364).
        "vsnprintf" => 4, "vprintf" => 2, "vfprintf" => 3,
        "vsprintf_s" => 4,
        // Breaking a `time_t` down and rendering it, as `asctime(localtime(&t))` does (D454).
        "localtime" => 1, "gmtime" => 1, "asctime" => 1,
        // Time, and waiting, all POSIX-documented. `gettimeofday` and `clock_gettime` are
        // implemented here and declared in `libScePosix`, where a title imports them (D367).
        "time" => 1, "sleep" => 1, "usleep" => 1, "nanosleep" => 2,
        "kill" => 2,
        // Parsing a formatted string, and rendering a time.
        "sscanf" => 6, "strftime" => 4,
        "getenv" => 1, "setenv" => 3, "unsetenv" => 1, "getcwd" => 2, "perror" => 1, "strerror_r" => 3,
        // The file calls that change a directory. Declared here, where FreeBSD puts them, and
        // implemented in `orbistoun-fs`, where the mount model lives (D367).
        "mkdir" => 2, "rmdir" => 1, "unlink" => 1, "remove" => 1,
        "rename" => 2, "access" => 2, "truncate" => 2, "ftruncate" => 2,
        "pread" => 4, "pwrite" => 4, "dup2" => 2,
        "chmod" => 2, "fchmod" => 2, "mlock" => 2, "munlock" => 2,
        "fdopen" => 2, "fileno" => 1, "sendfile" => 6,
        // Which addresses a guest can be reached on, and how it prints one. `inet_ntop` is declared
        // in `libScePosix`; FreeBSD's underscored spelling is declared here (D367).
        "getifaddrs" => 1, "freeifaddrs" => 1, "__inet_ntop" => 4, "__inet_pton" => 3,
        // How a file server decides whether a path a client names is real before acting on it.
        "realpath" => 2,
        // Waiting on many descriptors at once. Implemented in `orbistoun-fs` beside the descriptor
        // table, and declared here, where a payload imports them (D367).
        "kqueue" => 0, "kevent" => 6,
        // The question `sysctl` answers, asked by name; a guest branches on the kernel it is told
        // it is on (D397).
        "sysctlbyname" => 5,
        // What a descriptor is set to. Callers read the flags, change one bit and write them back.
        "fcntl" => 3,
        // What a guest is told about a file, and how it lists a directory. `fstat` is declared in
        // `libScePosix`, where a title imports it.
        "stat" => 2, "lstat" => 2,
        "opendir" => 1, "readdir" => 1, "closedir" => 1,
        "strdup" => 1, "strndup" => 2, "strncat" => 3,
        "strtok" => 2, "strtok_r" => 3,
        "qsort" => 4, "bsearch" => 5,
        // `setlocale` takes a category and a locale name; `clock` takes nothing; `setjmp` takes the
        // buffer it would save into.
        "setlocale" => 2, "clock" => 0, "setjmp" => 1,
        "memset" => 3,
        "memcpy" => 3,
        "memmove" => 3,
        "memcmp" => 3,
        // BSD `bcmp`: the same byte comparison as `memcmp` with only an equal/not-equal contract,
        // so `memcmp` answers it exactly.
        "bcmp" => 3,
        "memchr" => 3,
        "strlen" => 1,
        "strnlen" => 2,
        "strcmp" => 2,
        "strncmp" => 3,
        "strcpy" => 2,
        "strncpy" => 3,
        // `strcpy_s(dest, destsz, src)` - Annex K's bounds-checked copy, answering errno_t.
        "strcpy_s" => 3,
        "strcat" => 2,
        "strchr" => 2,
        "strrchr" => 2,
        "atexit" => 1,
        "malloc" => 1,
        "calloc" => 2,
        "realloc" => 2,
        "free" => 1,
        "__cxa_atexit" => 3,
        "__cxa_guard_acquire" => 1,
        "__cxa_guard_release" => 1,
        "__cxa_guard_abort" => 1,
        // The Itanium C++ ABI's allocation operators, which are the heap under another spelling.
        "_Znwm" => 1,
        "_Znam" => 1,
        "_ZdlPv" => 1,
        "_ZdaPv" => 1,
        "_ZdlPvm" => 2,
        "_ZdaPvm" => 2,
        // `std::_Random_device()` - the entropy primitive `std::random_device` reads through.
        // No arguments (the `v` suffix); answers an `unsigned int`.
        "_ZSt14_Random_devicev" => 0,
        // Stdio. Declaring a pointer-returning function lets the knowledge file say so, so an
        // unimplemented one answers null rather than an error code carried as a `FILE*` (D125).
        "fopen" => 2,
        "fclose" => 1,
        "fread" => 4,
        "fwrite" => 4,
        "fseek" => 3,
        "ftell" => 1,
        "rewind" => 1,
        // Reads a line; a read loop ends on its NULL.
        "fgets" => 3,
        "feof" => 1,
        "ferror" => 1,
        "fflush" => 1,
        // The bounds-checked spelling of `snprintf`.
        "snprintf_s" => 6,
        // Declared as the full register set: the arity of a variadic function is a property of each
        // call, and under-declaring would truncate the arguments before the renderer saw them.
        "memalign" => 2,
        "printf" => 6,
        // Declared because it must not return: the default stub returns into the trap a compiler
        // places after a `noreturn` call (D177).
        "abort" => 0,
        // The runtime's own assertion handler, which a failed `assert` reaches instead of calling
        // `abort`. One argument: the message.
        "_Assert" => 1,
        // The out-of-line C11 atomics. The trailing memory-order argument is counted: it is passed,
        // though the strongest order is used regardless.
        "_Atomic_load_4" => 2,
        "_Atomic_fetch_add_4" => 3,
        "_Atomic_fetch_sub_4" => 3,
        "_Atomic_compare_exchange_weak_4" => 5,
        // The runtime's own names for the conversions `strtoul`/`strtoull` wrap.
        "_Stoul" => 3, "_Stoull" => 3,
        "strtoumax" => 3, "strtoimax" => 3,
        // The runtime's internal recursive locks: a stream's, and its numbered system ones. One
        // argument each, the thing being locked.
        "_Lockfilelock" => 1, "_Unlockfilelock" => 1,
        "_Locksyslock" => 1, "_Unlocksyslock" => 1,
        // The C++ runtime this layer can answer without an unwinder: allocation, and the family
        // that never returns. Arities are the ABI's own; the `nothrow_t` and `align_val_t`
        // arguments are passed even where one is an empty tag type.
        "_ZnwmRKSt9nothrow_t" => 2, "_ZnwmSt11align_val_t" => 2,
        "_ZdlPvSt11align_val_t" => 2,
        "_ZSt15get_new_handlerv" => 0,
        "_ZSt9terminatev" => 0, "__cxa_pure_virtual" => 0,
        "_Unwind_Resume" => 1,
        "_ZSt14_Xlength_errorPKc" => 1, "_ZSt14_Xout_of_rangePKc" => 1,
        "_ZSt18_Xinvalid_argumentPKc" => 1,
        "_ZSt11_Xbad_allocv" => 0, "_ZSt19_Xbad_function_callv" => 0,
        "_ZSt14_Throw_C_errori" => 1, "_ZSt16_Throw_Cpp_errori" => 1,
        // Two arguments: the signal number and the handler. FreeBSD's `signal(3)`.
        "signal" => 2,
        // `getopt(argc, argv, optstring)`. POSIX.1-2008.
        "getopt" => 3,
        // FreeBSD's `__error()`: no arguments, answers a pointer to this thread's `errno`.
        "__error" => 0,
        // `strerror(errnum)`, answering a pointer to a message.
        "strerror" => 1,
        // `puts(s)`, which appends a newline where `printf` does not.
        "puts" => 1,
        // `putchar(c)`, one byte to the output stream.
        "putchar" => 1,
        // `getpid()`.
        "getpid" => 0,
        // `sysctl(name, namelen, oldp, oldlenp, newp, newlen)`. FreeBSD `sysctl(3)`.
        "sysctl" => 6,
        // `fprintf(stream, format, ...)`. One more argument than `printf`.
        "fprintf" => 6,
        // The platform's exposed Doug Lea `mspace` allocator: the arena is the leading argument,
        // the rest is the ordinary allocator (D451).
        "sceLibcMspaceMalloc" => 2,
        "sceLibcMspaceCalloc" => 3,
        "sceLibcMspaceRealloc" => 3,
        "sceLibcMspaceFree" => 2,
        "exit" => 1,
        "_Exit" => 1,
        // The raw syscall's spelling: FreeBSD entry 1 is `_exit`, and the name derived from
        // `SYS__exit` binds number 1 to this implementation.
        "_exit" => 1,
    }
}

/// Success, as a C caller reads it.
const OK: u64 = 0;

/// What C's stdio returns on failure: `EOF`, negative one widened to the register the guest
/// reads.
const EOF: u64 = u64::MAX;

/// Longest string these will walk before giving up.
///
/// Bounds a walk over an unterminated buffer, so the fault is not reported in string
/// handling. No real string reaches it.
const MAX_STRING: usize = 64 * 1024 * 1024;

/// Reinterprets a guest address as a pointer.
///
/// The mapping is identity, so this is a change of type rather than of value. Not `const`,
/// because exposing provenance in a const context needs a newer minimum supported version.
pub(crate) fn ptr(address: u64) -> *mut u8 {
    std::ptr::with_exposed_provenance_mut(address as usize)
}

/// Length of a NUL-terminated guest string, bounded.
///
/// # Safety
///
/// `address` must point at readable guest memory containing a NUL within [`MAX_STRING`]
/// bytes, the same contract the real function has.
pub(crate) unsafe fn c_len(address: u64) -> usize {
    if address == 0 {
        return 0;
    }
    let start = ptr(address);
    let mut len = 0;
    while len < MAX_STRING {
        // SAFETY: the caller guarantees readable memory up to the terminator, so every
        // offset up to and including it is in bounds.
        let at = unsafe { start.add(len) };
        // SAFETY: `at` is in bounds by the same guarantee, and one byte is readable.
        if unsafe { *at } == 0 {
            break;
        }
        len += 1;
    }
    len
}

fn memset(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (dest, byte, count) = (args[0], args[1] as u8, args[2] as usize);
    if dest != 0 && count > 0 {
        // SAFETY: the guest supplied destination and length, as the real call does, and the
        // identity mapping makes this the memory it named.
        unsafe { std::ptr::write_bytes(ptr(dest), byte, count) };
    }
    // Returns its destination, which callers chain on.
    dest
}

fn memcpy(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (dest, src, count) = (args[0], args[1], args[2] as usize);
    if dest != 0 && src != 0 && count > 0 {
        // SAFETY: guest-supplied pointers and length. `copy` rather than `copy_nonoverlapping`, so
        // a guest that overlaps gets a correct move rather than silent corruption.
        unsafe { std::ptr::copy(ptr(src), ptr(dest), count) };
    }
    dest
}

fn memmove(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    memcpy(args)
}

fn memcmp(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (a, b, count) = (args[0], args[1], args[2] as usize);
    if a == 0 || b == 0 || count == 0 {
        return 0;
    }
    // SAFETY: guest-supplied pointer and length, read only.
    let left = unsafe { std::slice::from_raw_parts(ptr(a), count) };
    // SAFETY: the other guest-supplied pointer, same contract.
    let right = unsafe { std::slice::from_raw_parts(ptr(b), count) };
    // Only the sign is specified; the byte difference matches callers that do arithmetic on it.
    for (x, y) in left.iter().zip(right) {
        if x != y {
            return i64::from(i32::from(*x) - i32::from(*y)) as u64;
        }
    }
    0
}

fn memchr(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (haystack, needle, count) = (args[0], args[1] as u8, args[2] as usize);
    if haystack == 0 || count == 0 {
        return 0;
    }
    // SAFETY: guest-supplied pointer and length, read only.
    let bytes = unsafe { std::slice::from_raw_parts(ptr(haystack), count) };
    bytes
        .iter()
        .position(|b| *b == needle)
        .map_or(0, |at| haystack + at as u64)
}

fn strlen(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // SAFETY: guest-supplied string, bounded by MAX_STRING.
    unsafe { c_len(args[0]) as u64 }
}

fn strnlen(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // SAFETY: guest-supplied string, bounded by its own limit and by ours.
    let len = unsafe { c_len(args[0]) };
    (len as u64).min(args[1])
}

fn strcmp(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let mut probe = [0_u64; GUEST_ARG_REGISTERS];
    // SAFETY: a guest-supplied string, bounded by MAX_STRING.
    let a = unsafe { c_len(args[0]) };
    // SAFETY: the other guest-supplied string, same contract.
    let b = unsafe { c_len(args[1]) };
    // The shorter length plus one, so the terminator is compared and two strings differing only
    // after their NUL compare equal.
    let n = a.min(b) + 1;
    probe[0] = args[0];
    probe[1] = args[1];
    probe[2] = n as u64;
    memcmp(&probe)
}

fn strncmp(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let mut probe = [0_u64; GUEST_ARG_REGISTERS];
    // SAFETY: a guest-supplied string, bounded by MAX_STRING.
    let a = unsafe { c_len(args[0]) };
    // SAFETY: the other guest-supplied string, same contract.
    let b = unsafe { c_len(args[1]) };
    // The shorter of the two plus its terminator, or the caller's limit, whichever is first.
    let n = (a.min(b) + 1).min(args[2] as usize);
    probe[0] = args[0];
    probe[1] = args[1];
    probe[2] = n as u64;
    memcmp(&probe)
}

fn strcpy(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (dest, src) = (args[0], args[1]);
    if dest == 0 || src == 0 {
        return dest;
    }
    // SAFETY: guest-supplied pointers; the terminator is copied with the string.
    let len = unsafe { c_len(src) };
    // SAFETY: as above, copying len+1 bytes to include the NUL.
    unsafe { std::ptr::copy(ptr(src), ptr(dest), len + 1) };
    dest
}

fn strncpy(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (dest, src, limit) = (args[0], args[1], args[2] as usize);
    if dest == 0 || src == 0 || limit == 0 {
        return dest;
    }
    // SAFETY: guest-supplied pointers.
    let len = unsafe { c_len(src) }.min(limit);
    // SAFETY: as above.
    unsafe { std::ptr::copy(ptr(src), ptr(dest), len) };
    if len < limit {
        // The standard pads the remainder with NUL, and callers rely on it.
        // SAFETY: the destination is at least `limit` bytes by the caller's contract.
        unsafe { std::ptr::write_bytes(ptr(dest + len as u64), 0, limit - len) };
    }
    dest
}

/// `strcpy_s(dest, destsz, src)` - the bounds-checked copy of Annex K, answering `errno_t`.
///
/// Copies `src`, terminator included, into `dest` when it fits in `destsz`, and answers `0`.
/// On a runtime-constraint violation (a null pointer, a zero or too-small `destsz`) it writes
/// an empty string to a usable `dest`, as the standard requires, and answers `EINVAL` or
/// `ERANGE`.
///
/// Reference: C11 Annex K, K.3.7.1.3 `strcpy_s`.
fn strcpy_s(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    /// `errno_t` for a null pointer or a zero destination size.
    const EINVAL: u64 = 22;
    /// `errno_t` for a destination too small to hold the source and its terminator.
    const ERANGE: u64 = 34;

    let (dest, destsz, src) = (args[0], args[1], args[2]);
    if dest == 0 || src == 0 || destsz == 0 {
        if dest != 0 && destsz != 0 {
            // SAFETY: `dest` is non-null and `destsz >= 1`, so its first byte is writable.
            unsafe { ptr(dest).write(0) };
        }
        return EINVAL;
    }
    // SAFETY: a guest-supplied string under the identity mapping, bounded by its terminator.
    let len = unsafe { c_len(src) };
    // The terminator has to fit as well, so the source needs `len + 1` bytes of room.
    if len + 1 > destsz as usize {
        // SAFETY: `dest` is non-null and `destsz >= 1`.
        unsafe { ptr(dest).write(0) };
        return ERANGE;
    }
    // SAFETY: both are guest-supplied pointers, and the check above established that `len + 1`
    // bytes fit within `destsz` at `dest`.
    unsafe { std::ptr::copy(ptr(src), ptr(dest), len + 1) };
    0
}

fn strcat(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (dest, src) = (args[0], args[1]);
    if dest == 0 || src == 0 {
        return dest;
    }
    // SAFETY: a guest-supplied string; this is where the append begins.
    let at = unsafe { c_len(dest) };
    // SAFETY: the other guest-supplied string, same contract.
    let len = unsafe { c_len(src) };
    // SAFETY: appending at the destination's terminator, including the source's own.
    unsafe { std::ptr::copy(ptr(src), ptr(dest + at as u64), len + 1) };
    dest
}

fn strchr(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (s, needle) = (args[0], args[1] as u8);
    if s == 0 {
        return 0;
    }
    // SAFETY: guest-supplied string. The terminator is searchable, as the standard requires:
    // `strchr(s, 0)` returns the end of the string, not null.
    let len = unsafe { c_len(s) } + 1;
    // SAFETY: len bytes are readable, terminator included.
    let bytes = unsafe { std::slice::from_raw_parts(ptr(s), len) };
    bytes
        .iter()
        .position(|b| *b == needle)
        .map_or(0, |at| s + at as u64)
}

fn strrchr(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (s, needle) = (args[0], args[1] as u8);
    if s == 0 {
        return 0;
    }
    // SAFETY: guest-supplied string, terminator included for the same reason as above.
    let len = unsafe { c_len(s) } + 1;
    // SAFETY: len bytes are readable.
    let bytes = unsafe { std::slice::from_raw_parts(ptr(s), len) };
    bytes
        .iter()
        .rposition(|b| *b == needle)
        .map_or(0, |at| s + at as u64)
}

/// `atexit(handler)`.
///
/// Accepts the registration and never runs it: nothing tears a guest down, and refusing makes
/// a guest believe its runtime failed to initialise.
fn atexit(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    0
}

/// `signal(sig, handler)` - records a handler and answers the one it replaced.
///
/// Nothing here delivers a signal. It must not fail: a network server's first act is
/// `signal(SIGPIPE, SIG_IGN)`, and `SIG_ERR` sends it down its error path. The answer is the
/// previous handler, `SIG_DFL` (zero) until something installs one.
///
/// Reference: FreeBSD `signal(3)`, POSIX.1-2008 `<signal.h>`.
fn signal(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    /// How many signal numbers to keep handlers for.
    ///
    /// FreeBSD defines signals 1 through 31 plus real-time ones above. A number past this gets
    /// `SIG_DFL` rather than a refusal, which would be an unmeasured claim about which signals
    /// exist.
    const SIGNALS: usize = 64;

    static HANDLERS: [std::sync::atomic::AtomicU64; SIGNALS] =
        [const { std::sync::atomic::AtomicU64::new(0) }; SIGNALS];

    let Ok(number) = usize::try_from(args[0]) else {
        return 0;
    };
    let Some(slot) = HANDLERS.get(number) else {
        return 0;
    };
    slot.swap(args[1], std::sync::atomic::Ordering::Relaxed)
}

/// `getopt(argc, argv, optstring)` - the POSIX option parser.
///
/// The position is kept here and reported through the guest's own `optarg` and `optind`,
/// globals the guest imports and this layer reserves storage for (D323). A guest that does
/// not import `optarg` gets no write.
///
/// Reference: POSIX.1-2008 `getopt(3)`, and FreeBSD's documented behaviour for the
/// leading-colon form.
fn getopt(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    /// What `getopt` answers when there is nothing left: `-1` as a 32-bit value, since the guest
    /// reads `eax`. Written out because `From` is not const.
    const DONE: u64 = 0xFFFF_FFFF;

    /// Where parsing has reached: one past the program name until something moves it.
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1);

    // Spelled out rather than destructured together: `argc` and `argv` differ by one character.
    let count = args[0];
    let vector = args[1];
    let options = args[2];

    // A wild count is refused rather than iterated: walking a pointer array sized by a stray
    // value would fault inside this call.
    let Ok(count) = usize::try_from(count) else {
        return DONE;
    };
    if count > MAX_ARGUMENTS || vector == 0 || options == 0 {
        return DONE;
    }

    let index = NEXT.load(std::sync::atomic::Ordering::Relaxed);
    // Publish the position the guest reads, whether or not anything is left.
    write_guest_word("optind", index as u64);

    if index >= count {
        return DONE;
    }

    // Option matching is not implemented: an argument that is present would need the letter
    // matched against `optstring` and `optarg` set, and answering without that would be wrong
    // rather than absent.
    DONE
}

/// `__error()` - a pointer to this thread's `errno`.
///
/// `errno` expands to `*__error()` on FreeBSD, so every guest use dereferences what comes
/// back, and the library owns real storage. Per thread: `errno` is thread-local by definition,
/// and a guest thread is a host thread here.
///
/// Reference: FreeBSD `errno(2)`, POSIX.1-2008 `<errno.h>`.
fn error_location(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    thread_local! {
        /// This thread's `errno`. A `thread_local` has an address stable for the life of the
        /// thread.
        static ERRNO: std::cell::UnsafeCell<i32> = const { std::cell::UnsafeCell::new(0) };
    }
    ERRNO.with(|cell| cell.get() as usize as u64)
}

/// The largest argument count this will walk.
///
/// A guard against a count that did not come from a real process image, possible whenever a
/// run enters somewhere other than the declared entry point.
const MAX_ARGUMENTS: usize = 1024;

/// Writes a word into a guest global this layer reserved storage for.
///
/// Does nothing when the guest does not import that name: there is nowhere for the value
/// to go.
pub(crate) fn write_guest_word(name: &str, value: u64) {
    let Some(at) = orbistoun_thunk::data_symbol(name) else {
        return;
    };
    let Ok(at) = usize::try_from(at) else {
        return;
    };
    // SAFETY: `data_symbol` returns the start of a page reserved read-write for this
    // import for as long as the guest runs, so eight bytes there are writable.
    unsafe { std::ptr::write(std::ptr::with_exposed_provenance_mut::<u64>(at), value) };
}

/// Bytes kept before every allocation, holding its size.
///
/// `free` is given only a pointer, so the size lives here. Sixteen because that is the
/// alignment `malloc` must return for any type on x86-64.
const HEAP_HEADER: usize = 16;

/// One machine word, which is what each half of the header holds.
const WORD: usize = size_of::<usize>();

/// Allocates `size` bytes whose address is a multiple of `align`.
///
/// `malloc` is this with `align` at the header size, and `memalign` with whatever the caller
/// asked for, so there is one header `free` reads (D128).
///
/// ```text
///   base                       body = base + offset
///   |                          |
///   v                          v
///   +-----------+--------------+---------------------------+
///   |  padding  |  header (16) |          payload          |
///   +-----------+--------------+---------------------------+
///                ^ total, offset
/// ```
///
/// `offset` equals the alignment the layout was built with, so it records both where the
/// allocation starts and the layout `dealloc` must be given.
pub(crate) fn allocate(size: usize, align: usize) -> u64 {
    // A zero request answers a unique, freeable one-byte allocation, as FreeBSD does; callers
    // treat null as failure (D383).
    let size = size.max(1);
    // At least the header, so the header fits between `base` and `body`; and a power of two.
    let align = align.max(HEAP_HEADER);
    if !align.is_power_of_two() {
        return 0;
    }
    let Some(total) = size.checked_add(align) else {
        return 0;
    };
    let Ok(layout) = std::alloc::Layout::from_size_align(total, align) else {
        return 0;
    };
    // A fixed-base region when one was asked for, and the host heap otherwise. Both hand back
    // `total` bytes aligned to `align`, so the header, `free` and `realloc` do not know which
    // answered.
    let base = match arena::take(total, align) {
        Some(address) => std::ptr::with_exposed_provenance_mut::<u8>(address as usize),
        // SAFETY: the layout has a non-zero size, which is `alloc`'s only requirement.
        None => unsafe { std::alloc::alloc(layout) },
    };
    if base.is_null() {
        // What a real allocator returns when it cannot allocate.
        return 0;
    }
    // SAFETY: `align <= total`, so this stays inside the allocation.
    let body = unsafe { base.add(align) };
    // SAFETY: `align >= HEAP_HEADER`, so the header sits at or after `base` and entirely before
    // `body`.
    let header = unsafe { body.sub(HEAP_HEADER) };
    // SAFETY: the header is sixteen writable bytes, and this writes the first eight, unaligned.
    unsafe { header.cast::<usize>().write_unaligned(total) };
    // SAFETY: `WORD` bytes into a sixteen-byte header, so the second word is in bounds.
    let second = unsafe { header.add(WORD) };
    // SAFETY: eight writable bytes, written unaligned.
    unsafe { second.cast::<usize>().write_unaligned(align) };
    if let Some(poison) = heap_fill() {
        // SAFETY: `size` bytes from `body` are the allocation just made, which nothing else holds
        // and the guest has not seen.
        unsafe { std::ptr::write_bytes(body, poison, size) };
        FILLED_ALLOCATIONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        FILLED_HEAP_BYTES.fetch_add(size as u64, std::sync::atomic::Ordering::Relaxed);
    }
    body as usize as u64
}

/// How many allocations this run filled, and how many bytes, so the report shows the fill ran
/// (D325).
static FILLED_ALLOCATIONS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// Bytes filled, alongside [`FILLED_ALLOCATIONS`].
static FILLED_HEAP_BYTES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// One line for a run report: what the fixed-base heap actually did.
///
/// [`None`] when none was asked for. A run that served nothing, or spilled into the host heap,
/// says so.
#[must_use]
pub fn heap_base_summary() -> Option<String> {
    arena::summary()
}

/// One line for a run report: what the heap fill actually did.
///
/// [`None`] when none was asked for. A run that asked and filled nothing says so.
#[must_use]
pub fn heap_fill_summary() -> Option<String> {
    use std::sync::atomic::Ordering;
    orbistoun_env::HEAP_FILL.get()?;
    let allocations = FILLED_ALLOCATIONS.load(Ordering::Relaxed);
    let bytes = FILLED_HEAP_BYTES.load(Ordering::Relaxed);
    Some(if allocations == 0 {
        "heap fill asked for and never fired - nothing was tested".to_owned()
    } else {
        format!("heap fill: {allocations} allocation(s), {bytes} bytes")
    })
}

/// What byte a fresh allocation is filled with before the guest sees it, if any.
///
/// Fresh host memory is almost always zero, so a guest reading a field nobody set looks like
/// one reading a deliberate zero; a fill tells them apart on the heap, as the stack fill does
/// on the stack. Zero is not a fill: it is what the host does anyway.
fn heap_fill() -> Option<u8> {
    static FILL: std::sync::OnceLock<Option<u8>> = std::sync::OnceLock::new();
    *FILL.get_or_init(|| {
        let raw = orbistoun_env::HEAP_FILL.get()?;
        let byte = u8::from_str_radix(raw.trim_start_matches("0x"), 16).ok()?;
        (byte != 0).then_some(byte)
    })
}

/// What was recorded when `body` was allocated: its whole size, and where it starts.
///
/// `None` for an address this library did not hand out, so the caller declines rather than
/// corrupting a heap it does not own.
fn header_of(body: u64) -> Option<(usize, usize)> {
    let body = usize::try_from(body).ok()?;
    let at = body.checked_sub(HEAP_HEADER)?;
    let header = std::ptr::with_exposed_provenance::<u8>(at);
    // SAFETY: reads the first word `allocate` wrote before the pointer it returned, in the form
    // it wrote it.
    let total = unsafe { header.cast::<usize>().read_unaligned() };
    // SAFETY: `WORD` bytes into the same sixteen-byte header.
    let second = unsafe { header.add(WORD) };
    // SAFETY: eight readable bytes, read in the form they were written.
    let offset = unsafe { second.cast::<usize>().read_unaligned() };
    // A block this library wrote always satisfies both, so a wild pointer becomes a no-op
    // rather than a `dealloc` against a layout nobody allocated.
    if offset < HEAP_HEADER || !offset.is_power_of_two() || total <= offset {
        return None;
    }
    Some((total, offset))
}

/// `malloc(size)` (D128).
fn malloc(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Ok(size) = usize::try_from(args[0]) else {
        return 0;
    };
    allocate(size, HEAP_HEADER)
}

/// `memalign(alignment, size)`.
///
/// Larger alignments cost nothing extra: the layout aligns the allocation rather than
/// over-allocating and rounding.
fn memalign(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (Ok(align), Ok(size)) = (usize::try_from(args[0]), usize::try_from(args[1])) else {
        return 0;
    };
    if !align.is_power_of_two() {
        // Every allocator interface requires a power of two; rounding would hide a caller's bug.
        return 0;
    }
    allocate(size, align)
}

/// `free(pointer)`.
///
/// Reads the size back out of the header `malloc` wrote.
pub(crate) fn free(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let pointer = args[0];
    if pointer == 0 {
        // Freeing null is defined and does nothing.
        return 0;
    }
    if arena::holds(pointer) {
        // Asked before the header is read: a fixed-region block carries the same header as a
        // host-heap one, and `dealloc` on reserved memory is undefined behaviour. The region never
        // reuses a block.
        return 0;
    }
    let Some((total, offset)) = header_of(pointer) else {
        return 0;
    };
    let Ok(body) = usize::try_from(pointer) else {
        return 0;
    };
    let Some(base) = body.checked_sub(offset) else {
        return 0;
    };
    let Ok(layout) = std::alloc::Layout::from_size_align(total, offset) else {
        return 0;
    };
    let base = std::ptr::with_exposed_provenance_mut::<u8>(base);
    // SAFETY: the layout is rebuilt from what `allocate` recorded, so it matches the one `alloc`
    // received, as `dealloc` requires.
    unsafe { std::alloc::dealloc(base, layout) };
    0
}

/// `calloc(count, size)`.
///
/// Zeroed, as callers rely on, and the multiplication is checked for overflow.
fn calloc(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (Ok(count), Ok(size)) = (usize::try_from(args[0]), usize::try_from(args[1])) else {
        return 0;
    };
    let Some(total) = count.checked_mul(size) else {
        return 0;
    };
    let mut request = [0_u64; GUEST_ARG_REGISTERS];
    request[0] = total as u64;
    let pointer = malloc(&request);
    if pointer != 0 && total > 0 {
        // SAFETY: `malloc` just returned `total` writable bytes at this address.
        unsafe { std::ptr::write_bytes(ptr(pointer), 0, total) };
    }
    pointer
}

/// `realloc(pointer, size)`.
///
/// Allocate, copy the smaller of the two sizes, free. A real allocator grows in place where
/// it can; nothing here is allocation-bound.
fn realloc(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (pointer, Ok(size)) = (args[0], usize::try_from(args[1])) else {
        return 0;
    };
    let mut request = [0_u64; GUEST_ARG_REGISTERS];
    request[0] = size as u64;
    if pointer == 0 {
        // `realloc(NULL, n)` is `malloc(n)`.
        return malloc(&request);
    }
    let fresh = malloc(&request);
    if fresh == 0 {
        // The original survives a failed realloc, since the caller still needs its data.
        return 0;
    }
    let Some((old_total, old_offset)) = header_of(pointer) else {
        return fresh;
    };
    let keep = old_total.saturating_sub(old_offset).min(size);
    if keep > 0 {
        // SAFETY: both allocations are at least `keep` bytes and do not overlap.
        unsafe { std::ptr::copy_nonoverlapping(ptr(pointer), ptr(fresh), keep) };
    }
    let mut release = [0_u64; GUEST_ARG_REGISTERS];
    release[0] = pointer;
    free(&release);
    fresh
}

/// The `sceLibcMspace*` family - the platform's exposed Doug Lea `mspace` allocator.
///
/// An mspace is an independent heap arena; these wrap dlmalloc's `mspace_*` calls, which take
/// the arena as a leading argument and are otherwise the ordinary allocator. Every mspace is
/// served from the one host heap and the arena handle is not consulted (D451): a guest touches
/// mspace memory only through this family, and `allocate` writes a header `free` reads back
/// whatever the arena.
///
/// Reference: Doug Lea's `dlmalloc` mspace interface (public domain), whose
/// `mspace_malloc(msp, bytes)` shape the platform names carry. `sceLibcMspaceMalloc`'s two
/// arguments were confirmed by an argument dump; the siblings follow the same shape.
fn mspace_malloc(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Ok(size) = usize::try_from(args[1]) else {
        return 0;
    };
    allocate(size, HEAP_HEADER)
}

/// `sceLibcMspaceCalloc(msp, count, size)` - zeroed, with the same arena handling as
/// [`mspace_malloc`]. The count and size trail the space, so they are `args[1]` and `args[2]`.
fn mspace_calloc(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let mut request = [0_u64; GUEST_ARG_REGISTERS];
    request[0] = args[1];
    request[1] = args[2];
    calloc(&request)
}

/// `sceLibcMspaceRealloc(msp, ptr, size)` - the pointer and size trail the space.
fn mspace_realloc(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let mut request = [0_u64; GUEST_ARG_REGISTERS];
    request[0] = args[1];
    request[1] = args[2];
    realloc(&request)
}

/// `sceLibcMspaceFree(msp, ptr)` - frees back to the shared heap; the header carries the size,
/// so which arena the guest thinks it belongs to does not matter.
fn mspace_free(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let mut request = [0_u64; GUEST_ARG_REGISTERS];
    request[0] = args[1];
    free(&request)
}

/// `__cxa_atexit(destructor, argument, dso_handle)`.
///
/// Registers a destructor for a C++ object with static storage duration, one per global
/// object a program constructs. Accepted and never run, as with `atexit`. Zero means
/// accepted; a non-zero answer makes a C++ runtime abort.
fn cxa_atexit(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    0
}

/// Byte within a guard variable that records whether initialisation has completed.
///
/// The Itanium C++ ABI puts the completion flag in the first byte on this architecture; the
/// remaining bytes are the implementation's.
const GUARD_DONE: u64 = 1;

/// `__cxa_guard_acquire(guard)`.
///
/// Asked before a function-local static is initialised. Non-zero means "not yet initialised,
/// go ahead"; zero means "already done". A placeholder error code is non-zero, so without a
/// real acquire and release every static would re-run its constructor on every visit.
fn cxa_guard_acquire(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let guard = args[0];
    if guard == 0 {
        return 0;
    }
    // SAFETY: the guest supplied the guard address, as the real call receives it; the identity
    // mapping makes this the byte it named.
    let done = unsafe { *ptr(guard) };
    u64::from(done == 0)
}

/// `__cxa_guard_release(guard)`.
///
/// Records that initialisation completed, so the static is never constructed twice.
fn cxa_guard_release(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let guard = args[0];
    if guard != 0 {
        // SAFETY: guest-supplied guard address, one byte written.
        unsafe { *ptr(guard) = GUARD_DONE as u8 };
    }
    0
}

/// `__cxa_guard_abort(guard)`.
///
/// Initialisation threw. The flag stays clear so the next attempt tries again, as the
/// standard requires.
fn cxa_guard_abort(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let guard = args[0];
    if guard != 0 {
        // SAFETY: guest-supplied guard address, one byte written.
        unsafe { *ptr(guard) = 0 };
    }
    0
}

/// Why a formatted write could not be honoured.
///
/// Enumerated, because the kinds need different responses: one is a conversion this could
/// support, another cannot be supported at this layer at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatFault {
    /// A conversion this does not implement.
    Unsupported(char),
    /// A floating-point conversion.
    ///
    /// Under System V a variadic floating-point argument arrives in an XMM register, and the
    /// trampoline captures the integer registers only, so the value never reaches this function
    /// (D183).
    FloatingPoint(char),
    /// The format called for more arguments than could be read.
    OutOfArguments,
}

/// What formatted writes could not do, across a run.
///
/// Counted rather than logged: how often and which conversion are the questions, and a line
/// per call would be unreadable at this volume.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct FormatStats {
    /// Formatted writes attempted.
    pub calls: u64,
    /// Writes that produced nothing because the format could not be honoured.
    pub refused: u64,
    /// Writes whose result did not fit and was cut short.
    pub truncated: u64,
    /// The first conversion that could not be honoured, if any.
    pub first_fault: Option<FormatFault>,
}

/// Running totals. Plain atomics: a guest thread must never block to be observed.
static FORMAT_CALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static FORMAT_REFUSED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static FORMAT_TRUNCATED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static FIRST_FAULT: std::sync::Mutex<Option<FormatFault>> = std::sync::Mutex::new(None);

/// What formatted writes did this run.
pub fn format_stats() -> FormatStats {
    use std::sync::atomic::Ordering::Relaxed;
    FormatStats {
        calls: FORMAT_CALLS.load(Relaxed),
        refused: FORMAT_REFUSED.load(Relaxed),
        truncated: FORMAT_TRUNCATED.load(Relaxed),
        first_fault: FIRST_FAULT.lock().ok().and_then(|f| *f),
    }
}

/// Whether a pointer a format wants to follow could be one.
///
/// Not a check against the guest's published ranges: a `%s` argument is often a pointer into
/// memory this project handed the guest (a `strerror` buffer, a `getifaddrs` block, a heap
/// allocation). Null, the null page and all-ones are not addresses any program computed;
/// everything else is followed and faults as the hardware would.
fn followable(address: u64) -> bool {
    /// The null page, which nothing maps and every small integer lands in.
    const NULL_PAGE: u64 = 0x1000;
    address >= NULL_PAGE && address != u64::MAX
}

/// Records the first thing a formatted write could not do.
fn note_fault(fault: FormatFault) {
    FORMAT_REFUSED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if let Ok(mut first) = FIRST_FAULT.lock() {
        first.get_or_insert(fault);
    }
}

/// Where a formatted write takes its arguments from.
///
/// The register forms are handed what the trampoline captured, and the `v` forms a `va_list`
/// cursor over the guest's spilled registers and stack. One renderer over both keeps them from
/// drifting (D364).
trait Arguments {
    /// The next integer-class argument, or [`None`] when there is not one.
    fn next_integer(&mut self) -> Option<u64>;
}

/// The arguments a trampoline caught in registers, and then the guest's stack.
///
/// System V passes the first six integer arguments in registers and the rest on the stack;
/// `snprintf` spends three registers on the buffer, size and format, so further conversions
/// are read from the stack.
struct Registers<'a> {
    /// The captured values, in order.
    values: &'a [u64],
    /// How many have been taken.
    taken: usize,
}

/// How many stack arguments one call may be asked for.
///
/// A ceiling, not a promise: only the format claims how many the guest passed, so this bounds
/// how far a wrong format can walk. More than any format needs, and less than a page.
const MOST_STACK_ARGUMENTS: usize = 64;

impl Arguments for Registers<'_> {
    fn next_integer(&mut self) -> Option<u64> {
        if let Some(value) = self.values.get(self.taken).copied() {
            self.taken += 1;
            return Some(value);
        }
        // Past the registers, the rest is where the psABI puts it: on the guest's stack, above the
        // return address, in order.
        let index = self.taken - self.values.len();
        if index >= MOST_STACK_ARGUMENTS {
            return None;
        }
        let area = orbistoun_thunk::stack_arguments();
        if area == 0 {
            // Not inside a guest call (tests, and rendering for this project's own reporting), so
            // there is no stack to read.
            return None;
        }
        let at = usize::try_from(area.saturating_add((index as u64).saturating_mul(8))).ok()?;
        self.taken += 1;
        // SAFETY: the guest's own stack under the identity mapping, inside the frame of the call
        // currently running, which the thunk published and has not returned from. Read unaligned,
        // so nothing depends on the slot's alignment.
        Some(unsafe { std::ptr::read_unaligned(std::ptr::with_exposed_provenance::<u64>(at)) })
    }
}

impl Arguments for varargs::VaList {
    fn next_integer(&mut self) -> Option<u64> {
        Self::next_integer(self)
    }
}

/// Everything between the `%` and the conversion character.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Specifier {
    /// `-`: pad on the right instead of the left.
    left: bool,
    /// `0`: pad with zeroes instead of spaces.
    zero: bool,
    /// The minimum field width.
    width: usize,
    /// The precision, where one was given. For `%s` it is a maximum length.
    precision: Option<usize>,
    /// How many of the argument's bits the conversion actually reads.
    width_bits: u32,
}

/// Reads one specifier, leaving the iterator on the conversion character.
///
/// Flags, width and precision are consumed whether or not they are honoured, so the
/// conversion itself is what gets reported.
fn read_specifier(chars: &mut std::iter::Peekable<impl Iterator<Item = u8>>) -> Specifier {
    let mut found = Specifier {
        left: false,
        zero: false,
        width: 0,
        precision: None,
        // The default is `int`, as C says, and a modifier widens or narrows it. A stack argument
        // sits in an eight-byte slot whose upper half is unspecified for anything narrower, so
        // reading the whole word would pick up another value's bits.
        width_bits: 32,
    };

    while let Some(&flag) = chars.peek() {
        match flag {
            b'-' => found.left = true,
            b'0' => found.zero = true,
            b'+' | b' ' | b'#' => {}
            _ => break,
        }
        chars.next();
    }
    found.width = read_number(chars);
    if chars.peek() == Some(&b'.') {
        chars.next();
        found.precision = Some(read_number(chars));
    }
    loop {
        match chars.peek() {
            // `l` is long and `ll` is long long; `size_t`, `intmax_t` and `ptrdiff_t` are the same
            // width on this data model, so they are one arm.
            Some(b'l' | b'z' | b'j' | b't') => found.width_bits = 64,
            // `h` is short and `hh` is char, so a second one narrows again.
            Some(b'h') => found.width_bits = if found.width_bits == 16 { 8 } else { 16 },
            // `L` belongs to a floating-point argument, which is refused either way.
            Some(b'L') => {}
            _ => break,
        }
        chars.next();
    }
    found
}

/// A run of decimal digits, or zero when there are none.
fn read_number(chars: &mut std::iter::Peekable<impl Iterator<Item = u8>>) -> usize {
    let mut value = 0_usize;
    while let Some(&digit) = chars.peek() {
        if !digit.is_ascii_digit() {
            break;
        }
        value = value
            .saturating_mul(10)
            .saturating_add(usize::from(digit - b'0'));
        chars.next();
    }
    value
}

/// The low `bits` of a value, which is what a conversion of that width received.
///
/// Sixty-four needs no mask, and shifting by the type's width would be undefined.
const fn narrow(value: u64, bits: u32) -> u64 {
    if bits >= 64 {
        return value;
    }
    value & ((1_u64 << bits) - 1)
}

/// The same value read as signed at that width.
///
/// `0xFFFF_FFFF` is `-1` as an `int` and `4294967295` as an `unsigned int`; only the conversion
/// says which.
const fn sign_extend(value: u64, bits: u32) -> i64 {
    if bits >= 64 {
        return value as i64;
    }
    let shift = 64 - bits;
    ((value << shift) as i64) >> shift
}

/// Renders a format string against the arguments that arrived in registers.
///
/// [`render_with`] is the same renderer over any argument source.
fn render_format(format: &[u8], args: &[u64]) -> Result<Vec<u8>, FormatFault> {
    render_with(
        format,
        &mut Registers {
            values: args,
            taken: 0,
        },
    )
}

/// Renders a format string against an argument source.
///
/// Refuses rather than rendering part of a format: a guest receiving `"texture_"` where it
/// expected `"texture_47.gnf"` opens the wrong file, while an empty string is wrong in a
/// bounded, immediate way (D183). Returns the whole rendering, ignoring any destination
/// limit; the caller applies truncation.
fn render_with(format: &[u8], args: &mut impl Arguments) -> Result<Vec<u8>, FormatFault> {
    let mut out = Vec::with_capacity(format.len());
    let mut chars = format.iter().copied().peekable();

    while let Some(byte) = chars.next() {
        if byte != b'%' {
            out.push(byte);
            continue;
        }
        let Specifier {
            left,
            zero,
            width,
            precision,
            width_bits,
        } = read_specifier(&mut chars);

        let Some(conversion) = chars.next() else {
            // A format ending in a bare `%` is malformed, and is a fault rather than dropped.
            return Err(FormatFault::Unsupported('%'));
        };
        if conversion == b'%' {
            out.push(b'%');
            continue;
        }
        if matches!(
            conversion,
            b'f' | b'F' | b'e' | b'E' | b'g' | b'G' | b'a' | b'A'
        ) {
            return Err(FormatFault::FloatingPoint(char::from(conversion)));
        }
        let Some(value) = args.next_integer() else {
            return Err(FormatFault::OutOfArguments);
        };
        // A pointer is always the whole word, whatever the modifier said.
        let value = if matches!(conversion, b's' | b'p') {
            value
        } else {
            narrow(value, width_bits)
        };

        let rendered: Vec<u8> = match conversion {
            b's' => {
                if value == 0 {
                    // What every common implementation does with a null.
                    b"(null)".to_vec()
                } else if !followable(value) {
                    // An address no program computed: what a register nothing set arrives holding.
                    // Following it would fault inside the renderer.
                    b"(bad pointer)".to_vec()
                } else {
                    // SAFETY: a guest-supplied string under the identity mapping, bounded by the
                    // limit every string function here uses.
                    let len = unsafe { c_len(value) };
                    let len = precision.map_or(len, |p| len.min(p));
                    // SAFETY: `c_len` established `len` readable bytes from `value`.
                    unsafe { std::slice::from_raw_parts(ptr(value).cast_const(), len) }.to_vec()
                }
            }
            b'c' => vec![value as u8],
            b'd' | b'i' => format!("{}", sign_extend(value, width_bits)).into_bytes(),
            b'u' => format!("{value}").into_bytes(),
            b'x' => format!("{value:x}").into_bytes(),
            b'X' => format!("{value:X}").into_bytes(),
            b'o' => format!("{value:o}").into_bytes(),
            b'p' => format!("{value:#x}").into_bytes(),
            other => return Err(FormatFault::Unsupported(char::from(other))),
        };

        // Integer and string conversions pad differently. ISO C 7.21.6.1 for `d i o u x X`:
        // precision is the minimum number of digits, default one, zero-filled on the left, and a
        // precision of zero with a value of zero renders nothing. A specified precision or the `-`
        // flag makes the `0` flag ignored, and zero padding goes after the sign.
        let numeric = matches!(conversion, b'd' | b'i' | b'u' | b'x' | b'X' | b'o');
        let (sign, digits): (&[u8], &[u8]) = if numeric && rendered.first() == Some(&b'-') {
            rendered.split_at(1)
        } else {
            (&[], &rendered)
        };
        let mut body: Vec<u8> = Vec::with_capacity(rendered.len());
        if numeric {
            if let Some(least) = precision {
                if least == 0 && digits == b"0" {
                    // The one case where a conversion renders no characters at all.
                } else {
                    body.extend(std::iter::repeat_n(
                        b'0',
                        least.saturating_sub(digits.len()),
                    ));
                    body.extend_from_slice(digits);
                }
            } else {
                body.extend_from_slice(digits);
            }
        } else {
            body.extend_from_slice(&rendered);
        }

        let pad = width.saturating_sub(sign.len() + body.len());
        let zero_fill = zero && !left && !(numeric && precision.is_some());
        if left {
            out.extend_from_slice(sign);
            out.extend_from_slice(&body);
            out.extend(std::iter::repeat_n(b' ', pad));
        } else if zero_fill {
            // The sign first, then the zeros: `-0042`, never `00-42`.
            out.extend_from_slice(sign);
            out.extend(std::iter::repeat_n(b'0', pad));
            out.extend_from_slice(&body);
        } else {
            out.extend(std::iter::repeat_n(b' ', pad));
            out.extend_from_slice(sign);
            out.extend_from_slice(&body);
        }
    }
    // Every renderer reaches here, so a message is recorded whether it was printed, written to a
    // descriptor, or formatted into a buffer the guest keeps.
    said::note(&out);
    Ok(out)
}

/// `snprintf_s(dest, size, format, ...)`, the C11 Annex K bounds-checked variant.
///
/// A format it cannot honour completely produces an empty, terminated destination and a zero
/// return, and is counted (D183). Annex K is optional and implementations differ, so what the
/// target library returns on truncation is unverified; the knowledge file records that.
fn snprintf_s(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    use std::sync::atomic::Ordering::Relaxed;

    let (dest, size, format) = (args[0], args[1] as usize, args[2]);
    FORMAT_CALLS.fetch_add(1, Relaxed);

    // A size of zero is a question, not a refusal: under ISO C 7.21.6.5 nothing is written, `s`
    // may be null, and the return is still the length the output would need, which is how a
    // caller sizes a buffer.
    if dest == 0 && size != 0 {
        return 0;
    }
    if format == 0 {
        note_fault(FormatFault::Unsupported('\0'));
        if size != 0 {
            // SAFETY: `dest` is non-null with at least one byte, per the size the guest passed; the
            // null-destination case returned above.
            unsafe { std::ptr::write(ptr(dest), 0) };
        }
        return 0;
    }

    // SAFETY: a guest-supplied string under the identity mapping, bounded.
    let len = unsafe { c_len(format) };
    // SAFETY: `c_len` established `len` readable bytes from `format`.
    let template = unsafe { std::slice::from_raw_parts(ptr(format).cast_const(), len) };

    let rendered = match render_format(template, &args[3..]) {
        Ok(text) => {
            // Captured here, where the words exist, so a message handed to an unimplemented write
            // path is still seen (D658).
            orbistoun_core::said::note(&text);
            text
        }
        Err(fault) => {
            note_fault(fault);
            if size != 0 {
                // SAFETY: `dest` is non-null with at least one byte.
                unsafe { std::ptr::write(ptr(dest), 0) };
            }
            return 0;
        }
    };

    // Nothing to write into, so nothing is written - and the length is still reported.
    if size == 0 {
        FORMAT_TRUNCATED.fetch_add(1, Relaxed);
        return rendered.len() as u64;
    }

    // One byte reserved for the terminator.
    let room = size - 1;
    let copied = rendered.len().min(room);
    if copied < rendered.len() {
        FORMAT_TRUNCATED.fetch_add(1, Relaxed);
    }
    // SAFETY: `copied` is at most `size - 1`, so the write and its terminator both fall
    // inside the buffer the guest described.
    unsafe {
        std::ptr::copy_nonoverlapping(rendered.as_ptr(), ptr(dest), copied);
    }
    // SAFETY: `copied` is at most `size - 1`, so this offset is inside the buffer.
    let end = unsafe { ptr(dest).add(copied) };
    // SAFETY: `end` is in bounds by the same reasoning, and one byte is writable.
    unsafe { std::ptr::write(end, 0) };

    // The full length, as the interface reports, so a caller can detect truncation.
    rendered.len() as u64
}

/// `sprintf(dest, format, ...)` - unbounded.
///
/// No size argument, so nothing here can stop an overrun: the guest promised the buffer is
/// large enough.
fn sprintf(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    use std::sync::atomic::Ordering::Relaxed;

    let (dest, format) = (args[0], args[1]);
    FORMAT_CALLS.fetch_add(1, Relaxed);
    if dest == 0 || format == 0 {
        return 0;
    }
    // SAFETY: a guest-supplied string under the identity mapping, bounded.
    let len = unsafe { c_len(format) };
    // SAFETY: `c_len` established `len` readable bytes from `format`.
    let template = unsafe { std::slice::from_raw_parts(ptr(format).cast_const(), len) };
    let Ok(rendered) = render_format(template, &args[2..]) else {
        // SAFETY: `dest` is non-null, and a caller promised a buffer.
        unsafe { std::ptr::write(ptr(dest), 0) };
        return 0;
    };
    // SAFETY: the guest promised a buffer large enough; that promise is the interface and
    // cannot be checked here.
    unsafe {
        std::ptr::copy_nonoverlapping(rendered.as_ptr(), ptr(dest), rendered.len());
    }
    // SAFETY: one past the rendered text, inside the same promised buffer.
    let end = unsafe { ptr(dest).add(rendered.len()) };
    // SAFETY: `end` is in bounds by the same promise, and one byte is writable.
    unsafe { std::ptr::write(end, 0) };
    rendered.len() as u64
}

/// Allocates `len + 1` bytes and copies a terminated string into them.
fn copy_into_new(from: u64, len: usize) -> u64 {
    let block = allocate(len + 1, HEAP_HEADER);
    if block == 0 {
        return 0;
    }
    if len > 0 {
        // SAFETY: `len` bytes are readable from `from` by the scan that measured them, and
        // the allocation is `len + 1` bytes of memory this process owns.
        unsafe { std::ptr::copy_nonoverlapping(ptr(from).cast_const(), ptr(block), len) };
    }
    // SAFETY: the allocation has room for the terminator by construction.
    let end = unsafe { ptr(block).add(len) };
    // SAFETY: `end` is inside the allocation, and one byte is writable.
    unsafe { std::ptr::write(end, 0) };
    block
}

/// `strdup(text)` - a copy in freshly allocated memory.
///
/// The caller frees it, so it comes from [`allocate`], the allocator `free` understands.
fn strdup(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // SAFETY: a guest-supplied string under the identity mapping, bounded.
    let len = unsafe { c_len(args[0]) };
    copy_into_new(args[0], len)
}

/// `strndup(text, n)` - at most `n` bytes, always terminated.
fn strndup(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // SAFETY: the same contract as `strdup`.
    let len = unsafe { c_len(args[0]) }.min(usize::try_from(args[1]).unwrap_or(usize::MAX));
    copy_into_new(args[0], len)
}

/// `strncat(dest, src, n)` - appends at most `n` bytes, then a terminator.
///
/// `n` bounds the source, not the result, as the standard defines.
fn strncat(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // SAFETY: a guest-supplied string under the identity mapping, bounded.
    let at = unsafe { c_len(args[0]) };
    // SAFETY: the other guest-supplied string, same contract.
    let from = unsafe { c_len(args[1]) };
    let take = from.min(usize::try_from(args[2]).unwrap_or(usize::MAX));
    if take > 0 {
        // SAFETY: the guest promised `dest` has room after its own length, the interface's
        // contract.
        let tail = unsafe { ptr(args[0]).add(at) };
        // SAFETY: `take` bytes are readable from `src`, and `tail` has room for them.
        unsafe {
            std::ptr::copy_nonoverlapping(ptr(args[1]).cast_const(), tail, take);
        }
    }
    // SAFETY: one byte past what was written, inside the promised buffer.
    let end = unsafe { ptr(args[0]).add(at + take) };
    // SAFETY: `end` is in bounds by the same promise, and one byte is writable.
    unsafe { std::ptr::write(end, 0) };
    args[0]
}

/// Where `strtok` keeps its place between calls.
///
/// A static, as the interface specifies: a second thread calling this mid-walk gets the first
/// one's position, as it would on the target.
static STRTOK_STATE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Finds the next token from `start`, terminating it in place, and where to resume.
///
/// Writes a NUL into the guest's own buffer, as `strtok` does.
fn next_token(start: u64, delimiters: u64) -> (u64, u64) {
    if start == 0 {
        return (0, 0);
    }
    // SAFETY: a guest-supplied string under the identity mapping, bounded.
    let len = unsafe { c_len(start) };
    // SAFETY: the delimiter set, same contract.
    let delim_len = unsafe { c_len(delimiters) };
    // SAFETY: both lengths were established by scanning to their terminators.
    let text = unsafe { std::slice::from_raw_parts(ptr(start).cast_const(), len) };
    // SAFETY: the same, for the delimiter set.
    let delims = unsafe { std::slice::from_raw_parts(ptr(delimiters).cast_const(), delim_len) };
    let Some(begin) = text.iter().position(|b| !delims.contains(b)) else {
        // Nothing but delimiters left: the walk is over, and a null resume point makes the next
        // call end too.
        return (0, 0);
    };
    let end = text[begin..]
        .iter()
        .position(|b| delims.contains(b))
        .map_or(len, |at| begin + at);
    if end < len {
        // SAFETY: `end` is inside the guest's own writable string; a caller passing a literal has
        // broken the interface's contract.
        let at = unsafe { ptr(start).add(end) };
        // SAFETY: `at` is in bounds by the same reasoning, and one byte is writable.
        unsafe { std::ptr::write(at, 0) };
        return (start + begin as u64, start + end as u64 + 1);
    }
    (start + begin as u64, 0)
}

/// `strtok(text, delimiters)` - the next token, or null.
fn strtok(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    use std::sync::atomic::Ordering::Relaxed;

    let start = if args[0] == 0 {
        STRTOK_STATE.load(Relaxed)
    } else {
        args[0]
    };
    let (token, rest) = next_token(start, args[1]);
    STRTOK_STATE.store(rest, Relaxed);
    token
}

/// `strtok_r(text, delimiters, save)` - the same walk, with the caller holding the place.
fn strtok_r(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let save = args[2];
    let start = if args[0] == 0 {
        let Ok(at) = usize::try_from(save) else {
            return 0;
        };
        if save == 0 {
            return 0;
        }
        // SAFETY: a guest-supplied `char **` under the identity mapping.
        unsafe { std::ptr::read_unaligned(std::ptr::with_exposed_provenance::<u64>(at)) }
    } else {
        args[0]
    };
    let (token, rest) = next_token(start, args[1]);
    if save != 0 {
        if let Ok(at) = usize::try_from(save) {
            // SAFETY: the same guest-supplied `char **`, written only when non-null.
            unsafe {
                std::ptr::write_unaligned(std::ptr::with_exposed_provenance_mut::<u64>(at), rest);
            }
        }
    }
    token
}

// Calling back into the guest.

/// A guest comparison function: `int (*)(const void *, const void *)`.
///
/// `qsort` and `bsearch` call a function pointer into the guest's own code (D274).
/// `extern "sysv64"` emits the guest's convention, and the thunk dispatch is re-entrant, so a
/// comparator that calls an import lands back here as an ordinary nested call.
type GuestComparator = extern "sysv64" fn(u64, u64) -> u64;

/// Turns a guest address into something callable.
///
/// # Safety
///
/// `address` must be a function in the guest's own fully relocated image, taking two pointers
/// under System V, which is what the caller promised by passing it as a comparator.
unsafe fn comparator(address: u64) -> GuestComparator {
    // SAFETY: the caller guarantees a guest function of this shape.
    unsafe { std::mem::transmute::<u64, GuestComparator>(address) }
}

/// The sign of a comparison, as C reports it.
fn compare_at(f: GuestComparator, a: u64, b: u64) -> std::cmp::Ordering {
    // The guest answers an `int`; a whole-word test would read every negative result as
    // positive.
    let raw = f(a, b) as u32 as i32;
    raw.cmp(&0)
}

/// `qsort(base, count, size, compare)`.
///
/// Sorts an index permutation, then applies it, so the array does not move under the
/// comparator while comparisons run. Stricter than the real implementation promises.
fn qsort(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (base, count, size, compare) = (args[0], args[1], args[2], args[3]);
    let (Ok(count), Ok(size)) = (usize::try_from(count), usize::try_from(size)) else {
        return 0;
    };
    if base == 0 || compare == 0 || count < 2 || size == 0 {
        return 0;
    }
    // SAFETY: the guest passed this as its comparator, which is the promise this needs.
    let f = unsafe { comparator(compare) };

    let mut order: Vec<usize> = (0..count).collect();
    order.sort_by(|a, b| compare_at(f, base + (*a * size) as u64, base + (*b * size) as u64));

    // Copied out first: applying a permutation in place needs the original.
    let total = count * size;
    // SAFETY: the guest described `count` elements of `size` bytes at `base`, the interface's
    // contract and the only description of the buffer there is.
    let original = unsafe { std::slice::from_raw_parts(ptr(base).cast_const(), total) }.to_vec();
    for (to, from) in order.iter().enumerate() {
        let src = &original[from * size..from * size + size];
        // SAFETY: `to` is below `count`, so this lands inside the same described buffer.
        let dest = unsafe { ptr(base).add(to * size) };
        // SAFETY: `size` bytes from the copy taken before anything was written back, into the slot
        // just bounded.
        unsafe {
            std::ptr::copy_nonoverlapping(src.as_ptr(), dest, size);
        }
    }
    0
}

/// `setlocale(category, locale)` - the locale in force, as a string.
///
/// Always `"C"`: a program that has not set a locale is in the C locale, and orbistoun
/// implements no other. The answer is a pointer the guest dereferences (D125). The string is
/// leaked once and never freed, since a guest may hold the pointer for its whole life.
fn setlocale(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    use std::sync::OnceLock;
    static C_LOCALE: OnceLock<u64> = OnceLock::new();
    *C_LOCALE.get_or_init(|| {
        let leaked: &'static mut [u8; 2] = Box::leak(Box::new(*b"C "));
        std::ptr::from_mut(leaked) as usize as u64
    })
}

/// `clock()` - processor time used, in `CLOCKS_PER_SEC` units.
///
/// Elapsed since the process started, as `sceKernelGetProcessTime` reports, so two runs of one
/// title compare. The unit is an assumption: microseconds, on POSIX's rule that
/// `CLOCKS_PER_SEC` is 1,000,000, where FreeBSD's headers have defined it as 128.
fn clock(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    use std::sync::OnceLock;
    static START: OnceLock<std::time::Instant> = OnceLock::new();
    let start = START.get_or_init(std::time::Instant::now);
    u64::try_from(start.elapsed().as_micros()).unwrap_or(u64::MAX)
}

/// `setjmp(env)` - zero, because this is the direct return.
///
/// Saves nothing: a `longjmp` into this buffer would jump through uninitialised memory, and
/// `longjmp` is not implemented. A non-zero answer would tell the guest it arrived via
/// `longjmp`, so zero is the true answer about which return this is.
fn setjmp(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    0
}

/// `bsearch(key, base, count, size, compare)` - the matching element, or null.
///
/// Answers a pointer, so a miss is null and never an error code (D125).
fn bsearch(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (key, base, count, size, compare) = (args[0], args[1], args[2], args[3], args[4]);
    let (Ok(count), Ok(size)) = (usize::try_from(count), usize::try_from(size)) else {
        return 0;
    };
    if key == 0 || base == 0 || compare == 0 || size == 0 {
        return 0;
    }
    // SAFETY: the guest passed this as its comparator.
    let f = unsafe { comparator(compare) };

    let (mut low, mut high) = (0_usize, count);
    while low < high {
        let mid = low + (high - low) / 2;
        let at = base + (mid * size) as u64;
        match compare_at(f, key, at) {
            std::cmp::Ordering::Equal => return at,
            std::cmp::Ordering::Less => high = mid,
            std::cmp::Ordering::Greater => low = mid + 1,
        }
    }
    0
}

/// `printf(format, ...)`.
///
/// A guest that gives up usually says why first, and this is how. Output goes to the host's
/// error stream (D170): a worker's standard output carries the protocol its parent parses.
/// Refuses the formats [`render_format`] refuses, since a half-rendered diagnostic is worse
/// than none.
fn printf(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    use std::io::Write as _;
    use std::sync::atomic::Ordering::Relaxed;

    let format = args[0];
    FORMAT_CALLS.fetch_add(1, Relaxed);
    if format == 0 {
        note_fault(FormatFault::Unsupported('\0'));
        return 0;
    }

    // SAFETY: a guest-supplied string under the identity mapping, bounded.
    let len = unsafe { c_len(format) };
    // SAFETY: `c_len` established `len` readable bytes from `format`.
    let template = unsafe { std::slice::from_raw_parts(ptr(format).cast_const(), len) };

    let rendered = match render_format(template, &args[1..]) {
        Ok(text) => {
            // Captured here, where the words exist, so a message handed to an unimplemented write
            // path is still seen (D658).
            orbistoun_core::said::note(&text);
            text
        }
        Err(fault) => {
            // Recorded rather than printed: a mangled diagnostic is not what the guest meant.
            note_fault(fault);
            return 0;
        }
    };

    // Guest output, not a log: the guest's own write, so it stays a direct write.
    let mut err = std::io::stderr();
    let _ = err.write_all(&rendered);
    let _ = err.flush();
    rendered.len() as u64
}

/// `strerror(errnum)` - a pointer to a message describing an error number.
///
/// The platform's message table is not measured, so the text says what it is rather than
/// imitating one. What matters is that the answer is a real, readable pointer, since callers
/// print it unchecked. The buffer is per thread, so two guest threads reporting different
/// failures do not overwrite each other's text.
///
/// Reference: POSIX.1-2008 `strerror(3)`.
fn strerror(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    /// Enough for the longest message this writes, with room to spare.
    const ROOM: usize = 64;

    thread_local! {
        /// This thread's message buffer, with an address stable for the life of the thread.
        static MESSAGE: std::cell::UnsafeCell<[u8; ROOM]> =
            const { std::cell::UnsafeCell::new([0; ROOM]) };
    }

    MESSAGE.with(|cell| {
        let text = format!(
            "error {} (orbistoun has no message table)\0",
            args[0] as i32
        );
        let bytes = text.as_bytes();
        let at = cell.get();
        let room = bytes.len().min(ROOM - 1);
        let start = at.cast::<u8>();
        // SAFETY: `at` points at a `ROOM`-byte array owned by this thread and borrowed by nothing
        // else here, and `room` is capped below `ROOM`.
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), start, room) };
        // SAFETY: `room < ROOM`, so this is the terminator inside the same array.
        let end = unsafe { start.add(room) };
        // SAFETY: as above - one byte inside the array this thread owns.
        unsafe { std::ptr::write(end, 0) };
        at as usize as u64
    })
}

/// `fprintf(stream, format, ...)` - `printf` with a stream in front.
///
/// A stream that wraps a descriptor is written to that descriptor; anything else goes to the
/// host's error stream, as `printf` does.
fn fprintf(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    use std::sync::atomic::Ordering::Relaxed;

    // A stream that is a descriptor goes to the descriptor: a server wraps an accepted
    // connection with `fdopen` and writes its replies with `fprintf`.
    let Some(fd) = orbistoun_fs::open::wrapped_descriptor(args[0]) else {
        let mut shifted = [0_u64; GUEST_ARG_REGISTERS];
        shifted[..GUEST_ARG_REGISTERS - 1].copy_from_slice(&args[1..]);
        return printf(&shifted);
    };

    let format = args[1];
    FORMAT_CALLS.fetch_add(1, Relaxed);
    if format == 0 {
        note_fault(FormatFault::Unsupported('\0'));
        return 0;
    }
    // SAFETY: a guest-supplied string under the identity mapping, bounded.
    let len = unsafe { c_len(format) };
    // SAFETY: `c_len` established `len` readable bytes from `format`.
    let template = unsafe { std::slice::from_raw_parts(ptr(format).cast_const(), len) };
    let rendered = match render_format(template, &args[2..]) {
        Ok(text) => {
            // Captured here, where the words exist, so a message handed to an unimplemented write
            // path is still seen (D658).
            orbistoun_core::said::note(&text);
            text
        }
        Err(fault) => {
            note_fault(fault);
            return 0;
        }
    };
    orbistoun_fs::descriptor::write(fd, &rendered).map_or(0, |written| written as u64)
}

/// Renders a guest's format string against a guest's `va_list`.
///
/// The shared half of the `v` forms, which differ only in where the bytes go. Answers
/// [`None`] when the format could not be honoured completely, having recorded why.
fn render_va(format: u64, ap: u64) -> Option<Vec<u8>> {
    use std::sync::atomic::Ordering::Relaxed;

    FORMAT_CALLS.fetch_add(1, Relaxed);
    if format == 0 {
        note_fault(FormatFault::Unsupported('\0'));
        return None;
    }
    // SAFETY: a guest-supplied `va_list` under the identity mapping. A null one is answered
    // without being dereferenced.
    let Some(mut list) = (unsafe { varargs::VaList::read(ap) }) else {
        // A null list with a format that wants arguments is out-of-arguments, counted with its
        // register-form twin.
        note_fault(FormatFault::OutOfArguments);
        return None;
    };
    // SAFETY: a guest-supplied string under the identity mapping, bounded.
    let len = unsafe { c_len(format) };
    // SAFETY: `c_len` established `len` readable bytes from `format`.
    let template = unsafe { std::slice::from_raw_parts(ptr(format).cast_const(), len) };

    match render_with(template, &mut list) {
        Ok(text) => Some(text),
        Err(fault) => {
            note_fault(fault);
            None
        }
    }
}

/// `vsprintf_s(buffer, count, format, argptr)` - the bounds-checked `vsprintf` of Annex K.
///
/// The same observable effect as [`vsnprintf`], so it delegates to it. The `_s` variant adds
/// only runtime-constraint handlers, which a caller passing a valid buffer and format never
/// invokes.
///
/// Reference: C11 Annex K `vsprintf_s`.
fn vsprintf_s(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    vsnprintf(args)
}

/// `vsnprintf(dest, size, format, ap)` - the bounded `va_list` form, which logging helpers are
/// built on (D364).
///
/// `vsnprintf(NULL, 0, format, ap)` asks how long the answer would be before allocating, so it
/// writes nothing and answers the full length; the return is always the rendered length.
///
/// Reference: ISO C `vsnprintf`; POSIX.1-2008 `vsnprintf(3)`.
fn vsnprintf(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    use std::sync::atomic::Ordering::Relaxed;

    let (dest, size, format, ap) = (args[0], args[1] as usize, args[2], args[3]);
    // A destination that cannot be one is not written to: all-ones is what a register nothing
    // set arrives holding.
    let dest = if dest == u64::MAX { 0 } else { dest };
    let Some(rendered) = render_va(format, ap) else {
        // Terminated where there is somewhere to terminate, so a caller that prints the destination
        // regardless prints nothing.
        if dest != 0 && size > 0 {
            // SAFETY: `dest` is non-null with at least one byte, per the size the guest passed.
            unsafe { std::ptr::write(ptr(dest), 0) };
        }
        return 0;
    };

    if dest != 0 && size > 0 {
        // One byte reserved for the terminator.
        let copied = rendered.len().min(size - 1);
        if copied < rendered.len() {
            FORMAT_TRUNCATED.fetch_add(1, Relaxed);
        }
        // SAFETY: `copied` is at most `size - 1`, so the copy falls inside the buffer the
        // guest described.
        unsafe { std::ptr::copy_nonoverlapping(rendered.as_ptr(), ptr(dest), copied) };
        // SAFETY: `copied` is at most `size - 1`, so this offset is inside the same buffer.
        let end = unsafe { ptr(dest).add(copied) };
        // SAFETY: `end` is in bounds by the line above, and one byte there is writable.
        unsafe { std::ptr::write(end, 0) };
    }

    // The length the whole rendering would have been, which the measure-then-allocate idiom
    // depends on.
    rendered.len() as u64
}

/// `vprintf(format, ap)` - the `va_list` form of [`printf`], to the host's error stream.
///
/// Reference: ISO C `vprintf`; POSIX.1-2008 `vprintf(3)`.
fn vprintf(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    use std::io::Write as _;

    let Some(rendered) = render_va(args[0], args[1]) else {
        return 0;
    };
    // Guest output, not a log: the guest's own write, so it stays a direct write.
    let mut err = std::io::stderr();
    let _ = err.write_all(&rendered);
    let _ = err.flush();
    rendered.len() as u64
}

/// `vfprintf(stream, format, ap)` - [`vprintf`] with a stream in front.
///
/// The stream is honoured as [`fprintf`] honours it: a stream that stands for a descriptor is
/// written to that descriptor, and anything else goes to the host's error stream.
///
/// Reference: ISO C `vfprintf`; POSIX.1-2008 `vfprintf(3)`.
fn vfprintf(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if let Some(fd) = orbistoun_fs::open::wrapped_descriptor(args[0]) {
        let Some(rendered) = render_va(args[1], args[2]) else {
            return 0;
        };
        return orbistoun_fs::descriptor::write(fd, &rendered).map_or(0, |written| written as u64);
    }
    let mut shifted = [0_u64; GUEST_ARG_REGISTERS];
    shifted[..GUEST_ARG_REGISTERS - 1].copy_from_slice(&args[1..]);
    vprintf(&shifted)
}

/// `puts(s)` - writes a string and a newline.
///
/// Not a format: a guest's own text containing a percent sign is written as it is.
///
/// Reference: POSIX.1-2008 `puts(3)`. Answers a non-negative number on success; the byte count
/// is a permitted choice.
fn puts(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    use std::io::Write as _;

    let text = args[0];
    if text == 0 {
        return 0;
    }
    // SAFETY: a guest-supplied string under the identity mapping, bounded.
    let len = unsafe { c_len(text) };
    // SAFETY: `c_len` established `len` readable bytes from `text`.
    let bytes = unsafe { std::slice::from_raw_parts(ptr(text).cast_const(), len) };

    // Guest output, not a log: the guest's own write, so it stays a direct write.
    let mut err = std::io::stderr();
    let _ = err.write_all(bytes);
    let _ = err.write_all(b"\n");
    let _ = err.flush();
    len as u64 + 1
}

/// `putchar(c)` - writes one byte to the output stream and returns it.
///
/// Routed to the host's error stream, as [`puts`] and [`printf`] are.
///
/// Reference: ISO C `putchar`; POSIX.1-2008 `putchar(3)`. Answers the byte written as an
/// `unsigned char` widened to `int`; the write cannot be refused, so `EOF` is not produced.
fn putchar(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    use std::io::Write as _;

    let byte = args[0] as u8;
    // Guest output, not a log: the guest's own write, so it stays a direct write.
    let mut err = std::io::stderr();
    let _ = err.write_all(&[byte]);
    let _ = err.flush();
    u64::from(byte)
}

/// `std::_Random_device()` - the entropy source `std::random_device` reads from.
///
/// Deterministic, so two runs of one build behave identically: a guest reads this to seed its
/// own generator and needs a well-distributed 32-bit value, not a physically random one. The
/// stream is `orbistoun_core::entropy`, shared with the random devices (D578).
///
/// Reference: C++ `std::random_device` (`[rand.device]`).
fn random_device(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // The low 32 bits: `std::random_device::result_type` is `unsigned int`.
    u64::from(orbistoun_core::entropy::next_word() as u32)
}

/// `getpid()` - the process the guest is running in.
///
/// The host process id, which is the guest's process id in every sense that can be checked
/// from inside it.
///
/// Reference: POSIX.1-2008 `getpid(3)`.
fn getpid(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    u64::from(std::process::id())
}

/// `kill(pid, sig)` - answers signal zero, and refuses any real signal.
///
/// The guest runs inside this process, so a real `kill` would signal the emulator. Signal
/// zero asks whether a process exists and may be signalled; the guest's own process is the
/// only one it can name, so that is answered and every other id fails. Nothing here delivers
/// signals, and reporting success would tell a guest it had ended something still running.
///
/// Reference: POSIX.1-2008 `kill(2)`.
fn kill(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (pid, signal) = (args[0] as i64, args[1]);
    if signal != 0 {
        return FAILED;
    }
    // A guest asking about itself gets a true yes. `pid` zero and negative values name process
    // groups, which are not modelled.
    if pid > 0 && u64::try_from(pid) == Ok(u64::from(std::process::id())) {
        OK
    } else {
        FAILED
    }
}

/// `getenv(name)` - a variable out of the environment the guest was given.
///
/// A run's environment is empty unless `config.toml` sets `entry.environment`; nothing here
/// invents what the platform might set. Anything else answers null, as for an unset variable.
///
/// Reference: POSIX.1-2008 `getenv(3)`.
fn getenv(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if args[0] == 0 {
        return 0;
    }
    // SAFETY: a guest-supplied string under the identity mapping, bounded.
    let len = unsafe { c_len(args[0]) };
    // SAFETY: `c_len` established `len` readable bytes.
    let wanted = unsafe { std::slice::from_raw_parts(ptr(args[0]).cast_const(), len) };
    let Ok(wanted) = std::str::from_utf8(wanted) else {
        return 0;
    };
    environment_value(wanted).map_or(0, |value| value as usize as u64)
}

/// The guest's environment as this library answers it: what it was handed, plus anything it
/// has set since, in one table so `getenv` reads back what `setenv` wrote.
static ENVIRONMENT: std::sync::Mutex<Option<std::collections::BTreeMap<String, &'static [u8]>>> =
    std::sync::Mutex::new(None);

/// The value of a variable in the guest's environment, as a stable address.
///
/// Cached per name, because `getenv` answers a pointer the caller may keep.
fn environment_value(name: &str) -> Option<*const u8> {
    let mut guard = ENVIRONMENT.lock().ok()?;
    let answered = guard.get_or_insert_with(std::collections::BTreeMap::new);
    if let Some(found) = answered.get(name) {
        return Some(found.as_ptr());
    }
    let prefix = format!("{name}=");
    let value = orbistoun_thunk::guest_environment()
        .into_iter()
        .find_map(|entry| entry.strip_prefix(&prefix).map(str::to_owned))?;
    let stored: &'static [u8] = Box::leak(format!("{value}\0").into_bytes().into_boxed_slice());
    answered.insert(name.to_owned(), stored);
    Some(stored.as_ptr())
}

/// `setenv(name, value, overwrite)` - POSIX.1-2008 `setenv(3)`.
///
/// Written into the table [`environment_value`] answers from, and leaked for the same reason.
/// An `overwrite` of zero leaves an existing variable alone and still reports success, as the
/// standard says. An empty name, or one containing `=`, is `EINVAL`.
fn setenv(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (name, value, overwrite) = (args[0], args[1], args[2]);
    let invalid = || {
        set_errno(i64::from(orbistoun_core::errno::INVALID));
        FAILED
    };
    if name == 0 || value == 0 {
        return invalid();
    }
    // SAFETY: a guest-supplied string under the identity mapping, bounded.
    let name_len = unsafe { c_len(name) };
    // SAFETY: the other one, same contract.
    let value_len = unsafe { c_len(value) };
    // SAFETY: `c_len` established `name_len` readable bytes at `name`.
    let name_bytes = unsafe { std::slice::from_raw_parts(ptr(name).cast_const(), name_len) };
    // SAFETY: and `value_len` readable bytes at `value`.
    let value_bytes = unsafe { std::slice::from_raw_parts(ptr(value).cast_const(), value_len) };
    let (Ok(name), Ok(value)) = (
        std::str::from_utf8(name_bytes),
        std::str::from_utf8(value_bytes),
    ) else {
        return invalid();
    };
    if name.is_empty() || name.contains('=') {
        return invalid();
    }
    let Ok(mut guard) = ENVIRONMENT.lock() else {
        return invalid();
    };
    let answered = guard.get_or_insert_with(std::collections::BTreeMap::new);
    if overwrite == 0 {
        // Set by an earlier call or present in what the guest was handed: both count as existing,
        // and the standard says to leave it and report success.
        let prefix = format!("{name}=");
        if answered.contains_key(name)
            || orbistoun_thunk::guest_environment()
                .iter()
                .any(|entry| entry.starts_with(&prefix))
        {
            return OK;
        }
    }
    let stored: &'static [u8] = Box::leak(format!("{value}\0").into_bytes().into_boxed_slice());
    answered.insert(name.to_owned(), stored);
    OK
}

/// `unsetenv(name)` - POSIX.1-2008 `unsetenv(3)`.
///
/// Removes only what this layer holds. A variable from the guest's process image cannot be
/// taken out of it, so it is reported as removed and `getenv` still finds it.
fn unsetenv(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let name = args[0];
    if name == 0 {
        set_errno(i64::from(orbistoun_core::errno::INVALID));
        return FAILED;
    }
    // SAFETY: a guest-supplied string under the identity mapping, bounded.
    let len = unsafe { c_len(name) };
    // SAFETY: `c_len` established `len` readable bytes.
    let bytes = unsafe { std::slice::from_raw_parts(ptr(name).cast_const(), len) };
    let Ok(name) = std::str::from_utf8(bytes) else {
        set_errno(i64::from(orbistoun_core::errno::INVALID));
        return FAILED;
    };
    if name.is_empty() || name.contains('=') {
        set_errno(i64::from(orbistoun_core::errno::INVALID));
        return FAILED;
    }
    if let Ok(mut guard) = ENVIRONMENT.lock()
        && let Some(answered) = guard.as_mut()
    {
        answered.remove(name);
    }
    OK
}

/// `getcwd(buffer, size)` - the root.
///
/// There is no working directory: paths resolve against the absolute mount table.
///
/// Reference: POSIX.1-2008 `getcwd(3)`. Answers the buffer on success and null on failure; a
/// buffer too small is the documented failure rather than a truncation.
fn getcwd(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (buffer, size) = (args[0], args[1]);
    let text = b"/\0";
    if buffer == 0 || size < text.len() as u64 {
        return 0;
    }
    // SAFETY: a guest-supplied buffer under the identity mapping, with at least the two bytes
    // just checked against the size the guest passed.
    unsafe { std::ptr::copy_nonoverlapping(text.as_ptr(), ptr(buffer), text.len()) };
    buffer
}

/// `realpath(path, resolved)` - a path with the `.`, the `..` and the doubled slashes gone.
///
/// A file server calls it on every path a client names to decide whether the path is real.
/// The answer is a guest path, not the host path it maps to. Components are walked here: `.`
/// is dropped, `..` pops the one before it (and stays at the root), empty components collapse,
/// and a relative path is taken from the root, as [`getcwd`] reports. POSIX requires every
/// component to exist, so an unresolvable path answers null.
///
/// Reference: POSIX.1-2008 `realpath(3)`; `PATH_MAX` from `sys/sys/syslimits.h`.
fn realpath(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    /// What the call answers when it cannot resolve the path.
    const FAILED_POINTER: u64 = 0;

    let (path, resolved) = (args[0], args[1]);
    if path == 0 {
        return FAILED_POINTER;
    }
    // SAFETY: a guest-supplied string under the identity mapping, bounded.
    let len = unsafe { c_len(path) };
    // SAFETY: `c_len` established `len` readable bytes from `path`.
    let bytes = unsafe { std::slice::from_raw_parts(ptr(path).cast_const(), len) };
    let Ok(asked) = std::str::from_utf8(bytes) else {
        return FAILED_POINTER;
    };
    let Some(answer) = resolved_guest_path(asked) else {
        return FAILED_POINTER;
    };

    let ceiling = orbistoun_hle::constants::abi_constant("syslimits", "PATH_MAX")
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(1024);
    let mut text = answer.into_bytes();
    text.push(0);
    if text.len() > ceiling {
        // Longer than a caller's buffer can be. Refused rather than truncated: half a path is a
        // different path.
        return FAILED_POINTER;
    }

    // A null second argument means "allocate one", from this library's heap so the guest's
    // `free` releases it.
    let destination = if resolved == 0 {
        let block = allocate(text.len(), HEAP_HEADER);
        if block == 0 {
            return FAILED_POINTER;
        }
        block
    } else {
        resolved
    };
    let Ok(at) = usize::try_from(destination) else {
        return FAILED_POINTER;
    };
    // SAFETY: either a block just allocated at exactly this length, or a guest-supplied buffer
    // the interface requires to hold `PATH_MAX` bytes; the length was checked against that above.
    unsafe {
        std::ptr::copy_nonoverlapping(
            text.as_ptr(),
            std::ptr::with_exposed_provenance_mut::<u8>(at),
            text.len(),
        );
    }
    destination
}

/// The canonical spelling of a guest path.
///
/// Pure, so the walk is testable without a mount table: `..` at the root, a trailing slash, an
/// empty path, a doubled separator.
fn canonical_components(asked: &str) -> String {
    // Normalised first, so a guest mixing separators cannot slip a component past the walk.
    let normalised = asked.replace('\\', "/");
    let mut kept: Vec<&str> = Vec::new();
    for component in normalised.split('/') {
        match component {
            // An empty component is a doubled slash or a leading one; `.` is this directory.
            "" | "." => {}
            ".." => {
                // At the root this stays at the root, so a path cannot leave the mount table by
                // spelling.
                kept.pop();
            }
            other => kept.push(other),
        }
    }
    let mut out = String::from("/");
    out.push_str(&kept.join("/"));
    if out.len() > 1 && out.ends_with('/') {
        out.pop();
    }
    out
}

/// The canonical path, if every component of it is really there.
fn resolved_guest_path(asked: &str) -> Option<String> {
    let canonical = canonical_components(asked);
    if orbistoun_fs::mount::is_directory(&canonical) {
        return Some(canonical);
    }
    let host = orbistoun_fs::mount::resolve(&canonical)?;
    host.exists().then_some(canonical)
}

/// `perror(prefix)` - the guest's own error message, on the error stream.
///
/// Renders `errno` through the message [`strerror`] gives. A null or empty prefix prints the
/// message alone, as the standard says.
///
/// Reference: POSIX.1-2008 `perror(3)`.
fn perror(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    use std::io::Write as _;

    let mut line = Vec::new();
    if args[0] != 0 {
        // SAFETY: a guest-supplied string under the identity mapping, bounded.
        let len = unsafe { c_len(args[0]) };
        if len > 0 {
            // SAFETY: `c_len` established `len` readable bytes.
            let prefix = unsafe { std::slice::from_raw_parts(ptr(args[0]).cast_const(), len) };
            line.extend_from_slice(prefix);
            line.extend_from_slice(b": ");
        }
    }
    let number = current_errno();
    line.extend_from_slice(format!("error {number} (orbistoun has no message table)").as_bytes());
    line.push(b'\n');

    // Guest output, not a log: the guest's own write, so it stays a direct write.
    let mut err = std::io::stderr();
    let _ = err.write_all(&line);
    let _ = err.flush();
    0
}

/// `strerror_r(errnum, buffer, size)` - `strerror` into a caller's own buffer.
///
/// Answers zero on success; a buffer too small gets nothing rather than half a message.
///
/// Reference: POSIX.1-2008 `strerror_r(3)`.
fn strerror_r(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (number, buffer, size) = (args[0] as i32, args[1], args[2]);
    let text = format!("error {number} (orbistoun has no message table)\0");
    if buffer == 0 || size < text.len() as u64 {
        return FAILED;
    }
    // SAFETY: a guest-supplied buffer under the identity mapping, with at least `text.len()`
    // bytes as just checked against the size the guest passed.
    unsafe { std::ptr::copy_nonoverlapping(text.as_ptr(), ptr(buffer), text.len()) };
    OK
}

/// `sysctl(name, namelen, oldp, oldlenp, newp, newlen)` - answers what it knows and refuses
/// the rest with the documented failure.
///
/// FreeBSD's `sysctl(3)` documents `ENOENT` for an unknown name, and a caller takes its own
/// error path on it (D350). Success would be worse: `oldp` is often null on the first of a
/// pair of calls asking only for the size, so success without a length leaves the caller an
/// uninitialised size. Every distinct unknown MIB is reported once.
fn sysctl(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    /// The most name components to read, from `sysctl(3)`'s own `CTL_MAXNAME` bound.
    const MAX_NAME: usize = 24;

    let (name, namelen) = (args[0], args[1]);
    let Ok(count) = usize::try_from(namelen) else {
        return FAILED;
    };
    if name == 0 || count == 0 || count > MAX_NAME {
        return FAILED;
    }

    let mut mib: Vec<u32> = Vec::with_capacity(count);
    for index in 0..count {
        let at = name.saturating_add((index as u64).saturating_mul(4));
        let Ok(at) = usize::try_from(at) else {
            return FAILED;
        };
        // SAFETY: a guest-supplied array of `namelen` 32-bit words under the identity mapping; a
        // length the guest declared, read within it.
        let word = unsafe { std::ptr::read(std::ptr::with_exposed_provenance::<u32>(at)) };
        mib.push(word);
    }

    if is_process_listing(&mib) {
        return no_processes(args[3]);
    }

    let spelled: Vec<String> = mib.iter().map(u32::to_string).collect();
    note_unknown_sysctl(&spelled.join("."));
    // The documented answer for an unknown name, from `sysctl(3)`, with the number read from
    // the harvested table (D350).
    if let Some(enoent) = orbistoun_hle::constants::abi_constant("errno", "ENOENT") {
        set_errno(enoent);
    }
    FAILED
}

/// This thread's `errno`, as a number.
///
/// Read from the storage `__error()` hands the guest.
fn current_errno() -> i32 {
    let at = error_location(&[0; GUEST_ARG_REGISTERS]);
    let Ok(at) = usize::try_from(at) else {
        return 0;
    };
    // SAFETY: `error_location` answers the address of this thread's `errno`, which is a live
    // `i32` owned by this thread for as long as it runs.
    unsafe { std::ptr::read(std::ptr::with_exposed_provenance::<i32>(at)) }
}

/// Sets this thread's `errno`, the way a failing C library call must, in the storage
/// `__error()` points at.
fn set_errno(value: i64) {
    let at = error_location(&[0; GUEST_ARG_REGISTERS]);
    let Ok(at) = usize::try_from(at) else {
        return;
    };
    // SAFETY: `error_location` answers the address of this thread's `errno`, which is a
    // live `i32` owned by this thread for as long as it runs.
    unsafe {
        std::ptr::write(
            std::ptr::with_exposed_provenance_mut::<i32>(at),
            value as i32,
        );
    };
}

/// Turns a vendor-encoded failure into the POSIX one: `errno` set, `-1` returned.
///
/// The POSIX-named exports fail with `-1` and `errno`, not the `0x8002_0000 | errno` of their
/// vendor twins (measured for `open` and `close`, the records in `libScePosix.toml`). Anything
/// that is not a vendor error code is returned unchanged.
#[must_use]
pub fn posix_failure(answer: u64) -> u64 {
    /// The vendor error family: `0x8002_0000 | errno`.
    const VENDOR: u64 = 0x8002_0000;
    if answer & !0xFF == VENDOR {
        set_errno(i64::try_from(answer & 0xFF).unwrap_or(0));
        FAILED
    } else {
        answer
    }
}

/// What a failing call answers, as the guest reads it: `-1` in a 32-bit register. Written
/// out because `From` is not const, and a sign-extended `u64::MAX` differs in `eax`.
const FAILED: u64 = 0xFFFF_FFFF;

/// Whether a MIB is asking for the list of running processes.
///
/// `kern.proc.proc` (`CTL_KERN`, `KERN_PROC`, `KERN_PROC_PROC`), every component read from the
/// harvested table.
fn is_process_listing(mib: &[u32]) -> bool {
    let component = |name: &str| {
        orbistoun_hle::constants::abi_constant("sysctl", name)
            .and_then(|value| u32::try_from(value).ok())
    };
    let (Some(kern), Some(proc), Some(all)) = (
        component("CTL_KERN"),
        component("KERN_PROC"),
        component("KERN_PROC_PROC"),
    ) else {
        return false;
    };
    mib.len() >= 3 && mib[0] == kern && mib[1] == proc && mib[2] == all
}

/// Answers a process listing with the truth: there are none.
///
/// A payload enumerating processes is looking for an earlier copy of itself, and no such
/// process is running, so the call succeeds with a zero-length result. This also avoids
/// `struct kinfo_proc`, whose layout differs between the harvested FreeBSD release and the
/// one the target derives from (D374).
///
/// Reference: FreeBSD `sysctl(3)`, `kern.proc`. A caller asking only for the size passes a
/// null buffer and gets the same answer.
fn no_processes(length_at: u64) -> u64 {
    let Ok(at) = usize::try_from(length_at) else {
        return FAILED;
    };
    if at == 0 {
        // No length to report into; the call still succeeded.
        return OK;
    }
    // SAFETY: a guest-supplied `size_t *` under the identity mapping, which the guest passed
    // expecting to be written through.
    unsafe { std::ptr::write_unaligned(std::ptr::with_exposed_provenance_mut::<u64>(at), 0) };
    OK
}

/// `sysctlbyname(name, oldp, oldlenp, newp, newlen)` - the same question, asked by name.
///
/// A guest asking `kern.osrelease` branches on the kernel it is told it is on. What is known is
/// answered; what is not is refused and reported once. This is the only `sysctlbyname`. An
/// unset `kern.osrelease` answers an empty NUL-terminated string, since the knob exists on the
/// hardware (D447). Failures answer vendor codes, because a missing name and a buffer too
/// small are different things to a caller, and only the second is worth retrying.
///
/// Reference: FreeBSD `sysctlbyname(3)`. A caller passing a null buffer is asking for the
/// size, and gets the same answer.
fn sysctl_by_name(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (name, out, out_len) = (args[0], args[1], args[2]);
    if name == 0 {
        return sysctl_failed(orbistoun_core::errno::FAULT);
    }
    // SAFETY: a guest-supplied string under the identity mapping, bounded.
    let len = unsafe { c_len(name) };
    // SAFETY: `c_len` established `len` readable bytes from `name`.
    let bytes = unsafe { std::slice::from_raw_parts(ptr(name).cast_const(), len) };
    let Ok(asked) = std::str::from_utf8(bytes) else {
        // A MIB name is ASCII; bytes that are not are not a name this can look up.
        return sysctl_failed(orbistoun_core::errno::INVALID);
    };

    // An integer knob is answered as raw bytes of the platform's width, since a caller reads it
    // as an `int` or a `long`. Tried first, so a name cannot be answered two ways.
    if let Some((value, width)) = answer_integer(asked) {
        return answer_bytes(&value.to_le_bytes()[..width], out, out_len);
    }
    let Some(text) = answer_for(asked) else {
        // Reported once, so the names a guest wanted form a list.
        note_unknown_sysctl(asked);
        return sysctl_failed(orbistoun_core::errno::NO_ENTRY);
    };
    answer_string(&text, out, out_len)
}

/// An integer knob and the byte width the platform answers it in, or nothing, for the machine
/// this run presents.
fn answer_integer(name: &str) -> Option<(u64, usize)> {
    integer_knob(orbistoun_core::machine::presented(), name)
}

/// An integer knob and the byte width the platform answers it in, for a given machine.
///
/// Each value was measured on hardware (obSCEne's `135-sysctl` checks). The width matters as
/// much as the value: `hw.ncpu` is a four-byte `int` and `tsc_freq` an eight-byte `long`.
/// `kern.sdk_version` is per machine, so it comes from the profile and a machine without one
/// refuses it (D675). The machine is an argument so that case is testable without the
/// process-wide slot.
fn integer_knob(machine: &orbistoun_core::machine::Machine, name: &str) -> Option<(u64, usize)> {
    match name {
        // `BSD` from `sys/sys/param.h`, `199506`, which is what `KERN_OSREV` returns on FreeBSD;
        // hardware answers the same (`135-sysctl/names`).
        "kern.osrevision" => Some((199_506, 4)),
        // One machine's value, from its profile. Zero is unset, and unset refuses.
        "kern.sdk_version" => {
            (machine.kernel_sdk_version != 0).then_some((u64::from(machine.kernel_sdk_version), 4))
        }
        // Sixteen hardware threads, read back as a four-byte int.
        "hw.ncpu" => Some((16, 4)),
        // Sixteen-kibibyte pages, `0x4000`, as the direct-memory layer and the loader assume.
        "hw.pagesize" => Some((0x4000, 4)),
        // The counter frequency, the same value the time stamp counter and the process-time counter
        // report.
        "machdep.tsc_freq" => Some((0x5f25_9b8e, 8)),
        _ => None,
    }
}

/// Writes raw bytes and their length the way `sysctl` reports an integer answer.
///
/// The size/value contract of [`answer_string`], without the terminator a string carries.
fn answer_bytes(value: &[u8], out: u64, out_len: u64) -> u64 {
    let needed = value.len();
    if out_len != 0 {
        let Ok(at) = usize::try_from(out_len) else {
            return FAILED;
        };
        // SAFETY: a guest-supplied `size_t *` under the identity mapping, which the guest passed
        // expecting to be written through.
        let room =
            unsafe { std::ptr::read_unaligned(std::ptr::with_exposed_provenance::<u64>(at)) };
        // SAFETY: the same pointer, written back with the width this answer needs.
        unsafe {
            std::ptr::write_unaligned(
                std::ptr::with_exposed_provenance_mut::<u64>(at),
                needed as u64,
            );
        }
        if out != 0 && room < needed as u64 {
            return sysctl_failed(orbistoun_core::errno::NO_MEMORY);
        }
    }
    if out == 0 {
        return OK;
    }
    let Ok(at) = usize::try_from(out) else {
        return FAILED;
    };
    // SAFETY: a guest-supplied buffer under the identity mapping, whose room was checked against
    // the declared length above when one was given.
    unsafe {
        std::ptr::copy_nonoverlapping(
            value.as_ptr(),
            std::ptr::with_exposed_provenance_mut::<u8>(at),
            value.len(),
        );
    }
    OK
}

/// The code a `sysctl` failure answers, from the errno underneath it.
///
/// The vendor encoding `0x8002_0000 | errno` rather than [`FAILED`]: a missing name, a buffer
/// too small and an unusable pointer are distinctions a caller branches on (D398).
fn sysctl_failed(errno: u32) -> u64 {
    u64::from(orbistoun_core::GuestError::vendor(errno).as_raw())
}

/// What this run's machine says about one named MIB, or nothing.
fn answer_for(name: &str) -> Option<String> {
    text_knob(orbistoun_core::machine::presented(), name)
}

/// What a given machine says about one named MIB, or nothing.
///
/// A short list: a name answered from a guess is worse than one refused. An unset
/// `kern.osrelease` answers an empty string (D447); an unset `kern.version` or `hw.model`
/// refuses, so a machine that knows neither does not report them as existing and empty
/// (D675).
fn text_knob(machine: &orbistoun_core::machine::Machine, name: &str) -> Option<String> {
    match name {
        // Answered even when empty: the knob exists on the hardware, so an empty NUL-terminated
        // string ("exists, no value") is true where a refusal ("no such name") is not (D447). A
        // machine profile may carry a release; the default does not invent one.
        "kern.osrelease" => Some(machine.kernel_release.clone()),
        // `MACHINE` from `sys/amd64/include/param.h`, what `HW_MACHINE` returns on an amd64 FreeBSD
        // kernel; hardware answers the same (`135-sysctl/names`).
        "hw.machine" => Some("amd64".to_owned()),
        // One machine's values, carried by its profile. Empty is unset, and unset refuses.
        "kern.version" => {
            (!machine.kernel_version.is_empty()).then(|| machine.kernel_version.clone())
        }
        "hw.model" => (!machine.hardware_model.is_empty()).then(|| machine.hardware_model.clone()),
        // Measured on hardware, and the same on every machine: the target kernel is
        // FreeBSD-derived.
        "kern.ostype" => Some("FreeBSD".to_owned()),
        // Empty: the hardware answers a single NUL for this knob, so the name exists with no value
        // (D447). A hostname is per machine, and orbistoun has none to report.
        "kern.hostname" => Some(String::new()),
        _ => None,
    }
}

/// Writes a string answer and its length the way `sysctl` reports one.
///
/// A null buffer with a length pointer is the size half of the pair, and gets the length
/// without the bytes.
fn answer_string(text: &str, out: u64, out_len: u64) -> u64 {
    let needed = text.len() + 1;
    if out_len != 0 {
        let Ok(at) = usize::try_from(out_len) else {
            return sysctl_failed(orbistoun_core::errno::FAULT);
        };
        // SAFETY: a guest-supplied `size_t *` under the identity mapping, which the guest passed
        // expecting to be written through.
        let room =
            unsafe { std::ptr::read_unaligned(std::ptr::with_exposed_provenance::<u64>(at)) };
        // SAFETY: the same pointer, written back with what the answer actually needs.
        unsafe {
            std::ptr::write_unaligned(
                std::ptr::with_exposed_provenance_mut::<u64>(at),
                needed as u64,
            );
        }
        if out != 0 && room < needed as u64 {
            // Too small; the length is already written back so the caller can retry, as the
            // interface documents.
            return sysctl_failed(orbistoun_core::errno::NO_MEMORY);
        }
    }
    if out == 0 {
        // The size half of the pair. Answered, with nothing written.
        return OK;
    }
    let Ok(at) = usize::try_from(out) else {
        return sysctl_failed(orbistoun_core::errno::FAULT);
    };
    let mut bytes = text.as_bytes().to_vec();
    bytes.push(0);
    // SAFETY: a guest-supplied buffer under the identity mapping, whose room was checked against
    // the declared length above when one was given.
    unsafe {
        std::ptr::copy_nonoverlapping(
            bytes.as_ptr(),
            std::ptr::with_exposed_provenance_mut::<u8>(at),
            bytes.len(),
        );
    }
    OK
}

/// Records a MIB nothing here implements, once per distinct name, so a guest in a retry loop
/// does not bury the rest of the report.
fn note_unknown_sysctl(mib: &str) {
    static SEEN: std::sync::Mutex<Option<std::collections::BTreeSet<String>>> =
        std::sync::Mutex::new(None);
    let Ok(mut seen) = SEEN.lock() else {
        return;
    };
    if seen
        .get_or_insert_with(Default::default)
        .insert(mib.to_owned())
    {
        let line = format!(
            concat!(
                "orbistoun: sysctl asked for [{}] and nothing here knows it - refused with ",
                "the documented failure"
            ),
            mib
        );
        tracing::warn!(
            "sysctl asked for [{mib}] and nothing here knows it - refused with the documented failure"
        );
        // And to the kernel log, while the guest is still running to read it (D396).
        orbistoun_core::klog::note(&line);
    }
}

/// `abort()`.
///
/// Never returns: a compiler emits an unreachable trap after a `noreturn` call, and returning
/// into it reports an illegal instruction instead of the guest giving up (D177).
fn abort(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    orbistoun_core::stop(orbistoun_core::StopReason::Aborted, args[0])
}

/// `_Assert(message)` - the runtime's assertion handler.
///
/// The platform's C runtime routes a failed `assert` here. ISO/IEC 9899 7.2.1.1 requires the
/// failure to be written to the standard error stream before `abort`, so the message is
/// written first.
fn assert_failed(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let message = if args[0] == 0 {
        "(no message)".to_owned()
    } else {
        // SAFETY: a guest-supplied string under the identity mapping, bounded.
        let len = unsafe { c_len(args[0]) };
        // SAFETY: `c_len` established `len` readable bytes.
        let bytes = unsafe { std::slice::from_raw_parts(ptr(args[0]).cast_const(), len) };
        String::from_utf8_lossy(bytes).into_owned()
    };
    tracing::warn!("the guest failed an assertion: {message}");
    orbistoun_core::klog::note(&format!("assertion failed: {message}"));
    orbistoun_core::stop(orbistoun_core::StopReason::Aborted, 0)
}

/// `exit(status)`, and `_Exit(status)`.
///
/// Never returns. A guest ending deliberately is a different outcome from one that faulted
/// (D177).
fn exit(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    orbistoun_core::stop(orbistoun_core::StopReason::Exited, args[0])
}

/// `operator new(size)`, and its array form.
///
/// The same heap `malloc` uses, so a program mixing the two sees one heap. A real
/// `operator new` throws on failure; this answers null, since there is no exception runtime.
fn operator_new(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    malloc(args)
}

/// `operator delete(pointer)`, and its array and sized forms.
///
/// A sized delete's size is ignored: the heap records each allocation's length in its header.
fn operator_delete(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    free(args)
}

/// `fopen(path, mode)`.
///
/// Guests dereference the result without checking it, so only a real handle is safe. Read-only:
/// guest writes land elsewhere (D250).
fn fopen(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // SAFETY: the guest's path argument, a NUL-terminated string by the call's contract.
    let Some(path) = (unsafe { orbistoun_mem::guest::read_path(args[0]) }) else {
        return 0;
    };
    // Null rather than an error code: the caller reads this as a pointer (D125), and null
    // faults nearest the cause.
    orbistoun_fs::open::open(&path).unwrap_or(0)
}

/// `fclose(stream)`.
fn fclose(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // Zero is success; a handle naming nothing answers `EOF`.
    if orbistoun_fs::open::close(args[0]) {
        OK
    } else {
        EOF
    }
}

/// `fread(dest, size, count, stream)`.
///
/// Answers the number of elements read, not bytes.
fn fread(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (dest, size, count, stream) = (args[0], args[1], args[2], args[3]);
    if dest == 0 || size == 0 || count == 0 {
        return 0;
    }
    let Some(total) = size
        .checked_mul(count)
        .and_then(|n| usize::try_from(n).ok())
    else {
        // A request that cannot be expressed is refused rather than truncated.
        return 0;
    };
    let Ok(at) = usize::try_from(dest) else {
        return 0;
    };

    // SAFETY: the guest supplied this destination and declared its size, as the real call's
    // contract states; an unmapped address faults here as it would in the guest, and the fault
    // reporter names it.
    let into = unsafe {
        std::slice::from_raw_parts_mut(std::ptr::with_exposed_provenance_mut::<u8>(at), total)
    };
    let read = orbistoun_fs::open::read(stream, into).unwrap_or(0);
    // Whole elements only, which is what the interface promises.
    (read as u64) / size
}

/// `fseek(stream, offset, whence)`.
fn fseek(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (stream, offset, whence) = (args[0], args[1] as i64, args[2]);
    let Some(from) = orbistoun_fs::open::From::from_whence(whence) else {
        return EOF;
    };
    match orbistoun_fs::open::seek(stream, from, offset) {
        Some(_) => OK,
        None => EOF,
    }
}

/// `ftell(stream)`.
///
/// Guests size buffers from this, so a wrong value becomes a wrong memory request.
fn ftell(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    orbistoun_fs::open::tell(args[0]).unwrap_or(EOF)
}

/// `rewind(stream)`.
fn rewind(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    orbistoun_fs::open::seek(args[0], orbistoun_fs::open::From::Start, 0);
    OK
}

/// `feof(stream)`.
///
/// Non-zero once a read has hit the end. A handle naming nothing answers zero.
fn feof(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    u64::from(orbistoun_fs::open::at_end(args[0]).unwrap_or(false))
}

/// `ferror(stream)`.
///
/// Always zero: reads either succeed or report short, and nothing here distinguishes a device
/// error from the end of a file.
fn ferror(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

/// `fflush(stream)`.
///
/// Nothing to flush: everything opened here is read-only.
fn fflush(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

/// `fgets(s, size, stream)` - one line, or up to `size - 1` bytes, whichever comes first.
///
/// Reads one byte at a time, keeping the newline that ends a line and stopping on it. The
/// result is NUL-terminated, and a call that reads nothing at end of file answers NULL, which
/// is what a `while (fgets(...))` loop tests.
///
/// Reference: ISO C 7.21.7.2 (`fgets`).
fn fgets(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (s, size, stream) = (args[0], args[1], args[2]);
    let (Ok(cap), Ok(at)) = (usize::try_from(size), usize::try_from(s)) else {
        return 0;
    };
    if s == 0 || cap == 0 {
        return 0;
    }
    let dest = std::ptr::with_exposed_provenance_mut::<u8>(at);
    let mut written = 0;
    let mut byte = [0_u8; 1];
    // One short of `size`, so the terminator the standard requires always fits.
    while written + 1 < cap {
        match orbistoun_fs::open::read(stream, &mut byte) {
            Some(1) => {
                // SAFETY: `written + 1 < cap <= size`, so this offset is inside the buffer the
                // guest declared, under the identity mapping.
                let slot = unsafe { dest.add(written) };
                // SAFETY: one byte written into that in-bounds slot.
                unsafe { *slot = byte[0] };
                written += 1;
                if byte[0] == b'\n' {
                    // The newline stays in the buffer, which distinguishes a whole line from a
                    // truncated one.
                    break;
                }
            }
            // End of file (zero bytes) or a handle naming nothing: stop.
            _ => break,
        }
    }
    if written == 0 {
        // Nothing read - end of file with an empty buffer, which the standard answers NULL for.
        return 0;
    }
    // SAFETY: `written < cap`, so the terminator is in bounds.
    let end = unsafe { dest.add(written) };
    // SAFETY: one byte written at that in-bounds position.
    unsafe { *end = 0 };
    s
}

/// The pseudo-random state behind `rand`/`srand`, seeded to 1 as C requires when `srand` is
/// never called. One process-wide value: C's `rand` is not thread-safe, and `rand_r` exists
/// for per-thread streams.
static RAND_STATE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// `rand()` - a value in `[0, 0x7fffffff]`, the platform's `RAND_MAX`.
///
/// A linear congruential generator, as the standard's own example uses, returning the high
/// bits because an LCG's low bits cycle short. `035-libc/rand-seeded` checks that two draws
/// differ and a re-seed reproduces them, not the platform's exact sequence.
fn rand(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    use std::sync::atomic::Ordering::Relaxed;
    let mut prev = RAND_STATE.load(Relaxed);
    loop {
        // The multiplier and increment are the widely used PCG/Knuth 64-bit LCG constants.
        let next = prev
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        match RAND_STATE.compare_exchange_weak(prev, next, Relaxed, Relaxed) {
            Ok(_) => return (next >> 33) & 0x7fff_ffff,
            Err(actual) => prev = actual,
        }
    }
}

/// `srand(seed)` - reset the sequence so a given seed reproduces a given series of draws.
fn srand(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    RAND_STATE.store(args[0], std::sync::atomic::Ordering::Relaxed);
    OK
}

/// Implementations this crate provides, by symbol name.
///
/// Names rather than hashes, so the table can be read and checked against the declarations.
pub fn implementations() -> Vec<(&'static str, GuestFn)> {
    let mut all = core_implementations().to_vec();
    all.extend_from_slice(cstring::implementations());
    all.extend_from_slice(clock::implementations());
    all.extend_from_slice(ctype::implementations());
    all.extend_from_slice(atomic::implementations());
    all.extend_from_slice(locks::implementations());
    all.extend_from_slice(cxx::implementations());
    all.extend_from_slice(scan::implementations());
    // Implemented next to the mount model, and declared here because this is the library that
    // exports them (D367).
    all.extend_from_slice(orbistoun_fs::posix::implementations());
    // Only the underscored spelling and the interface list: `inet_ntop` is declared in
    // `libScePosix` and served from there.
    all.extend(
        orbistoun_fs::ifaddrs::implementations()
            .iter()
            .filter(|(name, _)| *name != "inet_ntop")
            .copied(),
    );
    all.extend_from_slice(orbistoun_fs::kqueue::implementations());
    all.extend_from_slice(orbistoun_fs::fcntl::implementations());
    // Everything but `fstat`, which is declared in `libScePosix` and served from there.
    all.extend(
        orbistoun_fs::metadata::implementations()
            .iter()
            .filter(|(name, _)| *name != "fstat")
            .copied(),
    );
    all
}

/// The functions declared directly in this file.
fn core_implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("abort", abort),
        ("_Assert", assert_failed),
        ("exit", exit),
        ("_Exit", exit),
        // The raw syscall's own spelling: FreeBSD's entry 1 is `_exit`, so the harvested constant
        // is `SYS__exit` and the name derived from it is `_exit`.
        ("_exit", exit),
        ("_Znwm", operator_new),
        ("_Znam", operator_new),
        ("_ZdlPv", operator_delete),
        ("_ZdaPv", operator_delete),
        ("_ZdlPvm", operator_delete),
        ("_ZdaPvm", operator_delete),
        ("_ZSt14_Random_devicev", random_device),
        ("fopen", fopen),
        ("fclose", fclose),
        ("fread", fread),
        ("fseek", fseek),
        ("ftell", ftell),
        ("rewind", rewind),
        ("fgets", fgets),
        ("feof", feof),
        ("ferror", ferror),
        ("fflush", fflush),
        ("memset", memset),
        ("memcpy", memcpy),
        ("memmove", memmove),
        ("memcmp", memcmp),
        // `bcmp` is `memcmp`: equal iff zero, which is all `bcmp` promises.
        ("bcmp", memcmp),
        ("memchr", memchr),
        ("memalign", memalign),
        ("printf", printf),
        ("vsnprintf", vsnprintf),
        ("vsprintf_s", vsprintf_s),
        ("vprintf", vprintf),
        ("vfprintf", vfprintf),
        ("snprintf_s", snprintf_s),
        ("snprintf", snprintf_s),
        ("sprintf", sprintf),
        ("strdup", strdup),
        ("strndup", strndup),
        ("strncat", strncat),
        ("qsort", qsort),
        ("bsearch", bsearch),
        ("setlocale", setlocale),
        ("clock", clock),
        ("setjmp", setjmp),
        ("rand", rand),
        ("srand", srand),
        ("strtok", strtok),
        ("strtok_r", strtok_r),
        ("strlen", strlen),
        ("strnlen", strnlen),
        ("strcmp", strcmp),
        ("strncmp", strncmp),
        ("strcpy", strcpy),
        ("strncpy", strncpy),
        ("strcpy_s", strcpy_s),
        ("strcat", strcat),
        ("strchr", strchr),
        ("strrchr", strrchr),
        ("atexit", atexit),
        ("signal", signal),
        ("getopt", getopt),
        ("__error", error_location),
        ("strerror", strerror),
        ("puts", puts),
        ("putchar", putchar),
        ("getpid", getpid),
        ("sysctl", sysctl),
        ("kill", kill),
        ("getenv", getenv),
        ("setenv", setenv),
        ("unsetenv", unsetenv),
        ("getcwd", getcwd),
        ("sysctlbyname", sysctl_by_name),
        ("realpath", realpath),
        ("perror", perror),
        ("strerror_r", strerror_r),
        ("fprintf", fprintf),
        ("malloc", malloc),
        ("calloc", calloc),
        ("realloc", realloc),
        ("free", free),
        ("sceLibcMspaceMalloc", mspace_malloc),
        ("sceLibcMspaceCalloc", mspace_calloc),
        ("sceLibcMspaceRealloc", mspace_realloc),
        ("sceLibcMspaceFree", mspace_free),
        ("__cxa_atexit", cxa_atexit),
        ("__cxa_guard_acquire", cxa_guard_acquire),
        ("__cxa_guard_release", cxa_guard_release),
        ("__cxa_guard_abort", cxa_guard_abort),
    ]
}

#[cfg(test)]
mod abi_constant_tests {
    /// Every constant this crate asks for by name is in the table, since a missing one answers
    /// `None` and silently stops setting `errno` (D352).
    #[test]
    fn every_name_this_crate_looks_up_is_present() {
        /// Every constant this crate looks up by name; adding a lookup means adding a line here.
        const LOOKED_UP: &[(&str, &str)] = &[
            ("errno", "ENOENT"),
            ("clock", "CLOCK_REALTIME"),
            ("clock", "CLOCK_REALTIME_PRECISE"),
            ("clock", "CLOCK_REALTIME_FAST"),
            ("clock", "CLOCK_MONOTONIC"),
            ("clock", "CLOCK_MONOTONIC_PRECISE"),
            ("clock", "CLOCK_MONOTONIC_FAST"),
            ("sysctl", "CTL_KERN"),
            ("sysctl", "KERN_PROC"),
            ("sysctl", "KERN_PROC_PROC"),
            ("unistd", "W_OK"),
            ("socket", "AF_INET"),
            ("socket", "SOCK_STREAM"),
            ("if", "IFF_UP"),
            ("if", "IFF_LOOPBACK"),
            ("stat", "S_IFDIR"),
            ("stat", "S_IFREG"),
            ("dirent", "DT_DIR"),
            ("dirent", "DT_REG"),
        ];

        for (section, name) in LOOKED_UP {
            assert!(
                orbistoun_hle::constants::abi_constant(section, name).is_some(),
                "{section}.{name} is looked up in this crate and is not in the harvested table"
            );
        }
    }

    /// A constant `orbistoun-fs` writes by hand, checked against `sys/sys/unistd.h` here, where
    /// the harvested table is (D370).
    #[test]
    fn the_access_mode_another_crate_hardcodes_is_the_headers() {
        assert_eq!(
            Some(orbistoun_fs::posix::W_OK as i64),
            orbistoun_hle::constants::abi_constant("unistd", "W_OK"),
        );
    }

    /// The interface flags, written out in `orbistoun-fs` and checked here (D370).
    ///
    /// `IFF_LOOPBACK` matters most: a server walks the interface list for an address that is not
    /// the loopback.
    #[test]
    fn the_interface_flags_another_crate_hardcodes_are_the_headers() {
        assert_eq!(
            Some(i64::from(orbistoun_fs::ifaddrs::IFF_UP)),
            orbistoun_hle::constants::abi_constant("if", "IFF_UP"),
        );
        assert_eq!(
            Some(i64::from(orbistoun_fs::ifaddrs::IFF_LOOPBACK)),
            orbistoun_hle::constants::abi_constant("if", "IFF_LOOPBACK"),
        );
    }

    /// The file-type constants, checked the same way (D370): the `stat` bit and the `dirent` byte
    /// decide whether a name is a folder or a file.
    #[test]
    fn the_file_type_constants_another_crate_hardcodes_are_the_headers() {
        assert_eq!(
            Some(i64::from(orbistoun_fs::metadata::S_IFDIR)),
            orbistoun_hle::constants::abi_constant("stat", "S_IFDIR"),
        );
        assert_eq!(
            Some(i64::from(orbistoun_fs::metadata::S_IFREG)),
            orbistoun_hle::constants::abi_constant("stat", "S_IFREG"),
        );
        assert_eq!(
            Some(i64::from(orbistoun_fs::metadata::DT_DIR)),
            orbistoun_hle::constants::abi_constant("dirent", "DT_DIR"),
        );
        assert_eq!(
            Some(i64::from(orbistoun_fs::metadata::DT_REG)),
            orbistoun_hle::constants::abi_constant("dirent", "DT_REG"),
        );
    }

    /// A name that was never harvested answers nothing, rather than zero.
    #[test]
    fn an_unharvested_name_is_absent_rather_than_a_default() {
        assert_eq!(
            orbistoun_hle::constants::abi_constant("errno", "ENOSUCHTHING"),
            None
        );
        assert_eq!(
            orbistoun_hle::constants::abi_constant("nosuchsection", "ENOENT"),
            None
        );
    }

    /// The table holds the header's numbers: `SOL_SOCKET` is `0xffff` here and `1` on several
    /// other platforms.
    #[test]
    fn the_table_carries_the_platform_values_and_not_the_familiar_ones() {
        assert_eq!(
            orbistoun_hle::constants::abi_constant("errno", "ENOENT"),
            Some(2)
        );
        assert_eq!(
            orbistoun_hle::constants::abi_constant("signal", "SIGPIPE"),
            Some(13)
        );
        assert_eq!(
            orbistoun_hle::constants::abi_constant("socket", "SOL_SOCKET"),
            Some(0xffff),
            "0xffff, not the 1 that several other platforms use"
        );
        assert_eq!(
            orbistoun_hle::constants::abi_constant("sysctl", "KERN_PROC_PROC"),
            Some(8),
            "the MIG component klogsrv asks for, confirming the measurement in D350"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{FormatFault, MODULE, implementations, render_format, snprintf_s};
    use orbistoun_core::GUEST_ARG_REGISTERS;

    /// Calls one implementation by name, with host buffers standing in for guest memory, which
    /// the identity mapping makes exact.
    fn call(name: &str, args: [u64; GUEST_ARG_REGISTERS]) -> u64 {
        // Bound before searching: `implementations` builds its list, so borrowing through the call
        // would drop it while the match is held.
        let all = implementations();
        let (_, f) = all
            .iter()
            .find(|(n, _)| *n == name)
            .unwrap_or_else(|| panic!("{name} is not implemented"));
        f(&args)
    }

    fn address<T>(slice: &mut [T]) -> u64 {
        slice.as_mut_ptr() as usize as u64
    }

    #[test]
    fn memset_actually_writes_the_bytes() {
        // A guest writes through memory it was told was cleared.
        let mut buffer = [0xFF_u8; 16];
        let at = address(&mut buffer);
        assert_eq!(call("memset", [at, 0x41, 8, 0, 0, 0]), at, "returns dest");
        assert_eq!(&buffer[..8], &[0x41; 8]);
        assert_eq!(&buffer[8..], &[0xFF; 8], "and stops where it was told");
    }

    #[test]
    fn strlen_returns_a_length_and_not_an_error_code() {
        // A wrong length makes a guest walk off the end of every buffer it owns.
        let mut s = *b"hello\0padding";
        assert_eq!(call("strlen", [address(&mut s), 0, 0, 0, 0, 0]), 5);
        let mut empty = *b"\0";
        assert_eq!(call("strlen", [address(&mut empty), 0, 0, 0, 0, 0]), 0);
    }

    #[test]
    fn memcpy_copies_and_returns_its_destination() {
        let mut src = *b"abcdef";
        let mut dest = [0_u8; 6];
        let d = address(&mut dest);
        assert_eq!(call("memcpy", [d, address(&mut src), 6, 0, 0, 0]), d);
        assert_eq!(&dest, b"abcdef");
    }

    #[test]
    fn memcpy_handles_overlap_rather_than_corrupting_it() {
        // The real function may assume no overlap; overlapping here still gets a correct move.
        let mut buffer = *b"abcdef\0\0";
        let at = address(&mut buffer);
        call("memcpy", [at + 2, at, 6, 0, 0, 0]);
        assert_eq!(&buffer[2..8], b"abcdef");
    }

    #[test]
    fn memcmp_reports_ordering_by_sign() {
        let (mut a, mut b) = (*b"abc", *b"abd");
        let (pa, pb) = (address(&mut a), address(&mut b));
        assert_eq!(call("memcmp", [pa, pa, 3, 0, 0, 0]), 0, "equal is zero");
        assert!((call("memcmp", [pa, pb, 3, 0, 0, 0]) as i64) < 0);
        assert!((call("memcmp", [pb, pa, 3, 0, 0, 0]) as i64) > 0);
    }

    #[test]
    fn strcmp_stops_at_the_terminator_rather_than_at_a_fixed_length() {
        let (mut a, mut b) = (*b"ab\0XXXX", *b"ab\0YYYY");
        assert_eq!(
            call("strcmp", [address(&mut a), address(&mut b), 0, 0, 0, 0]),
            0,
            "what follows the terminator is not part of the string"
        );
    }

    #[test]
    fn strcpy_copies_the_terminator_too() {
        // Without the terminator the result is not a string.
        let mut src = *b"hi\0";
        let mut dest = [0xFF_u8; 8];
        call(
            "strcpy",
            [address(&mut dest), address(&mut src), 0, 0, 0, 0],
        );
        assert_eq!(&dest[..3], b"hi\0");
    }

    #[test]
    fn strncpy_pads_the_remainder_with_nul() {
        // The standard requires the padding and callers rely on it.
        let mut src = *b"ab\0";
        let mut dest = [0xFF_u8; 6];
        call(
            "strncpy",
            [address(&mut dest), address(&mut src), 6, 0, 0, 0],
        );
        assert_eq!(&dest, b"ab\0\0\0\0");
    }

    #[test]
    fn strchr_can_find_the_terminator_itself() {
        // `strchr(s, 0)` returns the end of the string, not null.
        let mut s = *b"abc\0";
        let at = address(&mut s);
        assert_eq!(call("strchr", [at, u64::from(b'b'), 0, 0, 0, 0]), at + 1);
        assert_eq!(call("strchr", [at, 0, 0, 0, 0, 0]), at + 3);
        assert_eq!(call("strchr", [at, u64::from(b'z'), 0, 0, 0, 0]), 0);
    }

    #[test]
    fn strrchr_finds_the_last_occurrence() {
        let mut s = *b"a/b/c\0";
        let at = address(&mut s);
        assert_eq!(call("strrchr", [at, u64::from(b'/'), 0, 0, 0, 0]), at + 3);
    }

    #[test]
    fn a_null_pointer_is_survived_rather_than_dereferenced() {
        // A null string answers zero rather than faulting inside the C library.
        assert_eq!(call("strlen", [0, 0, 0, 0, 0, 0]), 0);
        assert_eq!(call("memset", [0, 0x41, 16, 0, 0, 0]), 0);
        assert_eq!(call("strchr", [0, u64::from(b'a'), 0, 0, 0, 0]), 0);
    }

    #[test]
    fn a_guard_reports_uninitialised_once_and_only_once() {
        // A non-zero acquire means "go ahead and initialise", so without a real release every
        // static would reconstruct on every visit.
        let mut guard = [0_u64; 1];
        let at = address(&mut guard);

        assert_ne!(
            call("__cxa_guard_acquire", [at, 0, 0, 0, 0, 0]),
            0,
            "first visit initialises"
        );
        call("__cxa_guard_release", [at, 0, 0, 0, 0, 0]);
        assert_eq!(
            call("__cxa_guard_acquire", [at, 0, 0, 0, 0, 0]),
            0,
            "second visit must skip - otherwise the constructor runs twice, forever"
        );
    }

    #[test]
    fn an_aborted_initialisation_leaves_the_static_uninitialised() {
        // A constructor that throws has not initialised the object, and the standard requires the
        // next attempt to try again.
        let mut guard = [0_u64; 1];
        let at = address(&mut guard);
        assert_ne!(call("__cxa_guard_acquire", [at, 0, 0, 0, 0, 0]), 0);
        call("__cxa_guard_abort", [at, 0, 0, 0, 0, 0]);
        assert_ne!(
            call("__cxa_guard_acquire", [at, 0, 0, 0, 0, 0]),
            0,
            "an aborted construction must be retried, not skipped"
        );
    }

    #[test]
    fn cxa_atexit_accepts_the_registration() {
        // Non-zero means registration failed, and a C++ runtime told that aborts.
        assert_eq!(call("__cxa_atexit", [0x1000, 0x2000, 0x3000, 0, 0, 0]), 0);
    }

    #[test]
    fn an_allocation_is_writable_for_its_whole_length() {
        // A real, writable block (D128).
        let at = call("malloc", [4096, 0, 0, 0, 0, 0]);
        assert_ne!(at, 0, "a 4 KiB request should succeed");
        assert_eq!(call("memset", [at, 0xAB, 4096, 0, 0, 0]), at);

        // SAFETY: `malloc` just returned 4096 writable bytes here.
        let bytes = unsafe { std::slice::from_raw_parts(super::ptr(at), 4096) };
        assert!(bytes.iter().all(|b| *b == 0xAB), "every byte must be ours");
        call("free", [at, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn an_allocation_is_aligned_for_any_type() {
        // Sixteen-byte alignment on x86-64, or a vector type stored in the block faults.
        for size in [1_u64, 7, 8, 100, 4096] {
            let at = call("malloc", [size, 0, 0, 0, 0, 0]);
            assert_ne!(at, 0);
            assert_eq!(at % 16, 0, "size {size} came back misaligned");
            call("free", [at, 0, 0, 0, 0, 0]);
        }
    }

    #[test]
    fn calloc_zeroes_and_refuses_an_overflowing_product() {
        let at = call("calloc", [16, 8, 0, 0, 0, 0]);
        assert_ne!(at, 0);
        // SAFETY: calloc just returned 128 readable bytes here.
        let bytes = unsafe { std::slice::from_raw_parts(super::ptr(at), 128) };
        assert!(bytes.iter().all(|b| *b == 0), "callers rely on this");
        call("free", [at, 0, 0, 0, 0, 0]);

        // The multiplication overflows, and `calloc` refuses it.
        assert_eq!(call("calloc", [u64::MAX, 2, 0, 0, 0, 0]), 0);
    }

    #[test]
    fn realloc_preserves_the_contents_it_can_keep() {
        let at = call("malloc", [16, 0, 0, 0, 0, 0]);
        call("memset", [at, 0x5A, 16, 0, 0, 0]);
        let bigger = call("realloc", [at, 64, 0, 0, 0, 0]);
        assert_ne!(bigger, 0);

        // SAFETY: realloc returned at least 64 readable bytes.
        let bytes = unsafe { std::slice::from_raw_parts(super::ptr(bigger), 16) };
        assert!(
            bytes.iter().all(|b| *b == 0x5A),
            "the old contents must survive"
        );
        call("free", [bigger, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn realloc_of_null_allocates_and_free_of_null_does_nothing() {
        // Both are defined behaviour callers use deliberately.
        let at = call("realloc", [0, 32, 0, 0, 0, 0]);
        assert_ne!(at, 0);
        call("free", [at, 0, 0, 0, 0, 0]);
        assert_eq!(call("free", [0, 0, 0, 0, 0, 0]), 0);
    }

    #[test]
    fn a_failed_allocation_answers_null_rather_than_an_error_code() {
        // An error code in a pointer register is a wild pointer; null is what a caller tests for
        // (D125).
        assert_eq!(call("malloc", [u64::MAX, 0, 0, 0, 0, 0]), 0);
    }

    #[test]
    fn every_implementation_is_also_declared_here_or_says_why_not() {
        /// Implemented here and declared in another library.
        ///
        /// Where a symbol is declared is a claim about the target; where its code lives is a claim
        /// about this repository (D367). These are C library functions with no vendor-named twin,
        /// imported from `libScePosix`, where `orbistoun-posix` delegates to the code here.
        const DECLARED_ELSEWHERE: &[&str] = &[
            "gettimeofday",
            "clock_gettime",
            // The descriptor and mapping calls: POSIX names with no vendor-named twin, declared in
            // `libScePosix` and implemented in the filesystem layer.
            "creat",
            "readv",
            "writev",
            "preadv",
            "pwritev",
            "fsync",
            "fdatasync",
            "getpagesize",
            "madvise",
            // Byte order, for the same reason, beside the sockets that use it.
            "htonl",
            "htons",
            "ntohl",
            "ntohs",
        ];

        // An implementation nobody declared can never be reached: resolution goes through the
        // declared symbol list.
        let declared: Vec<&str> = MODULE.imports.iter().map(|i| i.name).collect();
        for (name, _) in implementations() {
            assert!(
                declared.contains(&name) || DECLARED_ELSEWHERE.contains(&name),
                "{name} is implemented but not declared"
            );
        }
        for name in DECLARED_ELSEWHERE {
            assert!(
                !declared.contains(name),
                "{name} is declared here after all - remove it from the exceptions"
            );
        }
    }

    #[test]
    fn a_format_with_no_conversions_is_copied_through() {
        assert_eq!(
            render_format(b"map_region", &[]).expect("plain"),
            b"map_region"
        );
    }

    #[test]
    fn integers_render_in_the_bases_the_conversions_name() {
        let args = [255_u64, 255, 255, 255];
        assert_eq!(render_format(b"%d", &args).expect("d"), b"255");
        assert_eq!(render_format(b"%u", &args).expect("u"), b"255");
        assert_eq!(render_format(b"%x", &args).expect("x"), b"ff");
        assert_eq!(render_format(b"%X", &args).expect("X"), b"FF");
    }

    #[test]
    fn a_signed_conversion_reads_the_register_as_signed() {
        // Only the conversion says whether the top bit is a sign.
        let args = [u64::MAX];
        assert_eq!(render_format(b"%d", &args).expect("d"), b"-1");
        assert_eq!(render_format(b"%ld", &args).expect("ld"), b"-1");
        // `%u` is an `unsigned int`, thirty-two bits, and `%lu` the whole word.
        assert_eq!(render_format(b"%u", &args).expect("u"), b"4294967295");
        assert_eq!(
            render_format(b"%lu", &args).expect("lu"),
            b"18446744073709551615"
        );
    }

    /// A conversion's default width is `int`, and the modifier changes it: a stack slot's upper
    /// half is unspecified for a narrower argument.
    #[test]
    fn a_conversion_reads_only_as_many_bits_as_its_width() {
        // A zero with rubbish above it, as a stack slot may hold.
        let dirty = [0xFFFF_FFFF_0000_0000_u64];
        assert_eq!(render_format(b"%d", &dirty).expect("d"), b"0");
        assert_eq!(render_format(b"%u", &dirty).expect("u"), b"0");
        assert_eq!(render_format(b"%x", &dirty).expect("x"), b"0");
        // And the modifier says to read all of it.
        assert_eq!(
            render_format(b"%ld", &dirty).expect("ld"),
            b"-4294967296",
            "the value the narrow conversions were reporting"
        );

        let value = [0x1234_5678_9ABC_DEF0_u64];
        assert_eq!(render_format(b"%x", &value).expect("x"), b"9abcdef0");
        assert_eq!(render_format(b"%hx", &value).expect("hx"), b"def0");
        assert_eq!(render_format(b"%hhx", &value).expect("hhx"), b"f0");
        assert_eq!(
            render_format(b"%lx", &value).expect("lx"),
            b"123456789abcdef0"
        );
        assert_eq!(
            render_format(b"%zx", &value).expect("zx"),
            b"123456789abcdef0"
        );
        // A pointer is the whole word whatever else was said.
        assert_eq!(
            render_format(b"%p", &value).expect("p"),
            b"0x123456789abcdef0"
        );
    }

    #[test]
    fn width_and_zero_padding_apply_to_every_conversion() {
        assert_eq!(render_format(b"%5d", &[42]).expect("w"), b"   42");
        assert_eq!(render_format(b"%05d", &[42]).expect("z"), b"00042");
        assert_eq!(render_format(b"%-5d|", &[42]).expect("l"), b"42   |");
    }

    #[test]
    fn a_doubled_percent_consumes_no_argument() {
        // Otherwise a literal percent shifts every later conversion onto the wrong argument.
        assert_eq!(
            render_format(b"100%% of %d", &[7]).expect("pct"),
            b"100% of 7"
        );
    }

    #[test]
    fn length_modifiers_are_consumed_rather_than_mistaken_for_conversions() {
        // `%llu` must not be read as `%l` followed by junk.
        assert_eq!(render_format(b"%llu", &[9]).expect("llu"), b"9");
        assert_eq!(render_format(b"%zu", &[9]).expect("zu"), b"9");
        assert_eq!(render_format(b"%hhd", &[9]).expect("hhd"), b"9");
    }

    #[test]
    fn a_floating_point_conversion_is_refused_rather_than_guessed() {
        // A variadic double arrives in an XMM register, which the trampoline does not capture, so
        // the value never reaches this function.
        assert_eq!(
            render_format(b"%f", &[1, 2, 3]),
            Err(FormatFault::FloatingPoint('f'))
        );
        for spec in [
            &b"%g"[..],
            &b"%e"[..],
            &b"%E"[..],
            &b"%G"[..],
            &b"%a"[..],
            &b"%.2f"[..],
        ] {
            assert!(
                matches!(
                    render_format(spec, &[1, 2, 3]),
                    Err(FormatFault::FloatingPoint(_))
                ),
                "{:?} must be refused, not rendered",
                core::str::from_utf8(spec)
            );
        }
    }

    #[test]
    fn running_out_of_arguments_is_reported_rather_than_padded() {
        // Three integer registers survive the fixed parameters; a fourth conversion has nothing
        // behind it in the array.
        assert_eq!(
            render_format(b"%d %d %d %d", &[1, 2, 3]),
            Err(FormatFault::OutOfArguments)
        );
    }

    #[test]
    fn an_unknown_conversion_names_itself() {
        // So a report can say which conversion to implement next.
        assert_eq!(
            render_format(b"%q", &[1]),
            Err(FormatFault::Unsupported('q'))
        );
    }

    #[test]
    fn a_trailing_percent_is_a_fault_not_a_silent_drop() {
        assert!(render_format(b"nearly %", &[]).is_err());
    }

    #[test]
    fn a_null_string_argument_renders_the_conventional_placeholder() {
        // A null prints `(null)` rather than faulting inside formatting.
        assert_eq!(render_format(b"[%s]", &[0]).expect("null"), b"[(null)]");
    }

    #[test]
    fn a_string_argument_is_read_from_guest_memory_and_honours_precision() {
        let text = b"texture_atlas\0";
        let address = text.as_ptr() as usize as u64;
        assert_eq!(
            render_format(b"%s", &[address]).expect("s"),
            b"texture_atlas"
        );
        assert_eq!(render_format(b"%.7s", &[address]).expect("p"), b"texture");
    }

    #[test]
    fn the_destination_is_terminated_and_the_full_length_is_reported() {
        // The return is what would have been written, so a caller detects truncation by comparing
        // it against the size it passed.
        let format = b"region_%d\0";
        let mut dest = [0xAA_u8; 32];
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = dest.as_mut_ptr() as usize as u64;
        args[1] = dest.len() as u64;
        args[2] = format.as_ptr() as usize as u64;
        args[3] = 47;

        assert_eq!(snprintf_s(&args), 9);
        assert_eq!(&dest[..10], b"region_47\0");
    }

    #[test]
    fn a_result_that_does_not_fit_is_cut_short_and_still_terminated() {
        // A small buffer's neighbours are not overwritten.
        let format = b"region_%d\0";
        let mut buffer = [0xAA_u8; 16];
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = buffer.as_mut_ptr() as usize as u64;
        args[1] = 5;
        args[2] = format.as_ptr() as usize as u64;
        args[3] = 47;

        assert_eq!(snprintf_s(&args), 9, "reports what would have been written");
        assert_eq!(&buffer[..5], b"regi\0", "four bytes plus a terminator");
        assert_eq!(buffer[5], 0xAA, "and nothing beyond the size it was given");
    }

    #[test]
    fn a_format_that_cannot_be_honoured_empties_the_destination() {
        // An empty string, not a half-rendered one.
        let format = b"%f percent\0";
        let mut dest = [0xAA_u8; 32];
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = dest.as_mut_ptr() as usize as u64;
        args[1] = dest.len() as u64;
        args[2] = format.as_ptr() as usize as u64;

        assert_eq!(snprintf_s(&args), 0);
        assert_eq!(dest[0], 0, "terminated, so the guest reads an empty string");
        assert!(
            super::format_stats().first_fault.is_some(),
            "and the run can say a conversion was responsible"
        );
        // Which conversion, asked of the renderer: `first_fault` is one process-wide slot shared by
        // every test, so its value depends on which test ran first.
        assert_eq!(
            render_format(b"%f percent", &[]),
            Err(FormatFault::FloatingPoint('f')),
            "and it can say which conversion was responsible"
        );
    }

    #[test]
    fn a_zero_sized_destination_is_left_alone_and_the_length_is_still_reported() {
        // The destination is untouched, since there is no room even for a terminator. The return is
        // the length the output would have needed, per ISO C 7.21.6.5.
        let format = b"x\0";
        let mut dest = [0xAA_u8; 4];
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = dest.as_mut_ptr() as usize as u64;
        args[1] = 0;
        args[2] = format.as_ptr() as usize as u64;

        assert_eq!(
            snprintf_s(&args),
            1,
            "one character would have been written"
        );
        assert_eq!(dest, [0xAA; 4], "and none of it was");
    }
    #[test]
    fn memalign_returns_what_it_was_asked_for() {
        // Every power-of-two alignment is honoured.
        for align in [8_u64, 16, 32, 64, 256, 4096] {
            let p = call("memalign", [align, 300, 0, 0, 0, 0]);
            assert_ne!(p, 0, "alignment {align} should be satisfiable");
            assert_eq!(p % align, 0, "alignment {align} was not honoured");
            call("free", [p, 0, 0, 0, 0, 0]);
        }
    }

    #[test]
    fn an_alignment_that_is_not_a_power_of_two_is_refused() {
        // Not a power of two, so refused rather than rounded up.
        assert_eq!(call("memalign", [24, 100, 0, 0, 0, 0]), 0);
        assert_eq!(call("memalign", [0, 100, 0, 0, 0, 0]), 0);
    }

    #[test]
    fn an_aligned_block_frees_through_the_same_path_as_an_ordinary_one() {
        // `dealloc` must receive the layout `alloc` did, so the alignment survives from allocation
        // to release.
        let a = call("memalign", [4096, 64, 0, 0, 0, 0]);
        let b = call("malloc", [64, 0, 0, 0, 0, 0]);
        assert_ne!(a, 0);
        assert_ne!(b, 0);
        call("free", [a, 0, 0, 0, 0, 0]);
        call("free", [b, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn realloc_preserves_the_payload_of_an_aligned_block() {
        // `realloc` sizes the copy from the recorded header, not the header size, so a heavily
        // aligned block does not copy past its payload.
        let p = call("memalign", [256, 8, 0, 0, 0, 0]);
        assert_ne!(p, 0);
        // SAFETY: `memalign` returned eight writable bytes here.
        unsafe { std::ptr::write_bytes(super::ptr(p), 0xAB, 8) };
        let grown = call("realloc", [p, 32, 0, 0, 0, 0]);
        assert_ne!(grown, 0);
        // SAFETY: `realloc` returned at least 32 readable bytes.
        let seen = unsafe { std::slice::from_raw_parts(super::ptr(grown).cast_const(), 8) };
        assert_eq!(seen, [0xAB; 8], "the payload survived");
        call("free", [grown, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn a_pointer_this_library_never_handed_out_is_declined() {
        // A wild pointer becomes a no-op rather than a `dealloc` against a layout nobody allocated.
        let mut junk = [0_u64; 8];
        let stray = std::ptr::addr_of_mut!(junk[4]) as usize as u64;
        assert_eq!(call("free", [stray, 0, 0, 0, 0, 0]), 0);
    }

    /// A guest's argument list, laid out as the psABI says one is.
    ///
    /// Under the identity mapping a list built on the host is one a guest could pass. The register
    /// half is filled from `spilled`, and everything past six goes to the overflow area.
    struct GuestArguments {
        /// The register save area, six words of it.
        save: Box<[u64; 6]>,
        /// The stack arguments, past the sixth.
        overflow: Vec<u64>,
        /// The `va_list` structure itself.
        list: Box<[u64; 3]>,
    }

    impl GuestArguments {
        fn new(values: &[u64], already_spent: u32) -> Self {
            let mut save = Box::new([0_u64; 6]);
            let taken = usize::try_from(already_spent / 8).expect("a small count");
            for (slot, value) in save.iter_mut().skip(taken).zip(values) {
                *slot = *value;
            }
            let overflow: Vec<u64> = values.iter().skip(6 - taken).copied().collect();
            let list = Box::new([
                u64::from(already_spent),
                overflow.as_ptr() as u64,
                save.as_ptr() as u64,
            ]);
            Self {
                save,
                overflow,
                list,
            }
        }

        /// The address a guest would pass as `ap`.
        fn address(&self) -> u64 {
            // Read so the fields are not dead storage to the compiler; the addresses inside `list`
            // point at both.
            let _ = (self.save.len(), self.overflow.len());
            self.list.as_ptr() as u64
        }
    }

    /// A `va_list` renders a seventh conversion, which the register form refuses (D364).
    #[test]
    fn a_va_list_renders_a_format_longer_than_the_registers_hold() {
        let seven = [1_u64, 2, 3, 4, 5, 6, 7];
        assert_eq!(
            render_format(b"%d %d %d %d %d %d %d", &seven[..6]),
            Err(FormatFault::OutOfArguments),
            "the register form cannot reach the seventh"
        );

        let arguments = GuestArguments::new(&seven, 0);
        let mut buffer = [0_u8; 32];
        let dest = buffer.as_mut_ptr() as u64;
        let written = call(
            "vsnprintf",
            [
                dest,
                buffer.len() as u64,
                c"%d %d %d %d %d %d %d".as_ptr() as u64,
                arguments.address(),
                0,
                0,
            ],
        );
        assert_eq!(written, 13);
        assert_eq!(&buffer[..13], b"1 2 3 4 5 6 7");
    }

    /// A size of zero measures rather than refuses.
    #[test]
    fn a_size_of_zero_answers_the_length_and_writes_nothing() {
        let arguments = GuestArguments::new(&[42], 0);
        let asked = call(
            "vsnprintf",
            [0, 0, c"value %d".as_ptr() as u64, arguments.address(), 0, 0],
        );
        assert_eq!(asked, 8, "the length the answer would have been");
    }

    /// Truncation reports the full length, so a caller can detect it by comparing.
    #[test]
    fn a_short_buffer_is_cut_and_the_full_length_reported() {
        let arguments = GuestArguments::new(&[123_456], 0);
        let mut buffer = [0xFF_u8; 4];
        let written = call(
            "vsnprintf",
            [
                buffer.as_mut_ptr() as u64,
                buffer.len() as u64,
                c"%d".as_ptr() as u64,
                arguments.address(),
                0,
                0,
            ],
        );
        assert_eq!(written, 6, "what it would have been");
        assert_eq!(&buffer, b"123\0", "three bytes and a terminator");
    }

    /// A caller that spent registers on its own fixed parameters starts partway in: `vsnprintf`
    /// has three.
    #[test]
    fn a_list_starting_partway_through_the_registers_reads_the_right_arguments() {
        let arguments = GuestArguments::new(&[7, 8], 24);
        let mut buffer = [0_u8; 16];
        let written = call(
            "vsnprintf",
            [
                buffer.as_mut_ptr() as u64,
                buffer.len() as u64,
                c"%d/%d".as_ptr() as u64,
                arguments.address(),
                0,
                0,
            ],
        );
        assert_eq!(written, 3);
        assert_eq!(&buffer[..3], b"7/8");
    }

    /// A null list writes nothing rather than reading through zero.
    #[test]
    fn a_null_argument_list_is_refused_and_the_destination_terminated() {
        let mut buffer = [0xFF_u8; 8];
        let written = call(
            "vsnprintf",
            [
                buffer.as_mut_ptr() as u64,
                buffer.len() as u64,
                c"%d".as_ptr() as u64,
                0,
                0,
                0,
            ],
        );
        assert_eq!(written, 0);
        assert_eq!(
            buffer[0], 0,
            "terminated, so a caller printing it prints nothing"
        );
    }

    /// The integer knobs answer the values and widths measured on hardware.
    #[test]
    fn measured_integer_knobs_answer_hardware_values() {
        assert_eq!(super::answer_integer("hw.ncpu"), Some((16, 4)));
        assert_eq!(super::answer_integer("hw.pagesize"), Some((0x4000, 4)));
        assert_eq!(
            super::answer_integer("machdep.tsc_freq"),
            Some((0x5f25_9b8e, 8)),
            "the counter frequency, by a third route"
        );
        assert_eq!(super::answer_integer("hw.nonesuch"), None);
    }

    /// `kern.ostype` answers what the platform answers, the same on every machine.
    #[test]
    fn ostype_is_the_measured_platform_name() {
        assert_eq!(super::answer_for("kern.ostype"), Some("FreeBSD".to_owned()));
    }

    /// `kern.osrelease` is answered whatever its value, including when it has none (D447).
    #[test]
    fn the_release_knob_answers_even_when_it_has_no_value() {
        assert!(
            super::answer_for("kern.osrelease").is_some(),
            "an unset release is an empty knob, not a missing one"
        );
    }

    /// Names orbistoun cannot source are refused rather than answered plausibly.
    ///
    /// `hw.availpages` is live memory state and `hw.physmem` is refused by the hardware itself.
    /// `kern.version` is refused on a machine that does not carry one, the default (D675).
    /// `hw.ncpu` is an integer knob answered by `answer_integer`, so it is not listed here.
    #[test]
    fn unsourceable_knobs_are_refused_rather_than_invented() {
        use orbistoun_core::machine::Machine;

        let unset = Machine::default();
        assert_eq!(super::integer_knob(&unset, "hw.availpages"), None);
        assert_eq!(super::answer_for("hw.physmem"), None);
        assert_eq!(super::text_knob(&unset, "kern.version"), None);
        assert_eq!(super::answer_for("kern.nonesuch"), None);
    }

    /// `kern.osrevision` and `hw.machine` answer published values from FreeBSD's headers, which
    /// hardware answers too (`135-sysctl/names`).
    #[test]
    fn published_knobs_answer_on_every_machine() {
        assert_eq!(super::answer_integer("kern.osrevision"), Some((199_506, 4)));
        assert_eq!(super::answer_for("hw.machine"), Some("amd64".to_owned()));
    }

    /// A machine's own knobs answer from its profile, and refuse when it has none (D675).
    ///
    /// Asserted against constructed machines, since the presented one is a process-wide slot.
    #[test]
    fn a_machines_own_knobs_answer_only_when_it_carries_them() {
        use orbistoun_core::machine::Machine;

        let unset = Machine::default();
        assert_eq!(super::text_knob(&unset, "kern.version"), None);
        assert_eq!(super::text_knob(&unset, "hw.model"), None);
        assert_eq!(super::integer_knob(&unset, "kern.sdk_version"), None);

        let measured = Machine {
            kernel_version: "r226974/releases/12.40 Nov 27 2025 02:23:38".to_owned(),
            kernel_sdk_version: 0x1240_0009,
            hardware_model: format!("{:<47}", "100-000000189"),
            ..Machine::default()
        };
        assert_eq!(
            super::text_knob(&measured, "kern.version").as_deref(),
            Some("r226974/releases/12.40 Nov 27 2025 02:23:38")
        );
        assert_eq!(
            super::integer_knob(&measured, "kern.sdk_version"),
            Some((0x1240_0009, 4)),
            "a four-byte int, the width hardware wrote"
        );
        assert_eq!(
            super::text_knob(&measured, "hw.model").map(|model| model.len()),
            Some(47),
            "trailing spaces kept - the console wrote 47 bytes"
        );
    }

    /// A knob that does not exist is told apart from a buffer too small for one that does: only
    /// the second is worth retrying.
    #[test]
    fn a_missing_knob_and_a_short_buffer_answer_differently() {
        use orbistoun_core::errno;

        let missing = super::sysctl_failed(errno::NO_ENTRY);
        let short = super::sysctl_failed(errno::NO_MEMORY);
        assert_ne!(missing, short);

        // Both carry the vendor encoding rather than this crate's generic refusal.
        assert_ne!(missing, super::FAILED);
        assert_eq!(missing, 0x8002_0002, "the errno pattern, name not found");
        assert_eq!(short, 0x8002_000c, "and the one a retry can fix");
    }

    /// An integer knob writes its width and no more, and reports that width through the pair.
    #[test]
    fn an_integer_knob_writes_exactly_its_width() {
        let mut value = [0xEE_u8; 8];
        let mut len = 8_u64;
        let rc = super::answer_bytes(
            &16_u64.to_le_bytes()[..4],
            value.as_mut_ptr() as u64,
            std::ptr::from_mut(&mut len) as u64,
        );
        assert_eq!(rc, super::OK);
        assert_eq!(len, 4, "the width is reported through the length pointer");
        assert_eq!(&value[..4], &[16, 0, 0, 0], "little-endian, four bytes");
        assert_eq!(
            &value[4..],
            &[0xEE; 4],
            "and nothing past the width is touched"
        );
    }
}

#[cfg(test)]
mod environment {
    use super::{GUEST_ARG_REGISTERS, environment_value, setenv, unsetenv};

    /// A NUL-terminated guest string this test owns.
    fn text(s: &str) -> Box<[u8]> {
        format!("{s}\0").into_bytes().into_boxed_slice()
    }

    fn call(f: fn(&[u64; GUEST_ARG_REGISTERS]) -> u64, args: [u64; 3]) -> u64 {
        let mut regs = [0_u64; GUEST_ARG_REGISTERS];
        regs[..3].copy_from_slice(&args);
        f(&regs)
    }

    fn value_of(name: &str) -> Option<String> {
        let at = environment_value(name)?;
        // SAFETY: `environment_value` answers a NUL-terminated leaked buffer this library
        // owns, so reading up to its terminator stays inside it.
        let bytes = unsafe { std::ffi::CStr::from_ptr(at.cast()) };
        Some(bytes.to_string_lossy().into_owned())
    }

    /// What is set can be read back.
    #[test]
    fn what_setenv_writes_getenv_reads() {
        let (name, value) = (text("ORBISTOUN_TEST_A"), text("first"));
        assert_eq!(
            call(setenv, [name.as_ptr() as u64, value.as_ptr() as u64, 1]),
            0
        );
        assert_eq!(value_of("ORBISTOUN_TEST_A").as_deref(), Some("first"));
    }

    /// An `overwrite` of zero leaves an existing variable alone and still reports success.
    #[test]
    fn overwrite_zero_keeps_the_existing_value_and_still_succeeds() {
        let (name, first, second) = (text("ORBISTOUN_TEST_B"), text("kept"), text("ignored"));
        assert_eq!(
            call(setenv, [name.as_ptr() as u64, first.as_ptr() as u64, 1]),
            0
        );
        assert_eq!(
            call(setenv, [name.as_ptr() as u64, second.as_ptr() as u64, 0]),
            0,
            "not overwriting is success, not failure"
        );
        assert_eq!(value_of("ORBISTOUN_TEST_B").as_deref(), Some("kept"));
        assert_eq!(
            call(setenv, [name.as_ptr() as u64, second.as_ptr() as u64, 1]),
            0
        );
        assert_eq!(
            value_of("ORBISTOUN_TEST_B").as_deref(),
            Some("ignored"),
            "and overwrite of one does replace it"
        );
    }

    /// A name containing `=`, or an empty one, is the documented `EINVAL`.
    #[test]
    fn a_name_with_an_equals_sign_is_refused() {
        let (bad, empty, value) = (text("HAS=EQUALS"), text(""), text("x"));
        assert_ne!(
            call(setenv, [bad.as_ptr() as u64, value.as_ptr() as u64, 1]),
            0
        );
        assert_ne!(
            call(setenv, [empty.as_ptr() as u64, value.as_ptr() as u64, 1]),
            0
        );
        assert_ne!(call(setenv, [0, value.as_ptr() as u64, 1]), 0, "and null");
    }

    /// `unsetenv` removes what this layer holds.
    #[test]
    fn unsetenv_removes_what_setenv_added() {
        let (name, value) = (text("ORBISTOUN_TEST_C"), text("gone"));
        call(setenv, [name.as_ptr() as u64, value.as_ptr() as u64, 1]);
        assert!(value_of("ORBISTOUN_TEST_C").is_some());
        assert_eq!(call(unsetenv, [name.as_ptr() as u64, 0, 0]), 0);
        assert_eq!(value_of("ORBISTOUN_TEST_C"), None);
    }
}
