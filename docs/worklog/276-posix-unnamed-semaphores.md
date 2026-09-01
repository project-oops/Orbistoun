# 2026-09-01 - (/loop) POSIX unnamed semaphores; the Cond.cpp wall is not a traced HLE call

Continued the spine on PPSA21564's `Conc/Cond.cpp:212: rc == 0` abort. The run's most prominent gap was
`sem_init` (11 calls, placeholder), so implemented the POSIX unnamed-semaphore family
(`sem_init`/`sem_wait`/`sem_trywait`/`sem_post`/`sem_destroy`) in `orbistoun-kernel` on the existing `sync`
semaphores, served via `orbistoun-posix`. `sem_t` holds the handle (same model as `pthread_cond_init`), and
`pshared` is ignored. Correct and needed - eleven placeholder concurrency calls now function. Recorded D455.

It did not move the wall (verdict same), and the investigation is the useful part:

- No cond/mutex/sem HLE call of any spelling precedes the assert in the trace, so the engine's `Cond` is
  built on an inlined/low-level primitive, not our HLE. Implementing more POSIX concurrency names won't reach
  it. The fault is a `read of -1` on the assert/abort path (rdi=0x51f), possibly downstream of the assert.
- File-based static RE is blocked: PPSA21564's eboot is 251 MB (122 MB code LOAD), and the fault's runtime
  bytes are not present anywhere in the eboot file - the code is transformed at load. (An earlier byte-level
  pass also mistakenly reused PPSA02664's segment offsets.) Next attempt should be emulator-side - which
  thread asserts, what -1 it reads, the abort-handler path - not eboot disassembly.

`fmt`/`clippy`/tests/knowledge pass for the additions (`cast_unsigned` avoided for MSRV 1.85). Additive.
