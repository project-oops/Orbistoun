# 476. The handler ran

**2026-09-09** - directed, continuing 475

Built the pending-signal slot, in the place D651 measured rather than the place D650 guessed.

## Three pieces

**A slot per thread, lock-free to read.** `SignalSlot { pending, parked }` behind an `Arc`, cached
in a thread-local by `become_thread`. The `Arc` is the whole trick: the pending flag is read inside
a condvar predicate under the queue lock, and reading it from the thread table there would invert
the lock order against `sceKernelRaiseException` and deadlock.

**A loop in the wait.** The predicate becomes `tokens == 0 && !signal_pending()`. A thread woken
for a signal has not had its word written, so it runs the handler and goes back to sleep -
returning `Woken` would hand the guest a satisfied wait whose condition is still false. The lock is
dropped before the handler runs, because the handler is guest code and may wait on the same queue.

**Hooks, not a call upward.** `sync` already takes its word-read as a closure so it can be tested
against a word it owns; running a guest handler is further outside that boundary, so it is
inverted down as a `SignalDelivery { pending, deliver }` pair (D652).

## What it did

```text
before:  ran to the time limit          (silent for 19.7s of 20)
after:   the title's own modules+0x170021b
         read of 0xf8    rdi=0x1e  rsi=0x0  r14=0x480001700210
```

The handler **ran**. The signal number arrived in `rdi`, `rsp` sits in the reentrant stack
`call_guest` reserves, and the handler executed `inc dword [rip+...]`, `cmp edi, 0x1e`, fell
through the `jne`, and then `mov rax, [rsi+0xf8]` - where `rsi` is the exception context and
orbistoun passes null.

**The fault is the design working.** The context layout is unpublished, so a plausible block would
have carried the guest past this instruction on invented fields and buried the question. Null puts
it exactly where the missing knowledge is. Filed as `REQ-20260909T1720Z-4e17`.

Two runs agree on the fault site.

## Surprises

- **A silent hang became a named fault, and the verdict called it `same`.** Nothing moved by the
  metric - identical imports and calls - while the run went from nineteen seconds of nothing to a
  precise instruction with a stated cause. The progress measure counts what a guest *reached*, and
  has no way to say "was hanging, now fails somewhere legible".
- **The refusal is the load-bearing part.** `raise_pending` answers false for an unparked thread
  and the raise returns the placeholder. Returning `0` there would tell a collector a handler will
  run; it then waits forever for an acknowledgement it was promised, and nothing inside the guest
  can tell the difference. The negative test is written first for that reason.

## Next

- The exception context at +0xf8, filed.
- The `selfish-elf` differential, still claimed and still owed.
