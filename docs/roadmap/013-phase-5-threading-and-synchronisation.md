# Phase 5 - Threading and synchronisation *(begun)*


`orbistoun-kernel`. Real host threads, correct per-thread TLS, and the mutex and
semaphore primitives. FreeBSD is the reference here - cite it per function.

**Observable result:** a multi-threaded guest gets past initialisation, and the trace
shows interleaved call sites across thread ids.

**Where it is.** Mutexes, mutex attributes and semaphores are implemented and exercised -
one title constructs eleven mutexes during static initialisation, and implementing
`sceKernelCreateSema` took two titles from 45 calls to 222. `scePthreadCreate` is written
and **has never been called by a guest**, so nothing above has been tested against a
second thread, and the observable result is not close.

The trace side of it is not built either: the current recorder is a fixed-size ring with
no per-thread sequence numbers and no way to write one out. See the call-recorder entry
in [BACKLOG.md](../BACKLOG.md).

