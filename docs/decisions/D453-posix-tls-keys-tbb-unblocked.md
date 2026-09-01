# D453 - POSIX thread-specific-data keys; PPSA21564's TBB scheduler stops aborting


**measured** - 2026-09-01 (user-directed /loop)

PPSA21564, "booting" after D451/D452, did not actually reach a main loop - it raced (the D450
non-determinism) between running to the call budget and **aborting**: `TBB failed to initialize task
scheduler TLS` -> `the guest called abort`. Unity ships Intel TBB, whose task scheduler keeps its per-thread
state through `pthread_key_create`/`pthread_setspecific`. Those were declared in `orbistoun-posix` but served
by nobody (arity 0, provisional), so they answered the placeholder - which TBB read as key allocation
failing.

**The fix.** orbistoun has *static* ELF TLS (the `__thread` block, in the loader) but not the POSIX
*dynamic* key API. Implemented the family - `pthread_key_create`/`pthread_setspecific`/
`pthread_getspecific`/`pthread_key_delete` - in `orbistoun-kernel` beside the thread registry, and wired the
`libScePosix` delegation. A key is a small integer from a monotonic counter; the value bound to it is per
thread, held in a `thread_local!` map, because a guest thread is a host thread here (D014) and a thread that
never set a key must read null - which a fresh map gives, exactly as POSIX requires. The destructor
`pthread_key_create` is handed is recorded nowhere and never run: nothing tears a guest thread down through
this layer yet.

**The result, and it is now deterministic.** The TBB abort is gone (`(was the guest called abort)`), and -
notably - the fault site stopped racing: every run now reaches the *same* new wall at ~500k calls. So the
TLS keys did double duty, removing both the abort and the scheduler-timing nondeterminism D450 first saw on
this title.

**The new wall, characterised.** `read of 0x7fff0001` with `rdi = 0x7fff0001` - a placeholder dereferenced
as a first-argument pointer, in a string compare (`strncmp` -> `c_len`, misattributed by nearest-symbol to
the adjacent `__cxa_guard_acquire`). The report also flags "1 system parameter query answered with a
placeholder". So an unimplemented **pointer-returning** call still in this title's set - `sceKernelGetGPI`,
`asctime`, or `localtime` are the live suspects - hands back `0x7fff0001`, and a C++ static initialiser
feeds it to a string compare. This is the same D344 class one layer deeper, reached only because TBB now
initialises. `asctime`/`localtime` are the awkward ones: they return a `char*`/`struct tm*`, and the `tm`
layout is implementation-defined rather than cleanly citable, so the honest fix is readable storage (as with
the ctype table) rather than a guessed layout - to be settled next.

`fmt`/`clippy`/`cargo test`/the knowledge audit pass for the additions (the kernel crate's pre-existing
fmt drift in test code is untouched and not in these additions). Additive - four new POSIX symbols served -
so other titles are unaffected.
