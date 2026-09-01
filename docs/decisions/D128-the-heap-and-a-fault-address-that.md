# D128 - The heap, and a fault address that identified itself

**decided** · 2026-08-20

`malloc` was not implemented, so it answered the placeholder error code. A title took that
as its buffer, handed it to `memset`, and our `memset` faithfully wrote there.

**The fault address was the proof, not a hint.** `write to 0x7fff0001` *is*
`GuestError::Unimplemented.as_raw()`. Principle 3 put placeholder codes in a range no real
value occupies specifically so a stub leaking into guest-visible behaviour would be
obvious, and it paid for itself here: the faulting address named its own cause. The
earlier `write to 0x7fff0119` was the same value plus a field offset.

The second half of the evidence was the instruction pointer - `0x7ffc…`, host address
space, not the guest's. A guest that faults inside *our* code is a guest we handed
something bad.

Implemented `malloc`, `calloc`, `realloc` and `free` over the host allocator, which is
legitimate rather than lazy: the address space is identity-mapped, so a host allocation is
a guest allocation at the same address, and a private arena would buy nothing but a second
allocator to get wrong. A sixteen-byte header carries the size, because `free` is given
only a pointer and sixteen is the alignment `malloc` owes any type on x86-64.

**Result: 29 distinct imports reached became 37**, and the title now gets to
`sceVideoOutOpen`, `sceUltMutexLock` and `sceUltConditionVariableSignal` - display and
synchronisation - before failing on a null it does not check.

### The progress metric misled here, and that is worth recording

D080 measures progress by the faulting instruction pointer. By that measure this run went
**backwards**: `image+0x13514` to `image+0xf2f6`.

It did not. The previous fault was inside *our* `memset` with the guest's own position
unrecorded, and the new one is a different code path entirely - reached only because the
allocation succeeded. An instruction pointer compares two positions in one path; it says
nothing useful across two different ones.

**Distinct imports reached is the better signal when the path changes.** Neither is right
alone: a guest can call more imports while getting less far, and can get further while
calling fewer. Both belong in the progress block, and the one that moved should be named
rather than reduced to a single verdict.


