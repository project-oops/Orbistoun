# The payload servers jump to null inside `find_pid` *(answered - D359)*


**Both payloads that run, at the same place, for a reason that is not the obvious one.**

`klogsrv` (at `find_pid+209`) and `ftpsrv` (at `find_pid+154`) both reach their own
`find_pid` - the SDK helper that looks up a process id - report a failure through their own
diagnostic path, and then execute `instruction fetch from 0x0`. It is the single thing
standing between here and Stage 2 of [PAYLOADS.md](../PAYLOADS.md): **neither guest reaches a
socket call**, so implementing sockets now would build toward something unreachable.

**Eliminated, by experiment rather than by argument:** it is not caused by `sysctl` refusing.
Patching `sysctl` to answer a size query with zero-length success moved the guest a little
further - 6 imports to 7, the fault from `image+0x5eaa` to `image+0x5f34` - and it still
jumped to null, still after the same `printf` / `__error` / `strerror` sequence. The patch
was reverted; it was a diagnostic, not a fix.

Also eliminated: `strerror` itself works, and is not returning a bad pointer. The guest's
own message *contains this project's `strerror` text* - `main.c:278:sysctl: error 2
(orbistoun has no message table)` - so it was called, answered a readable pointer, and the
caller rendered it.

And it is not `exit` returning from a `noreturn` call: `exit` is implemented, and stops the
run through `orbistoun_core::stop`.

**Answered (D359): it is a global that `__crt_start` would have initialised.** Entering at
`main` skips the program's initialisation, so the global holds what `.bss` holds - zero,
which is indistinguishable from a null function pointer. `ORBISTOUN_BSS_FILL=b5` changes the
fault completely and identically in both payloads.

`signal` and the data-import storage were both eliminated on the way, each by answering a
recognisable marker and watching the fault stay at exactly zero.

