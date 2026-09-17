# Phase 5 - Threading and synchronisation *(begun)*


`orbistoun-kernel`. Real host threads, correct per-thread TLS, and the mutex and
semaphore primitives. FreeBSD is the reference here - cite it per function.

**Observable result:** a multi-threaded guest gets past initialisation, and the trace
shows interleaved call sites across thread ids.

**Where it is.** Mutexes, mutex attributes and semaphores are implemented and exercised -
one title constructs eleven mutexes during static initialisation, and implementing
`sceKernelCreateSema` took two titles from 45 calls to 222. `scePthreadCreate` **is called
125 times across four titles** (PPSA02664 33, PPSA03416 33, PPSA25872 44, PPSA04263 15), each
spawning a real host thread, and the attribute block those calls pass is now honoured for the
stack size and affinity obSCEne measured the console honours (REQ-...c2e9). The observable
result is still not close - a multi-threaded guest getting *past* initialisation with
interleaved call sites in the trace is more than one honoured attribute - but it is no longer
untested against a second thread.

The trace side of it is not built either: the current recorder is a fixed-size ring with
no per-thread sequence numbers and no way to write one out. See the call-recorder entry
in [BACKLOG.md](../BACKLOG.md).

