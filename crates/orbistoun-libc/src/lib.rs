//! The C library, as the guest calls it.
//!
//! Chosen by measurement, and the measurement was emphatic. Every title that faults
//! rather than spins was faulting **because of this**: one calls `memset` three hundred
//! times and then writes to `0x7fff0119`, another calls `strlen` and `memcpy` two hundred
//! and forty-eight times each and then reads `0x5`. They were being told "not
//! implemented" and carrying on with the answer (D123).
//!
//! `strlen` returning an error code instead of a length is not a missing feature - it is
//! a guest that now believes every string is fourteen bytes long. The damage is immediate
//! and surfaces somewhere unrelated, which is exactly why these looked like three
//! different mysteries.
//!
//! # Why this is the easiest correct code in the project
//!
//! Everything else here is guessing at undocumented semantics. This is not: ISO C and
//! POSIX say precisely what these do, the target library is FreeBSD-derived, and both are
//! citable. There is no oracle problem, no plausible-wrong-answer risk, and no reason for
//! any of it to be subtly off.
//!
//! # Guest pointers are host pointers
//!
//! The address space is identity-mapped, so a pointer the guest hands over can be
//! dereferenced directly. That is what makes these implementable in a few lines rather
//! than through a translation layer - and it is also why every one of them is `unsafe`
//! and why the guest is trusted about its own arguments, exactly as the real library
//! trusts them. A guest that passes a bad pointer faults here precisely as it would have
//! faulted there, and the fault reporter names the address.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};
mod clock;
pub mod cstring;
pub mod math;
mod scan;
mod varargs;

use orbistoun_hle::guest_module;

guest_module! {
    "libc" {
        // `std::call_once`'s engine. Declared here because a title imports it from libc, and
        // implemented in `orbistoun-kernel` because it must run a guest callback on a fresh stack -
        // the reentrant call the thread registry owns (D367). Three integer arguments: the flag, the
        // callback, and its context.
        "_ZSt13_Execute_onceRSt9once_flagPFiPvS1_PS1_ES1_" => 3,
        // The C-runtime threading primitives `std::mutex`, `std::condition_variable` and
        // `std::this_thread` lower onto. Declared here because a title imports them from libc, and
        // implemented in `orbistoun-kernel` beside the thread registry and the `sync` primitives
        // that give them real mutual exclusion. Arity is the argument count each carries: the object
        // it acts on, plus a type for `_Mtx_init`, a mutex for the condition waits, and a deadline
        // for the timed one. `_Xtime_get_ticks` takes none and answers the clock.
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
        "_Thrd_sleep" => 2,
        // The maths library. Arity is the count of *floating-point* arguments here, which
        // is what these take - none of them touches an integer register. Declared beside
        // the rest because they are the same library, and separated only by how they are
        // implemented (D268).
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
        "snprintf" => 3, "sprintf" => 2,
        // The `va_list` forms. Fixed parameters only - the variadic half arrives through
        // the list rather than in registers, which is the whole point of them (D364).
        "vsnprintf" => 4, "vprintf" => 2, "vfprintf" => 3,
        // Breaking a `time_t` down and rendering it. A Unity title formats a timestamp with
        // `asctime(localtime(&t))`; unimplemented, `localtime` answered a placeholder that
        // `asctime` then `printf`'d as a `%s`, faulting on our own error code (D454).
        "localtime" => 1, "gmtime" => 1, "asctime" => 1,
        // Time, and waiting. Wanted by more of the open-toolchain payloads between them
        // than the whole socket set, and every one is POSIX-documented outright.
        // `gettimeofday` and `clock_gettime` are implemented here and **declared in
        // `libScePosix`**, where a title was measured importing them. One declaration per
        // symbol is the rule; which crate holds the code is a separate question.
        "time" => 1, "sleep" => 1, "usleep" => 1, "nanosleep" => 2,
        "kill" => 2,
        // The two calls between `ftpsrv` and a listening port (D382).
        "sscanf" => 6, "strftime" => 4,
        "getenv" => 1, "getcwd" => 2, "perror" => 1, "strerror_r" => 3,
        // The file calls that change a directory. Declared here, where FreeBSD puts them,
        // and implemented in `orbistoun-fs`, where the mount model that decides whether a
        // guest may touch a path lives (D367).
        "mkdir" => 2, "rmdir" => 1, "unlink" => 1, "remove" => 1,
        "rename" => 2, "access" => 2, "truncate" => 2, "ftruncate" => 2,
        "pread" => 4, "pwrite" => 4, "dup2" => 2,
        "chmod" => 2, "fchmod" => 2, "mlock" => 2, "munlock" => 2,
        "fdopen" => 2, "fileno" => 1, "sendfile" => 6,
        // Which addresses a guest can be reached on, and how it prints one. `inet_ntop` is
        // declared in `libScePosix`, where a title imports it; the underscored spelling the
        // payloads import is FreeBSD's own and is declared here (D367).
        "getifaddrs" => 1, "freeifaddrs" => 1, "__inet_ntop" => 4, "__inet_pton" => 3,
        // How a program that serves files decides whether a path is real before acting on
        // it. Every path a client names goes through it.
        "realpath" => 2,
        // Waiting on many descriptors at once, which is what a server with more than one
        // client uses instead of `select`. Implemented in `orbistoun-fs` beside the
        // descriptor table, and declared here because that is where a payload imports them
        // from (D367).
        "kqueue" => 0, "kevent" => 6,
        // The same question `sysctl` answers, asked by name - which is how a guest asks what
        // kernel it is on, and it branches on the answer (D397).
        "sysctlbyname" => 5,
        // What a descriptor is set to. Three of the payloads read the flags, change one bit
        // and write them back, so an unimplemented answer does not stay where it was put.
        "fcntl" => 3,
        // What a guest is told about a file, and how it lists a directory. `fstat` is
        // declared in `libScePosix`, where a title imports it.
        "stat" => 2, "lstat" => 2,
        "opendir" => 1, "readdir" => 1, "closedir" => 1,
        "strdup" => 1, "strndup" => 2, "strncat" => 3,
        "strtok" => 2, "strtok_r" => 3,
        "qsort" => 4, "bsearch" => 5,
        "memset" => 3,
        "memcpy" => 3,
        "memmove" => 3,
        "memcmp" => 3,
        // BSD `bcmp` - the same byte comparison as `memcmp`, but its contract is only
        // equal/not-equal rather than ordering, so `memcmp` answers it exactly. 16k calls in
        // one PPSA21564 boot; unimplemented it answered a nonzero placeholder, which reads as
        // "always differ" and can wedge a compare loop (D451).
        "bcmp" => 3,
        "memchr" => 3,
        "strlen" => 1,
        "strnlen" => 2,
        "strcmp" => 2,
        "strncmp" => 3,
        "strcpy" => 2,
        "strncpy" => 3,
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
        // The Itanium C++ ABI's allocation operators - published names, and `_Znwm` was
        // confirmed against a real import by hash before being written here (D165).
        // `new` and `delete` are the heap under another spelling, so they are the heap.
        "_Znwm" => 1,
        "_Znam" => 1,
        "_ZdlPv" => 1,
        "_ZdaPv" => 1,
        "_ZdlPvm" => 2,
        "_ZdaPvm" => 2,
        // Stdio. Declared before any of it is implemented, because *declaring* is what
        // lets the knowledge file say `fopen` returns a pointer - and an undeclared
        // pointer-returning function falls to the default stub, which answers an error
        // code the guest then carries around as a `FILE*` (D165).
        "fopen" => 2,
        "fclose" => 1,
        "fread" => 4,
        "fwrite" => 4,
        "fseek" => 3,
        "ftell" => 1,
        "rewind" => 1,
        // Reads a line. 6.5M calls in one PPSA21564 boot, because unimplemented it never
        // answered the NULL that ends a read loop (D454).
        "fgets" => 3,
        "feof" => 1,
        "ferror" => 1,
        "fflush" => 1,
        // 76 calls in one boot, formatting the name string the map call takes. Confirmed
        // by hash; the bounds-checked spelling, which is why the obvious ones missed.
        "snprintf_s" => 6,
        // One fixed parameter and whatever the format asks for. Declared as the full
        // register set because the arity of a variadic function is a property of each
        // call, not of the function - and under-declaring would truncate the arguments
        // before the renderer ever saw them.
        "memalign" => 2,
        "printf" => 6,
        // Declared because it must **not return**. Undeclared it fell to the default
        // stub, returned, and execution ran into the trap a compiler places after a
        // `noreturn` call - reported as `illegal instruction` at an address that meant
        // nothing, in two titles (D177).
        "abort" => 0,
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
        // `getpid()`.
        "getpid" => 0,
        // `sysctl(name, namelen, oldp, oldlenp, newp, newlen)`. FreeBSD `sysctl(3)`.
        "sysctl" => 6,
        // `fprintf(stream, format, ...)`. One more argument than `printf`.
        "fprintf" => 6,
        // The platform's exposed Doug Lea `mspace` allocator. The arena is the leading
        // argument; the rest is the ordinary allocator. Declared because they answer
        // pointers, and an undeclared one hands back the placeholder the guest then writes
        // through - PPSA21564's `write to 0x7fff0001` wall (D451).
        "sceLibcMspaceMalloc" => 2,
        "sceLibcMspaceCalloc" => 3,
        "sceLibcMspaceRealloc" => 3,
        "sceLibcMspaceFree" => 2,
        "exit" => 1,
        "_Exit" => 1,
    }
}

/// Success, as a C caller reads it.
const OK: u64 = 0;

/// What C's stdio returns on failure: `EOF`, which is negative one widened to the
/// register the guest reads.
const EOF: u64 = u64::MAX;

/// Longest string these will walk before giving up.
///
/// A guest that hands over an unterminated buffer would otherwise walk until it faults,
/// and the fault would appear to be in string handling rather than in whatever produced
/// the buffer. Generous enough that no real string reaches it.
const MAX_STRING: usize = 64 * 1024 * 1024;

/// Reinterprets a guest address as a pointer.
///
/// The mapping is identity, so this is a change of type rather than of value. Not
/// `const`: exposing provenance only became usable in a const context after this
/// project's minimum supported version.
pub(crate) fn ptr(address: u64) -> *mut u8 {
    std::ptr::with_exposed_provenance_mut(address as usize)
}

/// Length of a NUL-terminated guest string, bounded.
///
/// # Safety
///
/// `address` must point at readable guest memory containing a NUL within [`MAX_STRING`]
/// bytes, which is the same contract the real function has.
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
        // SAFETY: the guest supplied destination and length, exactly as the real call
        // does. The mapping is identity, so this writes the memory it named.
        unsafe { std::ptr::write_bytes(ptr(dest), byte, count) };
    }
    // Returns its destination, which callers chain on.
    dest
}

