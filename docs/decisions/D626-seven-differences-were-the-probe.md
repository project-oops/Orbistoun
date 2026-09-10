# D626 - Seven of the differences were the probe declining to call

**Status:** measured
**Date:** 2026-09-08

## An allocator that worked, refused, and was never reached

`020-memory/allocate` fails here and passes on a console. Called directly with obSCEne's own
arguments, orbistoun's `sceKernelAllocateDirectMemory` answers `0x0` and hands back `0x10000`. A
forced dump, once it worked (D623, D625), captured nothing: the thunk is never entered.

The answer is in obSCEne's own source, which this project holds. Its payload build does not call
the libkernel import at all. It carries `OBS_WEAK` definitions that go straight to a raw syscall -
`sceKernelAllocateDirectMemory` is 572, `sceKernelVirtualQuery` is 603, the `sceKernelUsleep`
fallback is 240 - and every one of them goes through `obs_invoke_syscall`, which begins:

> In a native title/eboot process, direct syscall instructions outside libkernel trigger an
> unhandled kernel exception. Avoid them if dynamically linked symbols are present or no syscall
> gadget is known.

```c
if (obs_get_payload_args() == NULL && s_libkernel_syscall_gadget == 0) {
    return -1;
}
```

Under orbistoun there are neither. The payload is entered as a bare ELF with no payload-args
struct, and `obs_bootstrap_title_output` - the path a title takes - fills only the two output
pointers and never looks for a gadget. **So the call is not made, `-1` comes back, and the check
reports the platform refusing.**

`0xffffffff` is `(uint32_t)(-1)`, and it is exactly what five of them record:

| check | recorded | says |
|---|---|---|
| `020-memory/allocate` | `0xffffffff` | allocation was refused |
| `020-memory/allocate-main` | `0xffffffff` | mapping allocated main direct memory failed |
| `020-memory/virtual-query-stack` | `0xffffffff` | virtual query on stack address refused |
| `020-memory/virtual-query-text` | `0xffffffff` | virtual query on code address refused |
| `050-time/usleep` | `0xffffffff` | a short sleep was refused |

Two more follow from the same declined sleep rather than carrying the code themselves:
`120-measure/identify-clocks` ("the sleep used to identify the clocks was refused") and
`120-measure/sleep-fidelity` ("a sleep returned far sooner than asked" - it returned at once,
having done nothing). And the `pass -> skip` cascade behind them - `020-memory/map`, `release`,
`unmap`, four `018-relational` checks, `130-layout/memory-type`,
`150-memory-map/after-allocation` - is all one refused allocation that never happened.

**Seven of the sixteen substantive divergences, and every skip below them, are not orbistoun's.**

## What this is an instance of

A message naming a cause must come from the branch that determined it. "Allocation was refused"
comes from a branch that only knows `rc != 0`, and the branch that returned that `rc` knows it
never issued anything. That is principle 3 exactly, on the probe's side of the boundary - and it
cost a day here, because a differential is only as honest as the weakest verdict in it.

Filed as `REQ-20260908T1620Z-4c1e` on the shared request bus: either make the wrappers resolve
`skip` naming the absent syscall route, or emit one record per leg saying whether a route existed
at all. Either is enough for a consumer to tell which of its fails are real.

## What orbistoun could do about it, and why not yet

Orbistoun already has the machinery. It builds a syscall gadget, it dispatches numbers through a
harvested table, and `getpid_export_slot` already puts a jump to that gadget at **byte ten of
getpid** because that is the convention every open-toolchain payload uses (D400, D407) - but only
under a firmware skeleton, which this run has none of. It also serves a handoff block through
`ORBISTOUN_ENTRY_ARGUMENT=handoff`.

That was tried, and it is worse: the payload bootstraps, resolves through the block, and faults on
an instruction fetch from `0x0` after 1,731 calls instead of running to completion after 394,585.
`BACK`, and by a long way. Handing a guest a resolver whose `+0xa` is the middle of a thunk rather
than a syscall instruction is handing it a wild call, so the honest options are a firmware run or
giving the by-name stubs the vendor stub shape - and neither is a change to make while the
measurement it would be judged by is the one this record just corrected.

Recorded rather than fixed. The differential is worth more with seven false defects removed from
it than with an intervention that moves a wall by a route nobody has checked.

## An eighth, found by the dump the same evening

`015-sync/condvar-wakes-a-waiter` is `pass -> partial` here with **"the waiter never reached the
wait"**, which reads as a condition-variable or thread-creation defect and is neither.

The check starts a waiter thread, sleeps **50 ms** - *"generous on purpose"*, its own comment says,
because too short "looks identical to a wakeup that does not work" - and then reads a state word
the waiter sets. Under orbistoun that sleep is `sceKernelUsleep`, which is one of the weak
definitions above: no gadget, no payload args, `-1` at once. **The generous window is zero.**

Measured rather than reasoned, with the forced dump from D625:

```text
ORBISTOUN_DUMP=sceKernelUsleep,scePthreadMutexTrylock  ->  armed 3 of 1040 stub slot(s)
  ! libkernel::scePthreadMutexTrylock was asked about, and here is what it was passed
      arg0 = 0x600000800a40 -> stack+0x800a40 = 60 01 00 00 2d 5e 00 00 …
  (nothing for sceKernelUsleep)
```

So the waiter thread **does** run and **does** reach its trylock, and the sleep meant to give it
time never reaches this emulator at all. That is sufficient on its own to produce the verdict.

Whether the condition variable itself also diverges cannot be told until the sleep works, and
saying otherwise either way would be the failure this record is about. Recorded at that strength.

Eight, then, and the point stands: **a differential is only as honest as the weakest verdict in
it**, and one declined call was producing findings in three unrelated sections.
