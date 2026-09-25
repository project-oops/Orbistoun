# 873. Execute breakpoints report their own hits

**2026-09-25**. The watchpoint summary kept one execute snapshot and printed one total count beside
the first hit's address. So `0x10` "hit 2 times" when `0x10` and `0xb2` had each hit once.

That misled a bisection of PPSA04263's `.ctors` list: four breakpoints per run, and each run read as
"only the first entry ran". That pointed at constructor 27 never returning, which was wrong. The main
thread's stack holds `image+0xbf`, the return address of `_start`'s `call main`, so the list completes
and `main()` runs.

**Change.** Each execute slot keeps its armed address and its own hit count (`EXEC_SLOT_AT`,
`EXEC_SLOT_HITS`). The summary prints one line per breakpoint, including `never hit`, and labels the
register snapshot with the breakpoint it came from.

A slot no longer forgets its address on the first hit. It disarms on the thread that hit it, and a
hit on a second thread (every guest thread carries the breakpoints now, worklog 871) is still
counted, where before it was passed on as a stranger's exception. Execute slots are left out of the
data-watch list.

Test: `execute_hits_are_counted_per_breakpoint`. It was watched failing with every hit charged to
slot 0.

**PPSA04263, corrected.**

- `_start` (`image+0x80`) calls the constructor runner at `image+0x10` itself, and that walks
  158 `.ctors` entries. All of them run.
- The static object at `image+0x5b37e98` is still never written, and that is now checked on every
  thread (worklog 794's watchpoint was one-thread).
- So the object is not a static constructor's. It is built lazily, on a path this run never
  reaches, which is worklog 796's execution divergence.
- The main thread sits in `main()` below `image+0x4b7c77`, `0x4b7280` and repeated `0x2ba47a0`
  frames. That is where to look next.
