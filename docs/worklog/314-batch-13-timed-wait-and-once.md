# 2026-09-02 - (/loop) Bulk port batch 13: the timed condition wait and `pthread_once`

```
documented   715 needed, 243 missing   ->   715 needed, 239 missing
```

Two functions and their `posix_` twins. Both reuse machinery that was already there, which is
what made them cheap - and finding that out was the whole of the work.

## What was already available

- `sync::cond_wait` **already takes an `Option<Duration>`**, so a timed condition wait needed no
  new primitive at all - only the argument conversion and the mutex dance the untimed one does.
- `read_xtime` and `duration_until` already convert a two-word time structure to a deadline, for
  the C11 `_Cnd_timedwait`. C11's `xtime` and POSIX's `timespec` are the same shape, so the
  reader is shared rather than written twice.
- `thread::call_guest` already runs a guest callback on a fresh stack through the reentrant
  call, for the C++ runtime's `_Execute_once`.

## `pthread_cond_timedwait`

**`abstime` is an absolute deadline, not a duration.** That is the half a caller's own code
depends on: it computes "now plus a second" once and re-passes the same deadline around its
spurious-wakeup loop, so treating it as relative restarts the clock every turn and the loop never
ends. A deadline already past becomes a zero wait rather than a negative one.

It carries the same non-atomicity the untimed wait already records: the condition variable and
the mutex are independent objects here, so a signal landing between the unlock and the wait is
lost where the platform would hold it. Stated in the code rather than left to be discovered.

## `pthread_once`

Not a forward to `_Execute_once`, though they sit next to each other. **The POSIX routine takes
no arguments and answers nothing**; the C++ runtime's callback is `InitOnce`-shaped and reports
success, and `_Execute_once` only marks its flag done when the callback says it succeeded. Two
different contracts, so two entry points - the same reasoning that split the rwlock and barrier
inits in worklog 312.

The flag is marked done **after** the routine returns, which is what leaves a routine that never
returns recorded as incomplete rather than complete.

Both carry `_Execute_once`'s own caveat, which applies unchanged: **not yet serialised across
threads**. Two threads racing the same fresh flag could both run the initialiser. Nothing
measured does, and a per-flag guard is the fix when something does.

## State

clippy `--tests` clean, fmt clean, kernel/posix tests pass, identity scan clean, nothing
committed.

**Next**: 239. The timed *lock* family (`pthread_mutex_timedlock`, `sem_timedwait`,
`pthread_rwlock_timedrdlock`/`timedwrlock`) needs timeout variants added to `sync.rs` - the
mutex, semaphore and rwlock primitives take no deadline, unlike the condition variable. That is
ordinary work but it is primitive work rather than glue. The socket scatter/gather (`sendmsg`,
`recvmsg`, `sendto`, `recvfrom`) can reuse the `iovec` reader from `readv`/`writev`. Thread
cancellation still needs a design rather than a spec reading.
