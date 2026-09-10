# D652 - The pending-signal slot

**Status:** measured
**Date:** 2026-09-09

## Built where D651 said, not where D650 said

D650 sketched a slot checked on the way into the thunk dispatch. D651 ruled that out by
measurement - the raise is call 310,985 of 310,986, so the target never dispatches again - and
found the route that works: `main` is asleep in `sceKernelSyncOnAddressWait`, a wait this project
owns.

So the slot is checked **in the wait**, not in the dispatch.

## The three pieces

**A slot per thread, read without a lock.** `SignalSlot { pending: AtomicU64, parked: AtomicBool }`,
held by `Arc` and cached in a thread-local by `become_thread`. The `Arc` is the point: the pending
flag is read from inside a condition-variable predicate, under the wait queue's lock, and reading
it from the thread table there would take the table lock while holding the queue lock -
`sceKernelRaiseException` takes them the other way round, so it would be a lock-order inversion
and eventually a deadlock. A cached `Arc` is read with no lock at all and the inversion cannot
arise.

**A loop in the wait, because a signal is not a wake.** The predicate becomes
`tokens == 0 && !signal_pending()`. A thread woken for a signal has *not* had its word written and
is still waiting for what it came for, so it runs the handler and goes back to sleep. Returning
`Woken` there would hand a guest a satisfied wait whose condition is still false. The lock is
dropped before the handler runs, because the handler is guest code and may call anything in the
module - including a wait on the same queue.

The signal is taken **before** the token, even when a wake arrives with it: hardware runs the
handler whatever else was happening, and breaking out on the token would leave the signal pending
until some later wait, which for a thread that never waits again is never.

**Hooks rather than a call upward.** `sync` knows nothing about guest memory or guest threads -
`wait_on_address` already takes its word-read as a closure so the module can be tested against a
word it owns. Running a guest handler is further outside that boundary again, so it is inverted
down as a `SignalDelivery { pending, deliver }` pair, the same shape as the thread-start hook.
Absent one, a wait behaves exactly as it did.

## The refusal is what makes the acceptance honest

`raise_pending` answers **false** for a thread that is not parked, and `raise_exception` then
returns the loud placeholder rather than `0`. This is the load-bearing part: `0` tells the guest a
handler will run, and a collector that believes it waits forever for an acknowledgement it was
promised. A signal accepted and silently dropped is worse than one refused, and the difference is
invisible from inside the guest.

The negative test is written first for that reason - accepting when delivery is impossible is the
failure that would never show up as a failure.

## What it did

PPSA25872, whose whole story this has been:

```text
before:  ran to the time limit          (silent for 19.7s of 20)
after:   the title's own modules+0x170021b
```

The handler **ran**. `rdi = 0x1e`, so the signal number arrived; `rsp` is inside the reentrant
stack `call_guest` reserves, so it ran where it should; `r14` still holds the handler's own
address. It executed `inc dword [rip+...]`, `cmp edi, 0x1e`, fell through the `jne` - and then
`mov rax, [rsi+0xf8]`, where `rsi` is the exception context and orbistoun passes null.

`read of 0xf8`, at a named instruction, for a stated reason.

**That fault is the design working, not failing.** The context layout is unpublished; a plausible
block would have carried the guest past this instruction on invented fields and buried the real
question. Null makes the guest stop exactly where the missing knowledge is, and the next request
asks for the sixteen bytes that matter.

Two runs of the build agree on all of it (D181).

## The limit, unchanged from D651

Delivery reaches a thread parked in a wait that consults its slot, and no other. A mutator
spinning in pure guest code is still unreachable, and this title works only because Unity's
happens to be on a futex. `sceKernelSyncOnAddressWait` is the one wait wired up; the others are a
line each when a guest needs them, and none should be wired speculatively.
