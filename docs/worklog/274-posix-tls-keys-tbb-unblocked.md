# 2026-09-01 - (/loop) POSIX TLS keys unblock Unity's TBB scheduler; PPSA21564 stops aborting/racing

Checked what "PPSA21564 boots" actually meant and found it was racing between running to budget and aborting
with `TBB failed to initialize task scheduler TLS`. Unity's Intel TBB builds its per-thread state on
`pthread_key_create`/`pthread_setspecific`, which were declared in `orbistoun-posix` but served by nobody, so
they answered the placeholder and TBB read that as failure.

Implemented the POSIX dynamic TLS-key family (`pthread_key_create`/`setspecific`/`getspecific`/`key_delete`)
in `orbistoun-kernel` beside the thread registry - keys from a monotonic counter, values in a per-thread
`thread_local!` map (a guest thread is a host thread, D014) - and wired the `libScePosix` delegation. orbistoun
already had static ELF TLS (the `__thread` block); this is the dynamic key API it lacked. Recorded D453.

Result: TBB abort gone, and the fault site stopped racing - every run now deterministically reaches the same
new wall at ~500k calls. So the TLS keys removed both the abort and the D450 scheduler nondeterminism on this
title.

New wall: `read of 0x7fff0001`, `rdi = 0x7fff0001` - a placeholder dereferenced as a string pointer in a C++
static initialiser (`strncmp`->`c_len`, misattributed to `__cxa_guard_acquire`). An unimplemented
pointer-returning call (`sceKernelGetGPI`/`asctime`/`localtime` are the suspects) still hands back the
placeholder. `asctime`/`localtime` need the `tm` layout, which is implementation-defined - so the honest fix
is readable storage, not a guessed layout. Next.

`fmt`/`clippy`/tests/knowledge audit pass for the additions; kernel's pre-existing test-code fmt drift is
untouched. Additive (four POSIX symbols); other titles unaffected.
