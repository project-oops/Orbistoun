# D455 - POSIX unnamed semaphores (sem_init family); the Cond.cpp wall is not a traced HLE call


**measured** - 2026-09-01 (user-directed /loop; spine-crunching PPSA21564 toward code-correct headless emulation)

Past the time/fgets work (D454), PPSA21564 aborts on `Conc/Cond.cpp:212: rc == 0`, and the run's most
prominent gap was `sem_init`, called eleven times and answered by the placeholder.

**Implemented: the POSIX unnamed-semaphore family** - `sem_init`/`sem_wait`/`sem_trywait`/`sem_post`/
`sem_destroy` - in `orbistoun-kernel` on the existing `sync` semaphore primitives, served under their POSIX
names via `orbistoun-posix`. `sem_t` is the guest's own storage, so `sem_init` writes the host handle into
it and the rest read it back - the same "handle lives in the object" model `pthread_cond_init`/`cond_at`
already use. `pshared` is ignored (a guest thread is a host thread). This is correct and needed regardless of
the wall: eleven placeholder-returning concurrency calls now function.

**But it did not move the wall, and the reason is worth recording.** The `Cond.cpp:212` assert still fires,
verdict unchanged. Investigation established two things that redirect the next attempt:

- **No condition-variable, mutex, or semaphore HLE call appears in the trace before the assert** - of any
  spelling (`pthread_cond_*`, `scePthreadCond*`, `pthread_mutex_*`, `sem_*`). So the engine's `Cond` is not
  built on this project's HLE primitives; it uses something inlined or lower-level (raw atomics, an event
  queue, or a futex-class syscall the guest issues directly). Implementing more POSIX concurrency names will
  not reach it. The fault is a `read of 0xffffffffffffffff` (a -1 dereferenced) with `rdi = 0x51f` - which is
  the assert/abort path, possibly downstream of the assert rather than the `rc` source itself.
- **File-based static RE of this fault is blocked.** PPSA21564's eboot is 251 MB; its code LOAD is 122 MB;
  and the fault's runtime bytes (the `mov esi,0xd4` line-number load, the assert-string `lea`s) are **not
  present anywhere in the eboot file** - so the executing code is transformed at load (the earlier byte-level
  analysis had also mistakenly reused PPSA02664's segment offsets). Locating this fault needs the runtime
  image, not the file: an emulator-side approach (which thread asserts, what -1 it reads, the assert/abort
  handler path) rather than disassembling the eboot.

`fmt`/`clippy`/`cargo test`/the knowledge audit pass for the additions (`cast_unsigned` was avoided for the
1.85 MSRV; the kernel crate's pre-existing test-code fmt drift is untouched). Additive - POSIX symbols only.
