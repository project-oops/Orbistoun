# D732 - A fiber is a guest context switched on the thread that runs it

**Status:** decided
**Date:** 2026-09-27

A libSceFiber fiber is an execution context, not a thread: the memory the guest supplies is its
stack, and `sceFiberRun`, `sceFiberSwitch` and `sceFiberReturnToThread` suspend the running
context inside the call and resume another on the same host thread. `orbistoun-abi::context`
switches: it saves the registers System V makes a callee preserve (`rbx`, `rbp`, `r12`-`r15`),
`MXCSR` and the x87 control word on the running stack, records the stack pointer, and restores
the target's. A fiber that has never run gets the same frame, resuming at a start routine that
calls `entry(argOnInitialize, argOnRun)` at the aligned top and stops the run, naming the fiber,
if the entry returns. The books live in orbistoun, not in the guest's `SceFiber` object: a record
per fiber address whose resume pointer is the fiber's state (non-zero while suspended, taken by
whatever resumes it), the running fiber per thread, and a per-thread slot for the thread's own
suspended context. A handler's Rust frames stay on the suspended stack and continue when it is
resumed, possibly on another host thread, so a thread-local read after a switch is computed
afresh: the fiber handlers and the dispatch's per-call bookkeeping use helpers that are never
inlined.

**Why:** a fiber's entry and everything it calls run on the fiber's own stack and continue from
where they suspended, which only a stack switch reproduces; the handlers that suspend sit in the
middle of guest call chains that must return normally when resumed. Callee-saved state is all a
cooperative switch needs, because every switch is a call. Taking the resume pointer atomically
makes "running" a property no two threads can both claim, with no lock held across a switch, and
the switch's write of the stack pointer is its last touch of the old stack. Guest thread-local
storage is per host thread, as on the platform, so a fiber sees the TLS of the thread running it.

**Rejected:**
- A host thread per fiber, handed control with a semaphore: a fiber would read another thread's
  TLS and `scePthreadSelf`, and a fiber migrating between threads could not be expressed.
- Running the entry synchronously inside `sceFiberRun` on a fresh stack (D430's shape): the fiber
  could never be resumed after `sceFiberReturnToThread`, only restarted.
- Keeping fiber state in the guest's `SceFiber` object: its layout is unmeasured, and a written
  guess could collide with what the platform keeps there.
- Returning from the entry into whatever sits above the stack top: undefined on every platform;
  the platform's behaviour is unmeasured, so the run stops and says so.
