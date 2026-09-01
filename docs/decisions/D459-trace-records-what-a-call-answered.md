# D459 - The call trace records what each call answered, not only what it was asked


**measured** - 2026-09-01 (user-directed /loop: build the watchpoint feature and crack the refers)

Every diagnostic on the dispatch path captured what the guest passed *in* - the first argument
(`RING_ARG0`), the pointer contents a dump reads, the stack alignment. None captured what our
own implementation handed *back*. So the failure this project hits most - an implemented
function answering a wrong value the guest then trusts, the D125 class - was invisible in the
one record a person reads at a wall: the tail showed a call happening and not its result. The
lead on both of PPSA02664's walls had to be reconstructed by reading the code and guessing,
which is the tool asking a person to do its job (the same complaint D198 answered for arguments).

**The feature.** `on_guest_call` in `orbistoun-thunk` now records the answer beside the
argument. Two parallel rings, allocation-free like the others: `RING_RET` holds the value and
`RING_RETURNED` a `Release`-written flag, because any `u64` is a legitimate answer and zero is
the commonest one (`OK`) - a slot still running, or one whose guest faulted the instant the call
returned, must read as **unknown** rather than as zero. The dispatch (float handler, integer
handler, forced value, stub placeholder) was funnelled into one `resolve` helper so every
possible answer leaves through a single point, which is where the return is recorded - no return
path can be added later that forgets to. `RecordedCall.ret` and `TracedCall.returned` are
`Option<u64>`; the report renders `-> 0xNN` on the tail and the "just before" evidence only when
it is known. Cost is bounded to the first `MAX_RECORDED_CALLS` calls, exactly as `arg0` is, so
the ninety-nine-million-call hot path pays nothing.

**Verified, and it named both walls at a glance - one of them against my own prior guess.**
PPSA02664 faults non-deterministically at one of two sites (D450). With the return column:

- `image+0xb14be3`: `libc::_Getpctype(0x34) -> 0x7fff0001`. `0x7fff0001` is our `Unimplemented`
  placeholder - so an unimplemented, pointer-returning ctype function answered a placeholder and
  the guest dereferenced it as the table pointer (`read of 0x7fff00cf`). The D125 class, named by
  its own return.
- `image+0xafcc08`: `sceKernelMapDirectMemory(...) -> 0x7fff0004` while
  `sceKernelReserveVirtualRange`, `sceKernelVirtualQuery` and `sceKernelAllocateMainDirectMemory`
  all answered `0x0` (`OK`). `0x7fff0004` is `NoMemory`: our `map_named_direct_memory` took its
  `space.reserve(...).is_err()` branch. The guest **reserved the range first** and then asked map
  to place physical memory *at that reserved address*, and our map tried to reserve it a second
  time and conflicted with the reservation that already held it. That overturned the previous
  turn's lead - a wrong pool size or an address-space mismatch - in a single run, which is the
  argument for the feature: the wrong value is almost always the one we *answered*, and nothing
  recorded it until now.

The fix for the map wall (commit into an already-reserved range rather than reserve afresh) and
the `_Getpctype` implementation are separate units; each needs the "does the wall move" second
observation before it counts as more than a lead (the D224/D226/D227 rule). Provenance-clean:
this reads a value the handler already computed and returned, never guest code.
