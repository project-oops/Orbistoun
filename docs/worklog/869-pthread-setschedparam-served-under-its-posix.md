# 869. pthread_setschedparam and pthread_getschedparam served under their POSIX names

**2026-09-25**. PPSA04263 imports `libScePosix::pthread_setschedparam`, and that import answered a
placeholder. The vendor twin, `scePthreadSetschedparam`, has been implemented for a long time.
Both spellings take `(thread, policy, const struct sched_param *)`, and `pthread_self` already
delegates, so the handles are the same kind.

Both are now in the POSIX delegation table, with `pthread_getschedparam` alongside. That is D349's
mechanism: one function pointer, so the two cannot drift, and its note on failure codes still
applies.

PPSA04263 now makes 602 calls on stubs, down from 603. The wall is unchanged
(`image+0x19676d7`, worklog 792). Its remaining stubs are the two APR calls, which wait on
REQ-20260925T1834Z-a7e2 like PPSA25872, and `sceImeUpdate` and `sceUserServiceGetGamePresets`,
which need measured values.