fn memcpy(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (dest, src, count) = (args[0], args[1], args[2] as usize);
    if dest != 0 && src != 0 && count > 0 {
        // SAFETY: guest-supplied pointers and length. `copy` rather than
        // `copy_nonoverlapping` because a guest that overlaps here would get silent
        // corruption from the stricter one, and being permissive costs nothing.
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
    // Sign is what matters and the magnitude is unspecified, but returning the byte
    // difference matches what callers that do arithmetic on it expect.
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
    // SAFETY: guest-supplied string, bounded twice - by its own limit and by ours.
    let len = unsafe { c_len(args[0]) };
    (len as u64).min(args[1])
}

fn strcmp(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let mut probe = [0_u64; GUEST_ARG_REGISTERS];
    // SAFETY: a guest-supplied string, bounded by MAX_STRING.
    let a = unsafe { c_len(args[0]) };
    // SAFETY: the other guest-supplied string, same contract.
    let b = unsafe { c_len(args[1]) };
    // The shorter length plus one, so the terminator itself is compared - that is what
    // makes two strings differing only after their NUL compare equal.
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
    // The shorter of the two plus its terminator, or the caller's limit, whichever
    // comes first.
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
    // SAFETY: guest-supplied pointers; the terminator is copied with the string, which
    // is what makes the result a valid C string.
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
        // The standard pads the remainder with NUL, and callers rely on it - a partial
        // copy left unpadded is an unterminated string.
        // SAFETY: the destination is at least `limit` bytes by the caller's contract.
        unsafe { std::ptr::write_bytes(ptr(dest + len as u64), 0, limit - len) };
    }
    dest
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
    // SAFETY: guest-supplied string. The terminator is searchable, which the standard
    // requires: `strchr(s, 0)` returns the end of the string, not null.
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
/// Accepts the registration and never runs it. Recorded honestly rather than pretended:
/// nothing here tears a guest down yet, so a handler would have no moment to be called -
/// but refusing the registration makes a guest think its runtime failed to initialise,
/// which is worse than a handler that never fires.
fn atexit(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    0
}

/// `signal(sig, handler)` - records a handler and answers the one it replaced.
///
/// # What this does and deliberately does not do
///
/// It **records**, and nothing here delivers a signal. That is honest rather than
/// convenient: a guest installing a handler is asking to be told about an event this
/// emulator has no way to generate, and pretending otherwise would be a stub answering
/// success for something that will never happen.
///
/// What it must not do is fail. The first thing a network server does is
/// `signal(SIGPIPE, SIG_IGN)` so a write to a closed socket does not kill it - and a
/// `SIG_ERR` there sends a correctly-written program down its error path before it has
/// done anything at all. `klogsrv` makes exactly this call, with `0xd`, as its very first
/// act (D343).
///
/// # The return value
///
/// The handler previously installed, which is `SIG_DFL` - zero - until something installs
/// one. That is a fact rather than a placeholder: nothing had been installed, and zero is
/// what "nothing" is spelled as here.
///
/// Reference: FreeBSD `signal(3)`, POSIX.1-2008 `<signal.h>`.
fn signal(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    /// How many signal numbers to keep handlers for.
    ///
    /// FreeBSD defines signals 1..=31 plus real-time ones above. A guest asking about a
    /// number past this gets `SIG_DFL` back rather than a refusal, because refusing would
    /// be a claim about which signals exist that nothing here has measured.
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
/// # Why this is here so early
///
/// It is the first thing both payloads measured do once they reach `main` (D343). A server
/// that cannot parse its own arguments never reaches the socket it exists to open.
///
/// # State, and where it lives
///
/// `getopt` keeps its position across calls and reports the current argument through the
/// **guest's own** `optarg` and `optind` - globals the guest imports and this layer
/// reserves storage for (D307, D323). So the position is kept here and written *there*:
/// writing it anywhere else would leave the guest reading a zero it can see and this
/// library holding a value it cannot.
///
/// A guest that does not import `optarg` gets no write rather than an invented one.
///
/// Reference: POSIX.1-2008 `getopt(3)`, and FreeBSD's documented behaviour for the
/// leading-colon form.
fn getopt(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    /// What `getopt` answers when there is nothing left.
    ///
    /// The standard says `-1`. A guest reads `eax`, so what it must find there is `-1` as
    /// a 32-bit value - written out rather than converted, because `From` is not const.
    const DONE: u64 = 0xFFFF_FFFF;

    /// Where parsing has reached - one past the program name until something moves it.
    ///
    /// Declared with the other items rather than beside its first use: items exist from the
    /// start of the scope whatever line they are written on, and the lint is right that
    /// pretending otherwise reads as a statement.
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1);

    // Spelled out rather than destructured together: `argc` and `argv` differ by one
    // character, which the lints refuse and a reader would misread just as easily.
    let count = args[0];
    let vector = args[1];
    let options = args[2];

    // **A wild count is refused rather than iterated.** Entered at `main` the guest may
    // have been handed whatever the entry-argument setting supplies, and walking a
    // pointer array sized by a stray value would fault inside this call - reported as the
    // guest's fault, which is the failure a library function must not have.
    let Ok(count) = usize::try_from(count) else {
        return DONE;
    };
    if count > MAX_ARGUMENTS || vector == 0 || options == 0 {
        return DONE;
    }

    let index = NEXT.load(std::sync::atomic::Ordering::Relaxed);
    // Publish the position the guest reads, whether or not anything is left to report.
    write_guest_word("optind", index as u64);

    if index >= count {
        return DONE;
    }

    // Nothing further is implemented yet: every payload measured runs with no arguments,
    // where the answer above is the whole of the contract. An argument that is actually
    // present would need the option letter matched against `optstring` and `optarg` set,
    // and answering anything here without doing that would be a wrong answer rather than
    // an absent one.
    DONE
}

/// `__error()` - a pointer to this thread's `errno`.
///
/// # Why a pointer, and why it must be real
///
/// `errno` is a macro that expands to `*__error()` on FreeBSD, so **every** use of it in a
/// guest goes through here and immediately dereferences what comes back. A stub answering
/// a placeholder gives the guest a wild pointer it then reads and writes, which is what
/// `klogsrv` did: `read of 0x7fff0001` - orbistoun's own placeholder, read as an address
/// (D344).
///
/// This is the same shape as the data imports (D323): the library has to own storage, not
/// merely answer a value.
///
/// # Per thread, deliberately
///
/// `errno` is thread-local by definition, and sharing one word between guest threads would
/// let one thread's failure appear as another's. `thread_local!` here is host-thread-local,
/// and a guest thread is a host thread in this emulator, so the two coincide.
///
/// Reference: FreeBSD `errno(2)`, POSIX.1-2008 `<errno.h>`.
fn error_location(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    thread_local! {
        /// This thread's `errno`. A `thread_local` already has an address stable for the
        /// life of the thread, so there is nothing to box.
        static ERRNO: std::cell::UnsafeCell<i32> = const { std::cell::UnsafeCell::new(0) };
    }
    ERRNO.with(|cell| cell.get() as usize as u64)
}

/// The largest argument count this will walk.
///
/// Not a limit the standard has. It is a guard against a count that did not come from a
/// real process image, which is possible whenever a run enters somewhere other than the
/// declared entry point.
const MAX_ARGUMENTS: usize = 1024;

/// Writes a word into a guest global this layer reserved storage for.
///
/// Silently does nothing when the guest does not import that name, which is the honest
/// answer: a program with no `optarg` has nowhere for one to go, and inventing somewhere
/// would put a value where nothing will ever read it.
fn write_guest_word(name: &str, value: u64) {
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
/// `free` is given only a pointer, so the size has to live somewhere. Sixteen rather than
/// eight because that is the alignment `malloc` must return for any type on x86-64 - a
/// smaller header would hand back memory that faults the first time a caller puts a
/// vector type in it.
const HEAP_HEADER: usize = 16;

/// One machine word, which is what each half of the header holds.
const WORD: usize = size_of::<usize>();

/// `malloc(size)`.
///
/// **Its absence was the wall behind a title that had otherwise reached graphics
/// initialisation.** Unimplemented, it returned the placeholder error code; the guest
/// took that as its buffer and handed it to `memset`, which faithfully wrote there
/// (D128).
/// Allocates `size` bytes whose address is a multiple of `align`.
///
/// # One path, because two would drift
///
/// `malloc` is this with `align` at the header size, and `memalign` is this with whatever
/// the caller asked for. A separate aligned path would have to write a header `free` could
/// still read, and the first time the two disagreed the failure would be a heap corruption
/// with no connection to either (D190).
///
/// The block looks like this, and `offset` is what makes it recoverable:
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
/// `offset` equals the alignment the layout was built with, so storing it stores both facts
/// at once: where the allocation starts, and what `dealloc` must be told. `dealloc` given a
/// layout that differs from the one `alloc` received is undefined behaviour, which is why
/// the alignment cannot simply be recomputed and hoped over.
fn allocate(size: usize, align: usize) -> u64 {
    // **Conforming is not the same as compatible.** The standard permits `malloc(0)` to
    // answer null *or* a unique pointer, and this answered null because it is simpler and a
    // caller must not dereference either. That reasoning is about the standard; this
    // emulator's job is the platform, FreeBSD answers a unique pointer, and the near-universal
    // caller idiom is `if (!p) fail`.
    //
    // So a zero request became an allocation failure. `ftpsrv` asked `sysctl` how many
    // processes there are, was told none, allocated nothing for the list, and reported
    // `main-prospero.c:49:malloc:` before giving up on everything after it - which read as a
    // privilege problem and was a one-line disagreement about zero (D383).
    //
    // One byte, so the pointer is unique, freeable, and carries a header like every other.
    let size = size.max(1);
    // At least the header, so the header always fits between `base` and `body`; and a
    // power of two, which every allocator interface requires of an alignment.
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
    // SAFETY: the layout has a non-zero size, which is `alloc`'s only requirement.
    let base = unsafe { std::alloc::alloc(layout) };
    if base.is_null() {
        // What a real allocator returns when it cannot allocate, and callers test for it.
        return 0;
    }
    // SAFETY: `align <= total`, so this stays inside the allocation.
    let body = unsafe { base.add(align) };
    // SAFETY: `align >= HEAP_HEADER`, so the header sits at or after `base` and entirely
    // before `body`.
    let header = unsafe { body.sub(HEAP_HEADER) };
    // SAFETY: the header is sixteen writable bytes, and this writes the first eight.
    // Unaligned by construction, so the pointer's alignment carries no requirement.
    unsafe { header.cast::<usize>().write_unaligned(total) };
    // SAFETY: `WORD` bytes into a sixteen-byte header, so the second word is in bounds.
    let second = unsafe { header.add(WORD) };
    // SAFETY: eight writable bytes, written unaligned.
    unsafe { second.cast::<usize>().write_unaligned(align) };
    if let Some(poison) = heap_fill() {
        // SAFETY: `size` bytes from `body` are the allocation just made, which nothing else
        // holds a reference to and the guest has not seen yet.
        unsafe { std::ptr::write_bytes(body, poison, size) };
        FILLED_ALLOCATIONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        FILLED_HEAP_BYTES.fetch_add(size as u64, std::sync::atomic::Ordering::Relaxed);
    }
    body as usize as u64
}

/// How many allocations this run filled, and how many bytes.
///
/// **Counted so the diagnostic can be shown to have run.** A poison that changed nothing
/// and a poison that never executed produce identical output, and the difference decides
/// whether an unchanged run is an elimination or is about nothing at all (D325).
static FILLED_ALLOCATIONS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// Bytes filled, alongside [`FILLED_ALLOCATIONS`].
static FILLED_HEAP_BYTES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// One line for a run report: what the heap fill actually did.
///
/// [`None`] when none was asked for, so a quiet run stays quiet. A run that asked and
/// reports nothing says so in those words, because that sentence is the one a reader must
/// never have to infer.
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
/// # The ambiguity this removes
///
/// The host allocator returns **uninitialised** memory, and on a page the process has not
/// used before that is almost always zero. So a guest reading a field nobody filled in and
/// a guest reading a deliberate zero are indistinguishable on the heap - which is exactly
/// the confusion the stack poison exists to remove, in the one region it cannot reach
/// (D185, D220).
///
/// Zero is not a fill. It is what the host does anyway, and rewriting every allocation to
/// no effect would make the instrumented run slower than the one it is compared against.
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
/// `None` for an address this library did not hand out, which is the same contract the
/// real `free` has - except that returning `None` lets the caller decline rather than
/// corrupt a heap it does not own.
fn header_of(body: u64) -> Option<(usize, usize)> {
    let body = usize::try_from(body).ok()?;
    let at = body.checked_sub(HEAP_HEADER)?;
    let header = std::ptr::with_exposed_provenance::<u8>(at);
    // SAFETY: reads the first word `allocate` wrote before the pointer it returned, in
    // the form it wrote it.
    let total = unsafe { header.cast::<usize>().read_unaligned() };
    // SAFETY: `WORD` bytes into the same sixteen-byte header.
    let second = unsafe { header.add(WORD) };
    // SAFETY: eight readable bytes, read in the form they were written.
    let offset = unsafe { second.cast::<usize>().read_unaligned() };
    // A block this library wrote always satisfies both. Refusing otherwise turns a wild
    // pointer into a no-op rather than a `dealloc` against a layout nobody allocated.
    if offset < HEAP_HEADER || !offset.is_power_of_two() || total <= offset {
        return None;
    }
    Some((total, offset))
}

/// `malloc(size)`.
///
/// **Its absence was the wall behind a title that had otherwise reached graphics
/// initialisation.** Unimplemented, it returned the placeholder error code; the guest
/// took that as its buffer and handed it to `memset`, which faithfully wrote there
/// (D128).
fn malloc(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Ok(size) = usize::try_from(args[0]) else {
        return 0;
    };
    allocate(size, HEAP_HEADER)
}

/// `memalign(alignment, size)`.
///
/// # Why this one mattered
///
/// Two Unity titles print `tlsf_create: Memory must be aligned to 8 bytes.` and then fail
/// to build their allocator. Unimplemented, this answered the placeholder error code, which
/// the guest took for an address - and an address that is not eight-aligned is exactly what
/// it then complained about (D190).
///
/// The guest asked for eight. Larger alignments cost nothing extra here because the
/// allocation is aligned by the layout rather than by over-allocating and rounding.
fn memalign(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (Ok(align), Ok(size)) = (usize::try_from(args[0]), usize::try_from(args[1])) else {
        return 0;
    };
    if !align.is_power_of_two() {
        // Every allocator interface requires a power of two, and rounding up on the
        // caller's behalf would hide a bug in the caller.
        return 0;
    }
    allocate(size, align)
}

/// `free(pointer)`.
///
/// Reads the size back out of the header `malloc` wrote.
fn free(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let pointer = args[0];
    if pointer == 0 {
        // Freeing null is defined and does nothing.
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
    // SAFETY: the layout is rebuilt from what `allocate` recorded, so it matches the one
    // `alloc` received - which is what `dealloc` requires.
    unsafe { std::alloc::dealloc(base, layout) };
    0
}

/// `calloc(count, size)`.
///
/// Zeroed on purpose and not by accident: callers rely on it, and the multiplication is
/// checked because `calloc` exists partly to catch that overflow.
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
/// Allocate, copy the smaller of the two sizes, free. Not the most efficient shape - a
/// real allocator grows in place where it can - but it is the one that is obviously
/// correct, and nothing here is allocation-bound.
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
        // The original must survive a failed realloc - freeing it here would lose the
        // caller's data on the one path where it still needs it.
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
/// # Why these matter, and why the space is ignored
///
/// An *mspace* is an independent heap arena; the platform's libc wraps dlmalloc's
/// `mspace_malloc`/`_free`/`_calloc`/`_realloc`, which take the arena as a leading argument
/// and are otherwise the ordinary allocator. PPSA21564 walled here: it called
/// `sceLibcMspaceMalloc(0x0, 0x48)`, the default stub answered the placeholder error code,
/// and the guest wrote through it as its 72-byte buffer - `write to 0x7fff0001`, orbistoun's
/// own `Unimplemented` placeholder read as an address. This is the `malloc` wall (D128) under
/// another name (D451).
///
/// Every mspace is served from the one host heap this crate already owns, so the arena handle
/// is not consulted. That is sound because a guest only ever touches mspace memory *through
/// this same family*: `allocate` writes a header `free` reads back, independent of any arena,
/// so an allocation and its release agree whether or not the handle is real. A guest that
/// created its own mspace over a specific region and then inspected which addresses came back
/// would notice - nothing observed does, and that is a refinement for when one does.
///
/// Reference: Doug Lea's `dlmalloc` mspace interface (public domain), whose
/// `mspace_malloc(msp, bytes)` shape the platform names carry. `sceLibcMspaceMalloc`'s two
/// arguments were confirmed by an argument dump (arg0 the space, arg1 the size); the siblings
/// follow the same published shape.
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
/// Registers a destructor for a C++ object with static storage duration. **The
/// most-called import in every title examined** - 1,218 calls in one, more than
/// everything else combined - because a program registers one of these for every global
/// object it constructs (D124).
///
/// Accepted and never run, for the same reason as `atexit`: nothing tears a guest down
/// yet, so there is no moment for it to fire. Zero means accepted; a non-zero answer
/// makes a C++ runtime believe registration failed, and the standard behaviour then is
/// to abort.
fn cxa_atexit(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    0
}

/// Byte within a guard variable that records whether initialisation has completed.
///
/// The Itanium C++ ABI - a published specification - puts the completion flag in the
/// first byte on this architecture. The remaining bytes are the implementation's to use.
const GUARD_DONE: u64 = 1;

/// `__cxa_guard_acquire(guard)`.
///
/// Asked before a function-local static is initialised. Returns non-zero to mean "not yet
/// initialised, go ahead"; zero to mean "already done, skip it".
///
/// **Getting this wrong is worse than not implementing it.** An unimplemented version
/// returns an error, which is non-zero, which reads as *go ahead* - so the guest
/// initialises. It then calls `release`, which did nothing, so the flag is never set, so
/// the next visit initialises again. Every static in the program re-runs its constructor
/// forever, which is a very good explanation for a startup that never finishes.
fn cxa_guard_acquire(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let guard = args[0];
    if guard == 0 {
        return 0;
    }
    // SAFETY: the guest supplied the guard address, exactly as the real call receives it.
    // The mapping is identity, so this reads the byte it named.
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
/// Initialisation threw. The flag stays clear so the next attempt tries again, which is
/// what the standard requires - a static whose constructor throws is not initialised.
fn cxa_guard_abort(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let guard = args[0];
    if guard != 0 {
        // SAFETY: guest-supplied guard address, one byte written.
        unsafe { *ptr(guard) = 0 };
    }
    0
}

/// Everything this crate implements, by symbol name.
/// Reads a NUL-terminated string the guest passed.
///
/// Bounded by the same reasoning as everything else here: an unterminated buffer would
/// otherwise walk until it faults, and the fault would look like a bug in string handling
/// rather than in whatever produced the buffer.
pub(crate) fn read_guest_path(address: u64) -> Option<String> {
    /// Longer than any path observed, and short enough to stay near its own page.
    const MAX_PATH: usize = 1024;

    let at = usize::try_from(address).ok()?;
    if at == 0 {
        return None;
    }
    let mut bytes = Vec::new();
    for offset in 0..MAX_PATH {
        // SAFETY: a guest-supplied string under the identity mapping (D014), read one
        // byte at a time so the scan cannot straddle the end of a mapping by more than
        // it reads.
        let byte = unsafe { std::ptr::read(std::ptr::with_exposed_provenance::<u8>(at + offset)) };
        if byte == 0 {
            break;
        }
        bytes.push(byte);
    }
    String::from_utf8(bytes).ok()
}

/// Why a formatted write could not be honoured.
///
/// **Enumerated rather than collapsed into "failed", because the two need opposite
/// responses.** One is a function this could support and does not; the other cannot be
/// supported at this layer at all, and knowing which is the difference between an hour's
/// work and a change to the calling convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatFault {
    /// A conversion this does not implement.
    Unsupported(char),
    /// A floating-point conversion.
    ///
    /// **Not the same as unsupported, and not fixable here.** Under System V a variadic
    /// floating-point argument arrives in an *XMM* register, and the trampoline captures
    /// the six integer registers only. There is nothing to read: the value never reached
    /// this function. Implementing the conversion would produce a confident number derived
    /// from an unrelated register (D183).
    FloatingPoint(char),
    /// The format called for more arguments than the trampoline captured.
    ///
    /// Six integer registers arrive, three of which `snprintf_s` spends on its own fixed
    /// parameters, so a format with four or more conversions runs off the end. The rest
    /// were passed on the stack, which is reachable but not from the argument array alone.
    OutOfArguments,
}

/// What formatted writes could not do, across a run.
///
/// Counted rather than logged. A line per call would be unreadable at this volume, and the
/// only questions worth answering are *how often* and *which conversion* - both of which a
/// counter answers and a log buries (principle 9).
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
/// # Why this is not the readable-range check
///
/// It was, for an hour. The ranges a run publishes are the **guest's** - its image and its
/// stack - and a `%s` argument is very often a pointer into memory *this project* handed the
/// guest: a `strerror` buffer, a `getifaddrs` block, an allocation from the guest heap. Those
/// are outside every published range, so the check refused them, and `ftpsrv` printed a
/// perfectly good error message with `(unmapped)` where the reason should have been (D382).
///
/// So it is back to the narrow rule, which is the one that was actually needed: null, the
/// null page, and all-ones are not addresses any program computed. Everything else is
/// followed and faults as the machine would - which is the rule everywhere else here, and
/// right for a pointer the guest really did compute.
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
/// **Two sources, one renderer.** `printf` and its relatives are handed whatever the
/// trampoline caught in registers; the `v` forms are handed a `va_list`, which is a cursor
/// over the registers the *guest* spilled and the stack beyond them. Rendering from either
/// through one interface is what stops the two drifting - and it is what lets a `va_list`
/// honour a format the register forms have to refuse (D364).
trait Arguments {
    /// The next integer-class argument, or [`None`] when there is not one.
    fn next_integer(&mut self) -> Option<u64>;
}

/// The arguments a trampoline caught in registers, and then the guest's stack.
///
/// # Why the stack half exists
///
/// System V passes the first six integer arguments in registers and the rest on the stack.
/// `printf` spends one register on the format and `snprintf` spends three on the buffer, the
/// size and the format - so a format with more than three conversions had nothing left to
/// render them from, and the renderer stopped at the first one it could not fill.
///
/// It stopped **quietly**, because stopping is what a correct renderer does when the
/// arguments run out. `zftpd` answered `227` with the passive-mode address missing and `257`
/// with the path missing, and both looked like a server bug (D385).
struct Registers<'a> {
    /// The captured values, in order.
    values: &'a [u64],
    /// How many have been taken.
    taken: usize,
}

/// How many stack arguments one call may be asked for.
///
/// **A ceiling rather than a promise.** Nothing says how many the guest actually passed - the
/// format string is the only claim about that, exactly as in a real `printf` - so this bounds
/// how far a wrong format can walk before it stops. Sixty-four words is more than any format
/// measured and less than a page.
const MOST_STACK_ARGUMENTS: usize = 64;

impl Arguments for Registers<'_> {
    fn next_integer(&mut self) -> Option<u64> {
        if let Some(value) = self.values.get(self.taken).copied() {
            self.taken += 1;
            return Some(value);
        }
        // Past the registers, so the rest is where the psABI puts it: on the guest's stack,
        // above the return address, in order.
        let index = self.taken - self.values.len();
        if index >= MOST_STACK_ARGUMENTS {
            return None;
        }
        let area = orbistoun_thunk::stack_arguments();
        if area == 0 {
            // Not inside a guest call - every test, and every path that renders for this
            // project's own reporting. There is no stack to read and saying so is right.
            return None;
        }
        let at = usize::try_from(area.saturating_add((index as u64).saturating_mul(8))).ok()?;
        self.taken += 1;
        // SAFETY: the guest's own stack under the identity mapping (D014), inside the frame
        // of the call currently running - which the thunk published and has not returned
        // from. Read unaligned because a stack argument is eight-aligned by the convention
        // and nothing here depends on that being true.
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
/// **Flags, width and precision are consumed whether or not they are honoured**, so a
/// specifier this cannot render is still delimited correctly and the *conversion* is what
/// gets reported - not whatever character the scan happened to stop on.
fn read_specifier(chars: &mut std::iter::Peekable<impl Iterator<Item = u8>>) -> Specifier {
    let mut found = Specifier {
        left: false,
        zero: false,
        width: 0,
        precision: None,
        // **The default is `int`, as C says**, and a modifier widens or narrows it.
        //
        // This used to be the whole register, on the reasoning that every integer argument
        // arrives as one and the conversion says how much of it counts. That was true only
        // while every argument *was* a register: a caller storing an `int` writes `edi`,
        // which zeroes the upper half of `rdi`, so reading sixty-four bits was right by
        // accident.
        //
        // An argument on the stack sits in an eight-byte slot whose **upper half is
        // unspecified** for anything narrower. The moment stack arguments were read, `zftpd`
        // logged `RES=-4294967296` - a zero with somebody else's bits above it (D385).
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
            // `l` is long and `ll` is long long; `size_t`, `intmax_t` and `ptrdiff_t` are
            // all the same width on this data model, so they are one arm.
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

/// The low `bits` of a value, which is what a conversion of that width actually received.
///
/// Sixty-four is the whole word and needs no mask - and would shift by the width of the type,
/// which is undefined.
const fn narrow(value: u64, bits: u32) -> u64 {
    if bits >= 64 {
        return value;
    }
    value & ((1_u64 << bits) - 1)
}

/// The same value read as signed at that width.
///
/// `0xFFFF_FFFF` is `-1` as an `int` and `4294967295` as an `unsigned int`, and the
/// conversion is the only thing that says which - so the width has to be applied here rather
/// than by casting the whole register.
const fn sign_extend(value: u64, bits: u32) -> i64 {
    if bits >= 64 {
        return value as i64;
    }
    let shift = 64 - bits;
    ((value << shift) as i64) >> shift
}

/// Renders a format string against the arguments that arrived in registers.
///
/// The register-limited entry point, kept because most callers here are register-based and
/// every test is. [`render_with`] is the same renderer over any argument source.
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
/// # Why this refuses rather than doing its best
///
/// A partially rendered string is **invented data wearing the shape of a real answer**. A
/// guest that receives `"texture_"` where it expected `"texture_47.gnf"` opens the wrong
/// file, and the failure surfaces somewhere with no connection to formatting. Refusing
/// produces an empty string, which is also wrong - but wrong in a way that is *bounded*
/// and shows up immediately (principle 3).
///
/// Returns what the whole rendering would be, ignoring any destination limit, because that
/// is what the interface reports and truncation is applied by the caller.
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
            // A format ending in a bare `%` is malformed; treated as a fault rather than
            // silently dropped, because the guest built this string and got it wrong.
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
                    // What every implementation of note does with a null, and a guest
                    // relying on it would otherwise fault inside formatting.
                    b"(null)".to_vec()
                } else if !followable(value) {
                    // **An address no program computed.** Not `(null)`, because it is not
                    // null: it is what a register nothing set arrives holding, and following
                    // it crashes inside the renderer rather than reporting anything.
                    b"(bad pointer)".to_vec()
                } else {
                    // SAFETY: a guest-supplied string under the identity mapping (D014),
                    // bounded by the same limit every string function here uses.
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

        // Padding, applied after rendering so it is the same code for every conversion.
        let pad = width.saturating_sub(rendered.len());
        let fill = if zero && !left { b'0' } else { b' ' };
        if left {
            out.extend_from_slice(&rendered);
            out.extend(std::iter::repeat_n(b' ', pad));
        } else {
            out.extend(std::iter::repeat_n(fill, pad));
            out.extend_from_slice(&rendered);
        }
    }
    Ok(out)
}

/// `snprintf_s(dest, size, format, ...)`.
///
/// The C11 Annex K bounds-checked variant - found by hash after `snprintf`, `sprintf` and
/// `vsnprintf` all missed, and the most-called unnamed import in a boot at seventy-six
/// calls. It sits between `sceKernelAllocateMainDirectMemory` and
/// `sceKernelMapNamedDirectMemory` with a stack address as its destination: the guest is
/// building the *name* that the mapping call takes.
///
/// # What it will not do
///
/// Three fixed parameters leave three integer registers for variadic arguments, and a
/// floating-point one never arrives in an integer register at all. Rather than render what
/// it can and invent the rest, a format it cannot honour completely produces an **empty,
/// terminated** destination and a zero return - and is counted, so the run can say how
/// often it happened and which conversion was responsible (D183).
///
/// Annex K is optional and implementations differ, so what the target library really
/// returns on truncation is unverified; the knowledge file records that.
fn snprintf_s(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    use std::sync::atomic::Ordering::Relaxed;

    let (dest, size, format) = (args[0], args[1] as usize, args[2]);
    FORMAT_CALLS.fetch_add(1, Relaxed);

    if dest == 0 || size == 0 {
        return 0;
    }
    if format == 0 {
        note_fault(FormatFault::Unsupported('\0'));
        // SAFETY: `dest` is non-null with at least one byte, per the size the guest passed.
        unsafe { std::ptr::write(ptr(dest), 0) };
        return 0;
    }

    // SAFETY: a guest-supplied string under the identity mapping (D014), bounded.
    let len = unsafe { c_len(format) };
    // SAFETY: `c_len` established `len` readable bytes from `format`.
    let template = unsafe { std::slice::from_raw_parts(ptr(format).cast_const(), len) };

    let rendered = match render_format(template, &args[3..]) {
        Ok(text) => text,
        Err(fault) => {
            note_fault(fault);
            // SAFETY: `dest` is non-null with at least one byte.
            unsafe { std::ptr::write(ptr(dest), 0) };
            return 0;
        }
    };

    // One byte reserved for the terminator, which is what makes this the bounded variant.
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

    // The full length, as the interface reports - a caller detects truncation by comparing
    // it against the size it passed, and reporting the copied length would hide that.
    rendered.len() as u64
}

/// `sprintf(dest, format, ...)` - unbounded, and that is the whole hazard.
///
/// No size argument means nothing here can stop an overrun: the guest promised the buffer
/// is large enough and this has no way to check. Implemented anyway, because refusing it
/// does not make the guest safer - it makes the guest fail without saying why.
fn sprintf(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    use std::sync::atomic::Ordering::Relaxed;

    let (dest, format) = (args[0], args[1]);
    FORMAT_CALLS.fetch_add(1, Relaxed);
    if dest == 0 || format == 0 {
        return 0;
    }
    // SAFETY: a guest-supplied string under the identity mapping (D014), bounded.
    let len = unsafe { c_len(format) };
    // SAFETY: `c_len` established `len` readable bytes from `format`.
    let template = unsafe { std::slice::from_raw_parts(ptr(format).cast_const(), len) };
    let Ok(rendered) = render_format(template, &args[2..]) else {
        // SAFETY: `dest` is non-null, and a caller promised a buffer.
        unsafe { std::ptr::write(ptr(dest), 0) };
        return 0;
    };
    // SAFETY: the guest promised a buffer large enough. That promise is the interface, and
    // there is nothing here that can verify it.
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
/// The caller frees it, so it must come from the allocator `free` understands - which is
/// why this goes through [`allocate`] rather than anything simpler.
fn strdup(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // SAFETY: a guest-supplied string under the identity mapping (D014), bounded.
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
/// **`n` bounds the source, not the result.** A caller passing the size of `dest` writes
/// past the end of it; the standard defines it this way and matching it is the job.
fn strncat(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // SAFETY: a guest-supplied string under the identity mapping (D014), bounded.
    let at = unsafe { c_len(args[0]) };
    // SAFETY: the other guest-supplied string, same contract.
    let from = unsafe { c_len(args[1]) };
    let take = from.min(usize::try_from(args[2]).unwrap_or(usize::MAX));
    if take > 0 {
        // SAFETY: the guest promised `dest` has room after its own length, which is the
        // interface's contract.
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
/// **A static, which is what the interface specifies and why `strtok_r` exists.** Recorded
/// rather than hidden: a second thread calling this mid-walk gets the first one's position,
/// exactly as it would on the target.
static STRTOK_STATE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Finds the next token from `start`, terminating it in place, and where to resume.
///
/// **Writes a NUL into the guest's own buffer**, which is what `strtok` does and why it
/// cannot be used on a string literal. Returning a copy would be a different function.
fn next_token(start: u64, delimiters: u64) -> (u64, u64) {
    if start == 0 {
        return (0, 0);
    }
    // SAFETY: a guest-supplied string under the identity mapping (D014), bounded.
    let len = unsafe { c_len(start) };
    // SAFETY: the delimiter set, same contract.
    let delim_len = unsafe { c_len(delimiters) };
    // SAFETY: both lengths were established by scanning to their terminators.
    let text = unsafe { std::slice::from_raw_parts(ptr(start).cast_const(), len) };
    // SAFETY: the same, for the delimiter set.
    let delims = unsafe { std::slice::from_raw_parts(ptr(delimiters).cast_const(), delim_len) };
    let Some(begin) = text.iter().position(|b| !delims.contains(b)) else {
        // Nothing but delimiters left: the walk is over, and a null resume point makes the
        // next call end too rather than restart.
        return (0, 0);
    };
    let end = text[begin..]
        .iter()
        .position(|b| delims.contains(b))
        .map_or(len, |at| begin + at);
    if end < len {
        // SAFETY: `end` is inside the guest's own writable string - a caller that passed a
        // literal has already broken the interface's contract.
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
        // SAFETY: a guest-supplied `char **` under the identity mapping (D014).
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

// --- calling back into the guest ------------------------------------------------------

/// A guest comparison function: `int (*)(const void *, const void *)`.
///
/// # Why this is the first time this crate calls the guest
///
/// Everything else here answers a call. `qsort` and `bsearch` *make* one: the caller hands
/// over a function pointer into its own code and expects it to be used. That is a genuine
/// capability rather than a missing function, which is why these two were the last checks
/// left in their section long after the rest of the library was working (D274).
///
/// `extern "sysv64"` so the compiler emits the guest's own convention. The thunk dispatch
/// is re-entrant, so a comparator that itself calls an import lands back here as an
/// ordinary nested call.
type GuestComparator = extern "sysv64" fn(u64, u64) -> u64;

/// Turns a guest address into something callable.
///
/// # Safety
///
/// `address` must be a function in the guest's own fully relocated image, taking two
/// pointers under System V - which is exactly what the caller promised by passing it as a
/// comparator. The same contract the real call has.
unsafe fn comparator(address: u64) -> GuestComparator {
    // SAFETY: the caller guarantees a guest function of this shape.
    unsafe { std::mem::transmute::<u64, GuestComparator>(address) }
}

/// The sign of a comparison, as C reports it.
fn compare_at(f: GuestComparator, a: u64, b: u64) -> std::cmp::Ordering {
    // The guest answers an `int`; anything with the top bit of the low word set is
    // negative, and a whole-word test would read every negative result as positive.
    let raw = f(a, b) as u32 as i32;
    raw.cmp(&0)
}

/// `qsort(base, count, size, compare)`.
///
/// **Sorts an index permutation, then applies it.** The comparator is handed addresses in
/// the guest's own array, which is what it expects - but the array is not moved underneath
/// it while comparisons are running, so a comparator that reads its arguments twice sees
/// the same bytes both times. The real implementation makes no such promise; this one is
/// stricter, not looser.
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

    // Copied out before anything is written back, because applying a permutation in place
    // needs the original and the array *is* the original.
    let total = count * size;
    // SAFETY: the guest described `count` elements of `size` bytes at `base`, which is the
    // interface's contract and the only description of the buffer there is.
    let original = unsafe { std::slice::from_raw_parts(ptr(base).cast_const(), total) }.to_vec();
    for (to, from) in order.iter().enumerate() {
        let src = &original[from * size..from * size + size];
        // SAFETY: `to` is below `count`, so this lands inside the same described buffer.
        let dest = unsafe { ptr(base).add(to * size) };
        // SAFETY: `size` bytes from the copy taken before anything was written back, into
        // the slot just bounded.
        unsafe {
            std::ptr::copy_nonoverlapping(src.as_ptr(), dest, size);
        }
    }
    0
}

/// `bsearch(key, base, count, size, compare)` - the matching element, or null.
///
/// **Answers a pointer, so a miss is null and never an error code.** A count-shaped
/// placeholder here would be a wild pointer the guest dereferences immediately, which is
/// the shape this project keeps finding (D125, D273).
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
/// # Why this is worth implementing before anything it prints matters
///
/// A guest that gives up almost always says why first, and this is how it says it. Two
/// titles abort during static initialisation after calling this eight times; until it is
/// implemented the emulator discards the guest's own explanation and then reports that the
/// guest stopped for reasons unknown (D186).
///
/// Output goes to the host's **error** stream, not its output stream, for the same reason
/// the guest's own descriptors 1 and 2 both do: a worker's stdout carries the JSON protocol
/// its parent is parsing, and a guest printing into it corrupts the run.
///
/// Refuses the same formats [`render_format`] refuses, and for the same reason - a
/// half-rendered diagnostic is worse than none, because it is the text somebody will then
/// reason from.
fn printf(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    use std::io::Write as _;
    use std::sync::atomic::Ordering::Relaxed;

    let format = args[0];
    FORMAT_CALLS.fetch_add(1, Relaxed);
    if format == 0 {
        note_fault(FormatFault::Unsupported('\0'));
        return 0;
    }

    // SAFETY: a guest-supplied string under the identity mapping (D014), bounded.
    let len = unsafe { c_len(format) };
    // SAFETY: `c_len` established `len` readable bytes from `format`.
    let template = unsafe { std::slice::from_raw_parts(ptr(format).cast_const(), len) };

    let rendered = match render_format(template, &args[1..]) {
        Ok(text) => text,
        Err(fault) => {
            // Recorded rather than printed. Whatever the guest meant to say, this is not
            // it, and a mangled diagnostic is the text somebody would then reason from.
            note_fault(fault);
            return 0;
        }
    };

    let mut err = std::io::stderr();
    let _ = err.write_all(&rendered);
    let _ = err.flush();
    rendered.len() as u64
}

/// `strerror(errnum)` - a pointer to a message describing an error number.
///
/// # Why the message is not FreeBSD's
///
/// Nothing here has measured what this platform's C library returns for a given number,
/// and the strings are not derivable from anything lawful in this repository. So the text
/// **says what it is** rather than imitating a table it has not seen. A guest printing it
/// gets something true and obviously ours; a guest comparing it against a known string was
/// never going to work either way.
///
/// What matters far more is that it is a **real, readable pointer**. `strerror` feeding a
/// placeholder into `printf` is what faulted both payloads measured (D344): the caller
/// does not check it, it prints it.
///
/// # Per thread
///
/// The standard permits a static buffer that a later call overwrites, and every real
/// implementation uses one. Per thread rather than per process so two guest threads
/// reporting different failures do not overwrite each other's text mid-print.
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
        // SAFETY: `at` points at a `ROOM`-byte array owned by this thread and borrowed by
        // nothing else here, and `room` is capped below `ROOM`.
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
/// **The stream is read and ignored, deliberately.** A guest's `stderr` is a `FILE *` it
/// imported as data, and this layer gives it zeroed storage rather than a real stream
/// (D323) - so there is nothing behind the handle to distinguish. Everything goes to the
/// host's error stream, which is where `printf` already sends it and why a worker's stdout
/// stays free for the protocol its parent parses.
///
/// Worth stating because it means `fprintf(stdout, ...)` and `fprintf(stderr, ...)` are not
/// currently told apart. A guest relying on that distinction would be misread.
fn fprintf(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    use std::sync::atomic::Ordering::Relaxed;

    // A stream that is a descriptor goes to the descriptor. **This is the whole reason
    // `fdopen` exists**: a server accepts a connection, wraps it, and writes its replies with
    // `fprintf` - and a reply that went to the host's error stream instead would look like a
    // working server nobody could talk to.
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
    // SAFETY: a guest-supplied string under the identity mapping (D014), bounded.
    let len = unsafe { c_len(format) };
    // SAFETY: `c_len` established `len` readable bytes from `format`.
    let template = unsafe { std::slice::from_raw_parts(ptr(format).cast_const(), len) };
    let rendered = match render_format(template, &args[2..]) {
        Ok(text) => text,
        Err(fault) => {
            note_fault(fault);
            return 0;
        }
    };
    orbistoun_fs::descriptor::write(fd, &rendered).map_or(0, |written| written as u64)
}

/// Renders a guest's format string against a guest's `va_list`.
///
/// The shared half of the three `v` forms below, which differ only in where the bytes go
/// afterwards. Answers [`None`] when the format could not be honoured completely, having
/// already recorded why - so every caller's failure path is "write nothing".
fn render_va(format: u64, ap: u64) -> Option<Vec<u8>> {
    use std::sync::atomic::Ordering::Relaxed;

    FORMAT_CALLS.fetch_add(1, Relaxed);
    if format == 0 {
        note_fault(FormatFault::Unsupported('\0'));
        return None;
    }
    // SAFETY: a guest-supplied `va_list` under the identity mapping (D014). A null one is
    // answered without being dereferenced.
    let Some(mut list) = (unsafe { varargs::VaList::read(ap) }) else {
        // A null list with a format that wants arguments is exactly out-of-arguments, and
        // reporting it as that puts it in the same counter as its register-form twin.
        note_fault(FormatFault::OutOfArguments);
        return None;
    };
    // SAFETY: a guest-supplied string under the identity mapping (D014), bounded.
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

/// `vsnprintf(dest, size, format, ap)` - the most wanted missing name in the payload
/// library.
///
/// Twenty-two of the twenty-five open-toolchain payloads measured import this, more than
/// any other name nothing here implements. Every one of their logging helpers is built out
/// of it, so a payload without it cannot say why it stopped - which is the whole reason
/// this is worth doing before anything it prints matters (D364).
///
/// # A size of zero is a real call, not a mistake
///
/// `vsnprintf(NULL, 0, format, ap)` is the standard way to ask **how long the answer would
/// be** before allocating for it, and callers do exactly that. It writes nothing and
/// answers the full length, which is why the return value here is the rendered length
/// rather than the copied one in every case.
///
/// Reference: ISO C `vsnprintf`; POSIX.1-2008 `vsnprintf(3)`.
fn vsnprintf(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    use std::sync::atomic::Ordering::Relaxed;

    let (dest, size, format, ap) = (args[0], args[1] as usize, args[2], args[3]);
    // A destination that cannot be one is not written to, for the same reason the format and
    // the list are checked: all-ones is what a register nothing set arrives holding (D378).
    let dest = if dest == u64::MAX { 0 } else { dest };
    let Some(rendered) = render_va(format, ap) else {
        // Terminated where there is somewhere to terminate, so a caller that prints the
        // destination regardless prints nothing rather than whatever was there.
        if dest != 0 && size > 0 {
            // SAFETY: `dest` is non-null with at least one byte, per the size the guest
            // passed.
            unsafe { std::ptr::write(ptr(dest), 0) };
        }
        return 0;
    };

    if dest != 0 && size > 0 {
        // One byte reserved for the terminator, which is what makes this the bounded form.
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

    // The length the whole rendering would have been, which is what the interface reports
    // and what the measure-then-allocate idiom depends on.
    rendered.len() as u64
}

/// `vprintf(format, ap)` - the `va_list` form of [`printf`].
///
/// Output goes to the host's **error** stream for the same reason every other write here
/// does: a worker's standard output carries the protocol its parent is parsing.
///
/// Reference: ISO C `vprintf`; POSIX.1-2008 `vprintf(3)`.
fn vprintf(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    use std::io::Write as _;

    let Some(rendered) = render_va(args[0], args[1]) else {
        return 0;
    };
    let mut err = std::io::stderr();
    let _ = err.write_all(&rendered);
    let _ = err.flush();
    rendered.len() as u64
}

/// `vfprintf(stream, format, ap)` - [`vprintf`] with a stream in front.
///
/// **The stream is not honoured**, exactly as in [`fprintf`] and for the same stated
/// reason: both of a guest's standard streams already land on the host's error stream, so
/// `stdout` and `stderr` are not currently told apart here. A guest relying on that
/// distinction would be misread.
///
/// Reference: ISO C `vfprintf`; POSIX.1-2008 `vfprintf(3)`.
fn vfprintf(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let mut shifted = [0_u64; GUEST_ARG_REGISTERS];
    shifted[..GUEST_ARG_REGISTERS - 1].copy_from_slice(&args[1..]);
    vprintf(&shifted)
}

/// `puts(s)` - writes a string and a newline.
///
/// **Not `printf` with the same argument.** `puts` appends a newline and does *not* treat
/// its argument as a format, so routing it through the renderer would make a guest's own
/// text containing a percent sign either vanish or be reported as a bad conversion. It is
/// the one difference that matters and it is the whole implementation.
///
/// Called eight times before `klogsrv` reaches anything else, which is a program telling
/// you what it is doing and being ignored (D344).
///
/// Reference: POSIX.1-2008 `puts(3)`. Answers a non-negative number on success; the byte
/// count is a permitted choice and is more informative than a bare zero.
fn puts(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    use std::io::Write as _;

    let text = args[0];
    if text == 0 {
        return 0;
    }
    // SAFETY: a guest-supplied string under the identity mapping (D014), bounded.
    let len = unsafe { c_len(text) };
    // SAFETY: `c_len` established `len` readable bytes from `text`.
    let bytes = unsafe { std::slice::from_raw_parts(ptr(text).cast_const(), len) };

    let mut err = std::io::stderr();
    let _ = err.write_all(bytes);
    let _ = err.write_all(b"\n");
    let _ = err.flush();
    len as u64 + 1
}

/// `getpid()` - the process the guest is running in.
///
/// The host process id, which is **true rather than invented**: the guest runs inside this
/// process, so this is its process id in every sense that can be checked from inside it.
/// A made-up constant would be indistinguishable until something compared it against
/// something else.
///
/// Reference: POSIX.1-2008 `getpid(3)`.
fn getpid(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    u64::from(std::process::id())
}

/// `kill(pid, sig)` - refuses to signal anything, and says why.
///
/// # Why this is not "send the signal"
///
/// The guest runs **inside this process**, so its process id is the host's (as `getpid`
/// records). A real `kill` here would signal the emulator - which is not what the guest
/// meant, and would end the run in a way that looked like the guest's own doing.
///
/// # The one case that is answerable
///
/// Signal zero sends nothing: it is the documented way to ask *does this process exist and
/// may I signal it*. That question has a true answer here - the guest's own process is the
/// only one it can name - so it is answered, and every other process id reports failure.
///
/// Any real signal is refused. Nothing here delivers signals at all, which `signal` already
/// records: a handler is remembered and never invoked. Reporting success would tell a guest
/// it had terminated something that is still running, and a payload that kills a previous
/// instance of itself and then binds its port would be told the port was free and find it was
/// not.
///
/// Reference: POSIX.1-2008 `kill(2)`. Signal zero's meaning is that page's own.
fn kill(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (pid, signal) = (args[0] as i64, args[1]);
    if signal != 0 {
        return FAILED;
    }
    // A guest asking about itself gets a true yes. `pid` zero and negative values name
    // process groups, which this has no notion of and will not pretend to.
    if pid > 0 && u64::try_from(pid) == Ok(u64::from(std::process::id())) {
        OK
    } else {
        FAILED
    }
}

/// `getenv(name)` - a variable out of the environment the guest was actually given.
///
/// # Why this answers null so often, and why that is right
///
/// The environment a run starts with is **empty by default**, and deliberately: nothing here
/// knows what the platform sets, and inventing plausible variables is how a guest ends up
/// taking a path nobody chose for it. What a run *does* set is in `config.toml`, under
/// `entry.environment`, so a guest that needs one can be given it and the giving is a
/// deliberate act with a diff.
///
/// So this reads the same strings the process image was built from, and answers null for
/// anything else - which is what `getenv` answers for a variable that is not set, and what
/// every caller already handles.
///
/// Reference: POSIX.1-2008 `getenv(3)`.
fn getenv(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if args[0] == 0 {
        return 0;
    }
    // SAFETY: a guest-supplied string under the identity mapping (D014), bounded.
    let len = unsafe { c_len(args[0]) };
    // SAFETY: `c_len` established `len` readable bytes.
    let wanted = unsafe { std::slice::from_raw_parts(ptr(args[0]).cast_const(), len) };
    let Ok(wanted) = std::str::from_utf8(wanted) else {
        return 0;
    };
    environment_value(wanted).map_or(0, |value| value as usize as u64)
}

/// The value of a variable in the guest's environment, as a stable address.
///
/// Cached per name, because `getenv` answers a pointer the caller may keep: handing out a
/// fresh allocation per call would be a leak, and a temporary would be a dangling pointer
/// the moment the caller looked at it.
fn environment_value(name: &str) -> Option<*const u8> {
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    static ANSWERED: Mutex<Option<BTreeMap<String, &'static [u8]>>> = Mutex::new(None);

    let mut guard = ANSWERED.lock().ok()?;
    let answered = guard.get_or_insert_with(BTreeMap::new);
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

/// `getcwd(buffer, size)` - where the guest thinks it is.
///
/// **The root, because there is no working directory here.** Nothing in this emulator tracks
/// one: paths are resolved against the mount table, which is absolute, and a guest that
/// changed directory would be changing something nothing reads. Answering the root is true in
/// the only sense available - every path a guest can name is absolute - and answering a
/// plausible-looking title directory would be inventing a fact.
///
/// Reference: POSIX.1-2008 `getcwd(3)`. Answers the buffer on success and null on failure,
/// and a buffer too small is the documented failure rather than a truncation.
fn getcwd(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (buffer, size) = (args[0], args[1]);
    let text = b"/\0";
    if buffer == 0 || size < text.len() as u64 {
        return 0;
    }
    // SAFETY: a guest-supplied buffer under the identity mapping (D014), with at least the
    // two bytes just checked against the size the guest passed.
    unsafe { std::ptr::copy_nonoverlapping(text.as_ptr(), ptr(buffer), text.len()) };
    buffer
}

/// `realpath(path, resolved)` - a path with the `.`, the `..` and the doubled slashes gone.
///
/// # Why a server calls it before it will do anything
///
/// It is how a program that serves files **decides whether a path is real** before acting on
/// it. `zftpd` calls it on every path a client names: with nothing answering, the placeholder
/// came back and `CWD /` was refused as `550 Invalid path.` - after the root had been given a
/// listing and was a perfectly good directory (D385).
///
/// # What "resolved" means here, which is not what it means on a host
///
/// The answer is a **guest** path, not the host path it maps to. A guest asked about `/app0`
/// and a guest is what it gets back: handing it `C:\titles\PPSA00000` would be a true fact
/// about this machine and a lie about the platform, and the guest would hand it straight back
/// to `open`.
///
/// So the components are walked here rather than by [`std::fs::canonicalize`]: `.` is
/// dropped, `..` pops the one before it, empty components collapse, and what is left is
/// rebuilt with single slashes. **`..` above the root stays at the root**, which is what
/// every filesystem does and what stops a path climbing out of the mount table by spelling.
///
/// A relative path is taken from the root, because there is no working directory here - the
/// same fact [`getcwd`] reports, stated the same way rather than differently.
///
/// # Failing is part of the interface
///
/// POSIX requires **every component to exist**, so a path this cannot resolve answers null
/// rather than the tidied-up text. A caller uses the difference to decide whether to serve
/// the path at all, and answering the spelling of a path that is not there would tell it yes.
///
/// Reference: POSIX.1-2008 `realpath(3)`; `PATH_MAX` from `sys/sys/syslimits.h`.
fn realpath(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    /// What the call answers when it cannot resolve the path.
    const FAILED_POINTER: u64 = 0;

    let (path, resolved) = (args[0], args[1]);
    if path == 0 {
        return FAILED_POINTER;
    }
    // SAFETY: a guest-supplied string under the identity mapping (D014), bounded.
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
        // Longer than a caller's buffer can be. Refused rather than truncated: half a path
        // is a different path, and the caller sized its buffer by this number.
        return FAILED_POINTER;
    }

    // **A null second argument means "allocate one"**, which is the form a caller uses when
    // it does not want to size a buffer itself. The block comes from this library's own heap,
    // so the guest's `free` gives it back.
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
    // SAFETY: either a block just allocated at exactly this length, or a guest-supplied
    // buffer the interface requires to hold `PATH_MAX` bytes - and the length was checked
    // against that above.
    unsafe {
        std::ptr::copy_nonoverlapping(
            text.as_ptr(),
            std::ptr::with_exposed_provenance_mut::<u8>(at),
            text.len(),
        );
    }
    destination
}

/// The canonical spelling of a guest path, or nothing when it does not exist.
///
/// Pure, so the walk is testable without a mount table - and the walk is the part with the
/// edges: `..` at the root, a trailing slash, an empty path, a doubled separator.
fn canonical_components(asked: &str) -> String {
    // Normalised into a local first: a guest that mixes separators must not be able to slip
    // a component past the walk below, and the walk borrows from what it splits.
    let normalised = asked.replace('\\', "/");
    let mut kept: Vec<&str> = Vec::new();
    for component in normalised.split('/') {
        match component {
            // An empty component is a doubled slash or a leading one; `.` is this directory.
            "" | "." => {}
            ".." => {
                // At the root this is the root, which is what every filesystem does - and
                // what stops a path leaving the mount table by spelling alone.
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
/// Reads `errno` and renders it through the same message [`strerror`] gives, so a guest
/// gets one story about a failure however it asks. A null or empty prefix prints the message
/// alone, which is what the standard says.
///
/// Reference: POSIX.1-2008 `perror(3)`.
fn perror(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    use std::io::Write as _;

    let mut line = Vec::new();
    if args[0] != 0 {
        // SAFETY: a guest-supplied string under the identity mapping (D014), bounded.
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

    let mut err = std::io::stderr();
    let _ = err.write_all(&line);
    let _ = err.flush();
    0
}

/// `strerror_r(errnum, buffer, size)` - `strerror` into a caller's own buffer.
///
/// The thread-safe spelling, and the one a server uses. Answers zero on success and the
/// documented `ERANGE`-shaped failure is instead a **truncation refused**: a buffer too small
/// gets nothing rather than half a message, for the same reason every other bounded write
/// here refuses.
///
/// Reference: POSIX.1-2008 `strerror_r(3)`.
fn strerror_r(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (number, buffer, size) = (args[0] as i32, args[1], args[2]);
    let text = format!("error {number} (orbistoun has no message table)\0");
    if buffer == 0 || size < text.len() as u64 {
        return FAILED;
    }
    // SAFETY: a guest-supplied buffer under the identity mapping (D014), with at least
    // `text.len()` bytes as just checked against the size the guest passed.
    unsafe { std::ptr::copy_nonoverlapping(text.as_ptr(), ptr(buffer), text.len()) };
    OK
}

/// `sysctl(name, namelen, oldp, oldlenp, newp, newlen)` - refuses what it does not know,
/// and says what was asked.
///
/// # Why refusing is the implementation
///
/// Nothing here knows what any particular MIB means. FreeBSD's `sysctl(3)` documents
/// `ENOENT` for exactly this - *"The name array specifies a value that is unknown"* - and a
/// documented failure is a real answer: a caller checks the return value and takes its own
/// error path, which is what `klogsrv` does, reporting the file and line itself.
///
/// Answering **success** would be far worse. `oldp` is often null on the first of a pair of
/// calls asking only how large the answer is, and reporting success without writing a
/// length hands the caller an uninitialised size it then allocates against.
///
/// # errno is deliberately not set
///
/// `ENOENT`'s numeric value is not derivable from anything lawful in this repository -
/// FreeBSD's `errno.h` is not in the local checkout - so it is left alone rather than
/// guessed at. The return value is what a caller branches on; the number only shapes a
/// message. Setting an invented one would put a wrong constant somewhere it would be
/// copied from later.
///
/// # What it says instead
///
/// Every distinct MIB asked for is reported once, because an unknown one is a work item
/// and the guest is the only thing that knows which are wanted. `klogsrv` asks for exactly
/// one (D349).
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
        // SAFETY: a guest-supplied array of `namelen` 32-bit words under the identity
        // mapping (D014); a length the guest declared, read within it.
        let word = unsafe { std::ptr::read(std::ptr::with_exposed_provenance::<u32>(at)) };
        mib.push(word);
    }

    if is_process_listing(&mib) {
        return no_processes(args[3]);
    }

    let spelled: Vec<String> = mib.iter().map(u32::to_string).collect();
    note_unknown_sysctl(&spelled.join("."));
    // The documented answer for a name nothing knows, from `sysctl(3)`: *"The name array
    // specifies a value that is unknown."* The number is read from the harvested table
    // rather than written here (D350, D351).
    if let Some(enoent) = orbistoun_hle::constants::abi_constant("errno", "ENOENT") {
        set_errno(enoent);
    }
    FAILED
}

/// Sets this thread's `errno`, the way a failing C library call must.
///
/// This thread's `errno`, as a number.
///
/// Read from the same storage `__error()` hands the guest, so what a guest set and what this
/// reads cannot differ.
fn current_errno() -> i32 {
    let at = error_location(&[0; GUEST_ARG_REGISTERS]);
    let Ok(at) = usize::try_from(at) else {
        return 0;
    };
    // SAFETY: `error_location` answers the address of this thread's `errno`, which is a live
    // `i32` owned by this thread for as long as it runs.
    unsafe { std::ptr::read(std::ptr::with_exposed_provenance::<i32>(at)) }
}

/// A caller reads it through `__error()`, so this writes where that points - the same
/// storage, not a second copy.
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

/// What a failing call answers, as the guest reads it.
///
/// `-1` in a 32-bit register. Written out rather than converted, because `From` is not
/// const and a sign-extended `u64::MAX` is a different value in `eax`.
const FAILED: u64 = 0xFFFF_FFFF;

/// Whether a MIB is asking for the list of running processes.
///
/// `kern.proc.proc`, which is `CTL_KERN`, `KERN_PROC`, `KERN_PROC_PROC` - every component read
/// from the harvested table rather than written here, because a MIB typed from memory is a
/// question answered about the wrong thing.
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
/// # Why this is an answer rather than a refusal
///
/// Both payloads that reach it are doing the same thing - looking for an earlier copy of
/// themselves so they can stand aside or kill it. `klogsrv` takes the failure and carries on;
/// **`ftpsrv` exits**, which is a correct program handling a call that failed for a reason it
/// cannot interpret.
///
/// Nothing here has a process table, and that is not a gap to be papered over: it is the
/// answer. An enumeration that finds nothing is what a guest gets when the process it is
/// looking for is not running, and *no process it could be looking for is running*. So the
/// call succeeds and reports a zero-length result, which a caller reads as "none" - and both
/// payloads then do the right thing for a first launch.
///
/// **It also avoids `struct kinfo_proc` entirely**, which matters: that structure is large,
/// and it is one of the ones whose layout moved between the release this project harvests and
/// the one the target forked from (D374). Answering zero says nothing about its shape.
///
/// Reference: FreeBSD `sysctl(3)`, `kern.proc`. A caller asking only for the size passes a
/// null buffer, which is the ordinary first half of the pair and needs the same answer.
fn no_processes(length_at: u64) -> u64 {
    let Ok(at) = usize::try_from(length_at) else {
        return FAILED;
    };
    if at == 0 {
        // No length to report into. The call still succeeded and there is still nothing.
        return OK;
    }
    // SAFETY: a guest-supplied `size_t *` under the identity mapping (D014) - the same
    // contract the real call has, and the guest passed it expecting to be written through.
    unsafe { std::ptr::write_unaligned(std::ptr::with_exposed_provenance_mut::<u64>(at), 0) };
    OK
}

/// `sysctlbyname(name, oldp, oldlenp, newp, newlen)` - the same question, asked by name.
///
/// # Why this is worth having rather than leaving unimplemented
///
/// A guest asking `kern.osrelease` is asking what kernel it is running on, and it **branches
/// on the answer**. `zftpd` reports `Firmware detection failed` and turns a feature off; a
/// title could do considerably more than that.
///
/// Unimplemented, the call answered a placeholder, which for something returning a length is
/// data. Now it is a named question with a recorded answer, in the same shape as the numeric
/// `sysctl`: what is known is answered, what is not is refused and *reported once*, so the
/// list of names a guest wanted is the work list.
///
/// # What is answered, and what is deliberately not invented
///
/// `kern.osrelease` comes from the machine's own configuration and is **empty by default**.
/// Nothing in this repository knows what a console's kernel calls itself - the FreeBSD
/// checkout is not that kernel - so an unset value refuses rather than answering a plausible
/// version, which would send a guest down a path chosen by a number nobody measured (D397).
///
/// Reference: FreeBSD `sysctlbyname(3)`. A caller passing a null buffer is asking for the
/// size, which is the ordinary first half of the pair and gets the same answer.
fn sysctl_by_name(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (name, out, out_len) = (args[0], args[1], args[2]);
    if name == 0 {
        return FAILED;
    }
    // SAFETY: a guest-supplied string under the identity mapping (D014), bounded.
    let len = unsafe { c_len(name) };
    // SAFETY: `c_len` established `len` readable bytes from `name`.
    let bytes = unsafe { std::slice::from_raw_parts(ptr(name).cast_const(), len) };
    let Ok(asked) = std::str::from_utf8(bytes) else {
        return FAILED;
    };

    // An integer knob is answered as raw bytes of the width the platform uses, because a
    // caller reads it as an `int` or a `long` and not as text. Tried first, so a name that is
    // both here and in the string table cannot be answered two ways.
    if let Some((value, width)) = answer_integer(asked) {
        return answer_bytes(&value.to_le_bytes()[..width], out, out_len);
    }
    let Some(text) = answer_for(asked) else {
        // Reported once, so the names a guest wanted are the work list rather than a silence.
        note_unknown_sysctl(asked);
        return FAILED;
    };
    answer_string(&text, out, out_len)
}

/// An integer knob and the byte width the platform answers it in, or nothing.
///
/// # Every value here was measured, not chosen
///
/// A conformance run read each of these off a target console. They are facts about one
/// machine at one firmware, which is why they answer only when a caller asks - a guest that
/// reads `hw.pagesize` and gets the wrong width mis-sizes every allocation after it, so a
/// guessed value would be worse than the refusal it replaces.
///
/// The width matters as much as the value: `hw.ncpu` is a four-byte `int` and `tsc_freq` is an
/// eight-byte `long`, and a caller reading four bytes of an eight-byte answer, or the reverse,
/// reads a different number than was written.
fn answer_integer(name: &str) -> Option<(u64, usize)> {
    match name {
        // Sixteen hardware threads, read back as a four-byte int.
        "hw.ncpu" => Some((16, 4)),
        // Sixteen-kibibyte pages - `0x4000` - which is also what the direct-memory layer and
        // the loader already assume, now confirmed against hardware rather than assumed.
        "hw.pagesize" => Some((0x4000, 4)),
        // The counter frequency, answered here for the third time by a third route: the time
        // stamp counter reports it, the process-time counter reports it, and a conformance run
        // read `machdep.tsc_freq` off hardware as the same `0x5f25_9b8e` (D398, D405).
        "machdep.tsc_freq" => Some((0x5f25_9b8e, 8)),
        _ => None,
    }
}

/// Writes raw bytes and their length the way `sysctl` reports an integer answer.
///
/// The size/value pair works exactly as [`answer_string`]'s does; only the bytes differ, so
/// the two share the contract and not the code - a string carries a terminator this must not.
fn answer_bytes(value: &[u8], out: u64, out_len: u64) -> u64 {
    let needed = value.len();
    if out_len != 0 {
        let Ok(at) = usize::try_from(out_len) else {
            return FAILED;
        };
        // SAFETY: a guest-supplied `size_t *` under the identity mapping (D014), which the
        // guest passed expecting to be written through.
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
            return FAILED;
        }
    }
    if out == 0 {
        return OK;
    }
    let Ok(at) = usize::try_from(out) else {
        return FAILED;
    };
    // SAFETY: a guest-supplied buffer under the identity mapping (D014), whose room was checked
    // against the declared length above when one was given.
    unsafe {
        std::ptr::copy_nonoverlapping(
            value.as_ptr(),
            std::ptr::with_exposed_provenance_mut::<u8>(at),
            value.len(),
        );
    }
    OK
}

/// What this machine says about one named MIB, or nothing.
///
/// Deliberately a short list. Every entry is a claim about the platform, and a name answered
/// from a guess is worse than one refused - the guest cannot tell them apart, and only one of
/// them is visible in the report.
fn answer_for(name: &str) -> Option<String> {
    match name {
        "kern.osrelease" => {
            let release = &orbistoun_core::machine::presented().kernel_release;
            (!release.is_empty()).then(|| release.clone())
        }
        // **Measured, and firmware-independent.** A conformance run read `FreeBSD` off a target
        // console, and unlike the release it is not a per-machine string - the target kernel is
        // FreeBSD-derived, which the whole project already relies on for the C library's
        // analogues, and this is that fact stated by the platform itself rather than assumed
        // (D405). Cited to the same run in the knowledge base.
        "kern.ostype" => Some("FreeBSD".to_owned()),
        _ => None,
    }
}

/// Writes a string answer and its length the way `sysctl` reports one.
///
/// A null buffer with a length pointer is the *size* half of the pair, and gets the length
/// without the bytes - which is how a caller sizes an allocation before asking again.
fn answer_string(text: &str, out: u64, out_len: u64) -> u64 {
    let needed = text.len() + 1;
    if out_len != 0 {
        let Ok(at) = usize::try_from(out_len) else {
            return FAILED;
        };
        // SAFETY: a guest-supplied `size_t *` under the identity mapping (D014), which the
        // guest passed expecting to be written through.
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
            // Too small, and the length is already written back so the caller can retry -
            // which is what the interface documents rather than a truncation.
            return FAILED;
        }
    }
    if out == 0 {
        // The size half of the pair. Answered, with nothing written.
        return OK;
    }
    let Ok(at) = usize::try_from(out) else {
        return FAILED;
    };
    let mut bytes = text.as_bytes().to_vec();
    bytes.push(0);
    // SAFETY: a guest-supplied buffer under the identity mapping (D014), whose room was
    // checked against the declared length above when one was given.
    unsafe {
        std::ptr::copy_nonoverlapping(
            bytes.as_ptr(),
            std::ptr::with_exposed_provenance_mut::<u8>(at),
            bytes.len(),
        );
    }
    OK
}

/// Records a MIB nothing here implements, once per distinct name.
///
/// Once, because a guest in a retry loop would otherwise bury every other line in the
/// report - and the same name a thousand times is one fact, not a thousand.
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
        use std::io::Write as _;
        let line = format!(
            concat!(
                "orbistoun: sysctl asked for [{}] and nothing here knows it - refused with ",
                "the documented failure"
            ),
            mib
        );
        let mut err = std::io::stderr();
        let _ = writeln!(err, "{line}");
        // And to the kernel log, while the guest is still running to read it (D396).
        orbistoun_core::klog::note(&line);
    }
}

/// `abort()`.
///
/// **Never returns, and that is the entire point.** `abort` is declared `noreturn`, so a
/// compiler emits an unreachable trap immediately after the call. An implementation that
/// returns puts execution into that trap, and the run reports `illegal instruction` at an
/// address with no meaning - which is what two titles were reporting while the guest was
/// doing something perfectly clear: giving up (D177).
fn abort(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    orbistoun_core::stop(orbistoun_core::StopReason::Aborted, args[0])
}

/// `exit(status)`, and `_Exit(status)`.
///
/// Also never returns. A guest ending deliberately is a different outcome from one that
/// faulted, and reporting them the same way loses the distinction that matters most.
fn exit(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    orbistoun_core::stop(orbistoun_core::StopReason::Exited, args[0])
}

/// `operator new(size)`, and its array form.
///
/// The same heap `malloc` uses, because that is what it is - the C++ ABI spells the
/// allocator differently and a program mixing the two must see one heap or it frees
/// pointers the other never gave out.
///
/// **A real `operator new` throws on failure rather than answering null.** This answers
/// null, and that is a stated gap: throwing needs the exception runtime, which does not
/// exist here, and inventing an unwind would be worse than a caller checking a pointer it
/// was not expecting to have to check.
fn operator_new(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    malloc(args)
}

/// `operator delete(pointer)`, and its array and sized forms.
///
/// The size a sized delete carries is ignored: the heap records each allocation's length
/// in its own header, so the caller's figure is at best a duplicate and at worst a
/// disagreement to be resolved in favour of the header anyway.
fn operator_delete(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    free(args)
}

/// `fopen(path, mode)`.
///
/// # Why this had to be implemented rather than stubbed harder
///
/// The guest **dereferences the result without checking it**, which is measured rather
/// than assumed. Unimplemented, it answered `0x7FFF0001` and the guest carried that code
/// through `fseek`, `ftell`, `fread` and `fclose`, sizing a two gigabyte allocation from
/// what `ftell` gave back. Answering null instead, it read offset four of the null and
/// faulted immediately - same fault address, `read of 0x4`, with `rdi` zero.
///
/// So no return value makes this safe. Only a real handle does (D165).
///
/// **Read-only.** Nothing observed writes, and a guest that could write through this would
/// be writing into the user's own title directory. Opening that up is a decision with
/// consequences, not an omission to be quietly corrected.
fn fopen(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(path) = read_guest_path(args[0]) else {
        return 0;
    };
    // Null rather than an error code: the caller reads this as a pointer, so an error
    // code here is a wild pointer (D125). It is still the wrong answer for a guest that
    // does not check - but it is the wrong answer that faults nearest the cause.
    orbistoun_fs::open::open(&path).unwrap_or(0)
}

/// `fclose(stream)`.
fn fclose(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // Zero is success for this call, and a handle naming nothing is the error value that
    // C uses - which is `EOF`, negative one, widened.
    if orbistoun_fs::open::close(args[0]) {
        OK
    } else {
        EOF
    }
}

/// `fread(dest, size, count, stream)`.
///
/// Answers the number of *elements* read, not bytes - a distinction that costs nothing to
/// get right and produces a silently truncated load if got wrong.
fn fread(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (dest, size, count, stream) = (args[0], args[1], args[2], args[3]);
    if dest == 0 || size == 0 || count == 0 {
        return 0;
    }
    let Some(total) = size
        .checked_mul(count)
        .and_then(|n| usize::try_from(n).ok())
    else {
        // A request that cannot be expressed is refused rather than truncated to
        // something plausible.
        return 0;
    };
    let Ok(at) = usize::try_from(dest) else {
        return 0;
    };

    // SAFETY: the guest supplied this destination and declared its size, which is the
    // same contract the real call has. The mapping is identity, so a guest address is a
    // host address; an address the guest has not mapped faults here exactly as it would
    // have faulted in the guest, and the fault reporter names it.
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
/// **A guest sizes buffers from this.** One asked for two gigabytes on the strength of a
/// nonsense answer, so a wrong value here does not stay contained - it becomes a memory
/// request the next subsystem has to refuse.
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
/// Non-zero once a read has hit the end. A handle naming nothing answers zero - "not at
/// the end" - because a caller looping until `feof` on a bad handle would otherwise never
/// stop.
fn feof(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    u64::from(orbistoun_fs::open::at_end(args[0]).unwrap_or(false))
}

/// `ferror(stream)`.
///
/// Always zero: reads either succeed or report short, and nothing here distinguishes a
/// device error from the end of a file. Stated rather than left to look implemented.
fn ferror(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

/// `fflush(stream)`.
///
/// Nothing to flush - everything opened here is read-only.
fn fflush(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

/// `fgets(s, size, stream)` - one line, or up to `size - 1` bytes, whichever comes first.
///
/// # Why it had to answer NULL, not a placeholder
///
/// A guest reading a text file writes `while (fgets(buf, n, f)) { ... }`. Unimplemented, this
/// answered the placeholder - non-null - so the loop never ended: PPSA21564 spun `fgets`,
/// `feof` and `strlen` six and a half million times each, right up to the call budget, having
/// otherwise reached `main` and printed its banner (D454). The one value that stops that loop
/// is the NULL the standard requires at end of file.
///
/// # How it reads
///
/// One byte at a time, keeping the newline that ends a line and stopping on it, so it never
/// consumes past the line the way a fixed-size `fread` would. The result is NUL-terminated,
/// and a call that reads nothing because the stream is already at its end answers NULL - which
/// is what the guest's loop condition tests.
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
                // guest declared, under the identity mapping (D014).
                let slot = unsafe { dest.add(written) };
                // SAFETY: one byte written into that in-bounds slot.
                unsafe { *slot = byte[0] };
                written += 1;
                if byte[0] == b'\n' {
                    // The newline stays in the buffer, which is what distinguishes a whole
                    // line from a truncated one.
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
/// never called. A single process-wide value because C's `rand` is not thread-safe and a guest
/// that wanted per-thread streams would use `rand_r`.
static RAND_STATE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// `rand()` - a value in `[0, 0x7fffffff]`, the platform's `RAND_MAX`.
///
/// Left unimplemented, `rand` fell to the placeholder stub and answered the same number every
/// call, which `035-libc/rand-seeded` catches as two identical draws. A linear congruential
/// generator is what the standard's own example uses; the high bits are returned because an
/// LCG's low bits cycle short. Faithful *sequence* is not what the check measures - that two
/// draws differ and that a re-seed reproduces them is - so the generator is chosen to be
/// obviously seedable rather than to match the platform's numbers.
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
/// Names rather than hashes: the hash is derived from the name, so a table written in
/// hashes could not be read by a person or checked against the declarations above.
pub fn implementations() -> Vec<(&'static str, GuestFn)> {
    let mut all = core_implementations().to_vec();
    all.extend_from_slice(cstring::implementations());
    all.extend_from_slice(clock::implementations());
    all.extend_from_slice(scan::implementations());
    // Implemented next to the mount model rather than here, and declared here because this
    // is the library that exports them (D367).
    all.extend_from_slice(orbistoun_fs::posix::implementations());
    // Only the underscored spelling and the interface list: `inet_ntop` without the
    // underscores is declared in `libScePosix`, where a title was measured importing it,
    // and is served from there.
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
        ("exit", exit),
        ("_Exit", exit),
        ("_Znwm", operator_new),
        ("_Znam", operator_new),
        ("_ZdlPv", operator_delete),
        ("_ZdaPv", operator_delete),
        ("_ZdlPvm", operator_delete),
        ("_ZdaPvm", operator_delete),
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
        // `bcmp` is `memcmp` - equal iff zero, which is all `bcmp` promises.
        ("bcmp", memcmp),
        ("memchr", memchr),
        ("memalign", memalign),
        ("printf", printf),
        ("vsnprintf", vsnprintf),
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
        ("strcat", strcat),
        ("strchr", strchr),
        ("strrchr", strrchr),
        ("atexit", atexit),
        ("signal", signal),
        ("getopt", getopt),
        ("__error", error_location),
        ("strerror", strerror),
        ("puts", puts),
        ("getpid", getpid),
        ("sysctl", sysctl),
        ("kill", kill),
        ("getenv", getenv),
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
    /// **Every constant this crate asks for by name is actually in the table.**
    ///
    /// The lookup answers `None` rather than a default when a name is missing, which is
    /// right - a wrong constant is worse than an absent one - but it means a typo would
    /// silently stop setting `errno` and nothing would say so. This is the guard that makes
    /// somebody notice (D351).
    #[test]
    fn every_name_this_crate_looks_up_is_present() {
        /// Every constant this crate looks up by name.
        ///
        /// A slice rather than an inline literal because it is the thing that grows - the
        /// socket work adds a dozen - and because the guard is only worth having if adding
        /// a lookup means adding a line here.
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

    /// **A constant another crate wrote by hand, checked against the header** (D370).
    ///
    /// `orbistoun-fs` decides whether a guest may write to a path, and the number it compares
    /// the guest's mode against came out of `sys/sys/unistd.h`. That crate cannot read the
    /// harvested table - it does not depend on this one - so the check lives here, where the
    /// table is. A constant nobody checks is exactly the kind that turns out to be a
    /// different platform's.
    #[test]
    fn the_access_mode_another_crate_hardcodes_is_the_headers() {
        assert_eq!(
            Some(orbistoun_fs::posix::W_OK as i64),
            orbistoun_hle::constants::abi_constant("unistd", "W_OK"),
        );
    }

    /// The interface flags, still written out in `orbistoun-fs` and checked here.
    ///
    /// **The socket families used to be checked here too and are not any more**: they are read
    /// straight from the table now, because the table moved down to `orbistoun-hle` and
    /// `orbistoun-fs` can reach it (D385). A number read where it is used needs no test
    /// tying two copies together, because there is one copy. The ones below are the
    /// remainder - `S_IFDIR`, `DT_DIR` and the interface flags are still written out, and
    /// each is a candidate for the same treatment.
    ///
    /// `IFF_LOOPBACK` is the one that matters: a server walks the interface list looking for
    /// an address that is *not* the loopback, so a wrong bit there is a server that prints
    /// `127.0.0.1` and waits for a connection nobody can make.
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

    /// The file-type constants, checked the same way and for the same reason (D370).
    ///
    /// These decide whether a file server shows a name as a folder or as a file, and both
    /// halves of the question - the `stat` bit and the `dirent` byte - are written out in
    /// `orbistoun-fs`, which cannot reach this table.
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

    /// **A spot check that the table holds the header's numbers, not plausible ones.**
    ///
    /// `SOL_SOCKET` is the one worth pinning: it is `0xffff` here and `1` on several other
    /// platforms, so a table built from recall rather than from the header would differ
    /// here first.
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

    /// Calls one implementation by name, with host buffers standing in for guest memory.
    ///
    /// Legitimate because the mapping is identity: a host address *is* a guest address,
    /// so a test buffer is exactly what a guest would have handed over.
    fn call(name: &str, args: [u64; GUEST_ARG_REGISTERS]) -> u64 {
        // Bound before searching: `implementations` now builds its list, so borrowing
        // through the call would drop it while the match is still held.
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
        // The failure that killed a real title: 305 calls to this, then a write to a
        // garbage pointer because the memory it was told to clear never was.
        let mut buffer = [0xFF_u8; 16];
        let at = address(&mut buffer);
        assert_eq!(call("memset", [at, 0x41, 8, 0, 0, 0]), at, "returns dest");
        assert_eq!(&buffer[..8], &[0x41; 8]);
        assert_eq!(&buffer[8..], &[0xFF; 8], "and stops where it was told");
    }

    #[test]
    fn strlen_returns_a_length_and_not_an_error_code() {
        // The single most damaging stub in the project: a guest told a string is
        // fourteen bytes long walks off the end of every buffer it owns.
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
        // The real function is allowed to assume no overlap; a guest that overlaps
        // anyway gets silent corruption from the stricter version, and being permissive
        // costs nothing here.
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
        // Without it the result is not a string, and the next strlen walks into
        // whatever was there.
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
        // The standard requires it and callers rely on it - a short copy left unpadded
        // is an unterminated string.
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
        // `strchr(s, 0)` returns the end of the string, not null. Getting this wrong
        // breaks every caller that uses it to find where a string ends.
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
        // A guest passing null is a guest bug, but faulting inside the C library makes
        // it look like ours - and the fault reporter would name the wrong region.
        assert_eq!(call("strlen", [0, 0, 0, 0, 0, 0]), 0);
        assert_eq!(call("memset", [0, 0x41, 16, 0, 0, 0]), 0);
        assert_eq!(call("strchr", [0, u64::from(b'a'), 0, 0, 0, 0]), 0);
    }

    #[test]
    fn a_guard_reports_uninitialised_once_and_only_once() {
        // The failure this prevents: an unimplemented acquire returns an error, which is
        // non-zero, which reads as "go ahead and initialise" - and with release doing
        // nothing the flag never sets, so every static reconstructs forever.
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
        // A constructor that throws has not initialised the object, and the standard
        // requires the next attempt to try again.
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
        // The failure this replaced: malloc returned the placeholder error code, a title
        // handed that to memset, and memset faithfully wrote to it (D128).
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
        // Sixteen bytes on x86-64. Handing back less faults the first time a caller puts
        // a vector type in it, somewhere unrelated to the allocation.
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

        // Catching this multiplication is half the reason calloc exists.
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
        // Both are defined behaviour that callers use deliberately.
        let at = call("realloc", [0, 32, 0, 0, 0, 0]);
        assert_ne!(at, 0);
        call("free", [at, 0, 0, 0, 0, 0]);
        assert_eq!(call("free", [0, 0, 0, 0, 0, 0]), 0);
    }

    #[test]
    fn a_failed_allocation_answers_null_rather_than_an_error_code() {
        // The whole point of D125: an error code in a pointer register is a wild pointer
        // the guest dereferences. Null is what a caller already tests for.
        assert_eq!(call("malloc", [u64::MAX, 0, 0, 0, 0, 0]), 0);
    }

    #[test]
    fn every_implementation_is_also_declared_here_or_says_why_not() {
        /// Implemented here and declared in another library, deliberately.
        ///
        /// **Where a symbol is declared is a claim about the target; where its code lives
        /// is a claim about this repository** (D367). These two are C library functions
        /// with no vendor-named twin, and a title was measured importing them from
        /// `libScePosix` - so that is where they are declared, and `orbistoun-posix`
        /// delegates to the code here. A list rather than a blanket exemption, because the
        /// guard is only worth having if a new one has to be argued for.
        const DECLARED_ELSEWHERE: &[&str] = &["gettimeofday", "clock_gettime"];

        // An implementation nobody declared can never be reached: resolution goes
        // through the declared symbol list.
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
        // Only the conversion says whether the top bit is a sign. Getting this wrong turns
        // -1 into four billion, which is the kind of value that looks like a corrupt pointer
        // three frames later.
        let args = [u64::MAX];
        assert_eq!(render_format(b"%d", &args).expect("d"), b"-1");
        assert_eq!(render_format(b"%ld", &args).expect("ld"), b"-1");
        // `%u` is an *unsigned int* - thirty-two bits - and `%lu` is the whole word. They are
        // different questions about the same register and must not answer the same.
        assert_eq!(render_format(b"%u", &args).expect("u"), b"4294967295");
        assert_eq!(
            render_format(b"%lu", &args).expect("lu"),
            b"18446744073709551615"
        );
    }

    /// **A conversion's default width is `int`**, and the modifier changes it.
    ///
    /// It did not use to matter: a caller storing an `int` writes `edi`, which zeroes the
    /// upper half of `rdi`, so reading sixty-four bits was right by accident. An argument on
    /// the *stack* sits in an eight-byte slot whose upper half is unspecified, and the first
    /// thing to read one logged `RES=-4294967296` - a zero with somebody else's bits above
    /// it (D385).
    #[test]
    fn a_conversion_reads_only_as_many_bits_as_its_width() {
        // A zero with rubbish above it, which is exactly what a stack slot holds.
        let dirty = [0xFFFF_FFFF_0000_0000_u64];
        assert_eq!(render_format(b"%d", &dirty).expect("d"), b"0");
        assert_eq!(render_format(b"%u", &dirty).expect("u"), b"0");
        assert_eq!(render_format(b"%x", &dirty).expect("x"), b"0");
        // And the modifier says to read all of it, which is a different answer.
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
        // Otherwise a format containing a literal percent silently shifts every later
        // conversion onto the wrong argument - which renders successfully and is wrong.
        assert_eq!(
            render_format(b"100%% of %d", &[7]).expect("pct"),
            b"100% of 7"
        );
    }

    #[test]
    fn length_modifiers_are_consumed_rather_than_mistaken_for_conversions() {
        // `%llu` must not be read as `%l` followed by junk. Every integer argument is a
        // whole register here, so the modifier changes nothing but still has to be eaten.
        assert_eq!(render_format(b"%llu", &[9]).expect("llu"), b"9");
        assert_eq!(render_format(b"%zu", &[9]).expect("zu"), b"9");
        assert_eq!(render_format(b"%hhd", &[9]).expect("hhd"), b"9");
    }

    #[test]
    fn a_floating_point_conversion_is_refused_rather_than_guessed() {
        // **The one that matters most.** A variadic double arrives in an XMM register and
        // the trampoline captures the six integer registers only, so the value never
        // reached this function. Rendering anything would be a confident number derived
        // from an unrelated register.
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
        // Three integer registers survive the fixed parameters. A fourth conversion read
        // whatever was in the array beyond them, which is not the guest's argument.
        assert_eq!(
            render_format(b"%d %d %d %d", &[1, 2, 3]),
            Err(FormatFault::OutOfArguments)
        );
    }

    #[test]
    fn an_unknown_conversion_names_itself() {
        // So a report can say which one to implement next, rather than "formatting failed".
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
        // Every implementation of note does this, and a guest relying on it would
        // otherwise fault inside formatting rather than at whatever produced the null.
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
        // snprintf semantics: the return is what *would* have been written, so a caller
        // detects truncation by comparing it against the size it passed. Reporting the
        // copied length instead would make truncation invisible.
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
        // The bounded variant's entire reason to exist. A guest that passed a small buffer
        // must not have its neighbours overwritten, and must not read past the end.
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
        // Bounded wrong, not plausibly wrong. An empty string shows up immediately; a
        // half-rendered one opens the wrong file and surfaces somewhere unrelated.
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
        // **Which** conversion, asked of the renderer rather than of the counter.
        //
        // `first_fault` is one slot for the whole process and the tests share one, so
        // asserting a specific value there passes or fails on which test ran first. It
        // passed for a long time because nothing else recorded a fault; adding the first
        // `vsnprintf` test that does made it fail, and the test was the wrong thing, not
        // the new one. Same shape as the three shared-state hazards before it: ask the
        // thing that computed the answer, not a global that happens to hold one.
        assert_eq!(
            render_format(b"%f percent", &[]),
            Err(FormatFault::FloatingPoint('f')),
            "and it can say which conversion was responsible"
        );
    }

    #[test]
    fn a_zero_sized_destination_is_left_alone() {
        // There is no room even for a terminator, so writing one would corrupt whatever
        // the guest put next to it.
        let format = b"x\0";
        let mut dest = [0xAA_u8; 4];
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = dest.as_mut_ptr() as usize as u64;
        args[1] = 0;
        args[2] = format.as_ptr() as usize as u64;

        assert_eq!(snprintf_s(&args), 0);
        assert_eq!(dest, [0xAA; 4]);
    }
    #[test]
    fn memalign_returns_what_it_was_asked_for() {
        // The guest asked for eight and got a placeholder error code, which is not
        // eight-aligned - which is precisely what it then complained about (D190).
        for align in [8_u64, 16, 32, 64, 256, 4096] {
            let p = call("memalign", [align, 300, 0, 0, 0, 0]);
            assert_ne!(p, 0, "alignment {align} should be satisfiable");
            assert_eq!(p % align, 0, "alignment {align} was not honoured");
            call("free", [p, 0, 0, 0, 0, 0]);
        }
    }

    #[test]
    fn an_alignment_that_is_not_a_power_of_two_is_refused() {
        // Rounding up on the caller's behalf would hide a bug in the caller, and every
        // allocator interface requires this of its argument.
        assert_eq!(call("memalign", [24, 100, 0, 0, 0, 0]), 0);
        assert_eq!(call("memalign", [0, 100, 0, 0, 0, 0]), 0);
    }

    #[test]
    fn an_aligned_block_frees_through_the_same_path_as_an_ordinary_one() {
        // **The reason there is one allocation path.** `dealloc` given a layout that
        // differs from the one `alloc` received is undefined behaviour, so the alignment
        // has to survive from allocation to release. Under a separate aligned path the
        // first disagreement would be a heap corruption with no connection to either.
        let a = call("memalign", [4096, 64, 0, 0, 0, 0]);
        let b = call("malloc", [64, 0, 0, 0, 0, 0]);
        assert_ne!(a, 0);
        assert_ne!(b, 0);
        call("free", [a, 0, 0, 0, 0, 0]);
        call("free", [b, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn realloc_preserves_the_payload_of_an_aligned_block() {
        // `realloc` sizes the copy from the recorded header. With the offset no longer
        // fixed at the header size, using the header size here would copy too much from a
        // heavily aligned block and read past its payload.
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
        // A wild pointer becomes a no-op rather than a `dealloc` against a layout nobody
        // allocated. The real `free` has the same contract; this one can decline instead.
        let mut junk = [0_u64; 8];
        let stray = std::ptr::addr_of_mut!(junk[4]) as usize as u64;
        assert_eq!(call("free", [stray, 0, 0, 0, 0, 0]), 0);
    }

    /// A guest's argument list, laid out exactly as the psABI says one is.
    ///
    /// Legitimate for the same reason every other buffer here is: the mapping is identity,
    /// so a list built on the host *is* a list a guest could have passed. The register
    /// half is filled from `spilled` and everything past six goes to the overflow area,
    /// which is what a compiler's prologue would have produced.
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
            // Read so the fields are not merely dead storage to the compiler; the
            // addresses inside `list` point at both.
            let _ = (self.save.len(), self.overflow.len());
            self.list.as_ptr() as u64
        }
    }

    /// **The capability the register forms do not have** (D364).
    ///
    /// Seven conversions is one more than there are integer registers, so the register
    /// form refuses the whole thing - and refusing is right for it, because it genuinely
    /// cannot see the seventh. A `va_list` can, and renders it.
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

    /// The measure-then-allocate idiom, which is why a size of zero must not be refused.
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

    /// A caller that already spent registers on its own fixed parameters - the ordinary
    /// case, since `vsnprintf` has three of its own.
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

    /// The integer knobs answer the values, and the widths, hardware was measured to use.
    ///
    /// **This is the measurement written as a test.** A conformance run read each off a target
    /// console; if a value or its width is ever changed the failure names which reading it
    /// contradicts, rather than a number moving and nobody knowing what it cost (D405).
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

    /// `kern.ostype` answers what the platform answered, and it is not a per-machine string.
    #[test]
    fn ostype_is_the_measured_platform_name() {
        assert_eq!(super::answer_for("kern.ostype"), Some("FreeBSD".to_owned()));
    }

    /// An integer knob writes its width and no more, and reports that width through the pair.
    ///
    /// The case that matters: a caller reading four bytes of a value must be handed exactly
    /// four, not eight with the top half being whatever the buffer held.
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
