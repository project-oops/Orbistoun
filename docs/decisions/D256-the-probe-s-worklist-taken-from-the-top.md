# D256 - The probe's worklist, taken from the top


**decided** · 2026-08-25 · the first turn of the loop driven by a conformance suite

The conformance probe's first full run ranked what it wanted and could not have: seventeen
calls to `scePthreadMutexDestroy`, thirteen to `sceKernelGetProcessTime`, nine to
`scePthreadMutexTrylock`. All three were undeclared; the sync layer already had `try_lock`
and `destroy`, so only the guest-facing entries were missing.

Implementing the three moved six checks from fail to pass and made two further sections
reachable - `010-kernel` went to a clean three, `015-sync` 3->5, `018-relational` 5->8.

Three things the run decided rather than the author:

- **`trylock` must not answer `OK` when it fails.** `lock` blocks, so success is its only
  interesting answer; `trylock` exists to report that it could *not* take the mutex, and a
  guest branches on that. A stub reporting success would send it into a critical section it
  does not hold - worse than the missing implementation, because nothing in the trace would
  say so.
- **`GuestError::Busy` is a new variant, not `InvalidArgument`.** A held lock is the
  *ordinary* outcome of `trylock`, not a caller error, and spelling them the same would make
  a trace unable to tell a busy lock from a bad pointer.
- **`GetProcessTime`'s unit is assumed.** The probe checks that two readings increase, which
  establishes the clock is monotonic and says nothing about microseconds. A wrong unit passes
  that check and mistimes anything that waits, so it is recorded as an assumption for the
  hardware trip rather than as a fact.

The gate caught the omission that mattered: three functions were implemented before their
knowledge entries existed, and `every_implemented_function_is_written_down` refused the
build until they were written.

