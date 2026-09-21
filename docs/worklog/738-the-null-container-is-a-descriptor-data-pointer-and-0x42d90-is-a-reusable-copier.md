# 738. The null container is a descriptor data pointer, and 0x42d90 is a reusable copier

**2026-09-20** — following worklog 737's finding that the faulting `0x42d90` call is gated by a byte in an
object `r14`, this tick pinned what `r14` is and where the null actually lives. It is not a field nobody
filled on the workload object; it is a **data pointer inside a register-group descriptor** that says a
group is present while pointing at nothing.

## 0x42d90 reads its container from arg1, and it is a shared routine

Re-disassembled from the loaded bytes, `0x42d90`'s prologue is `r14d = edx` (arg2), `r13 = rsi` (arg1),
`rbx = rdi` (arg0). The container the memcpy dies on is `r13` = **arg1**, not a field loaded from the
object - so the null is whatever the caller passed as the second argument. And `0x42d90` is not
special to the fault: the function one level up (`0x38xxx`) calls it too, at `0x383d4`, with the
descriptor as arg0 and a stack buffer as arg1. It is a reusable "copy a register group" routine, so the
question is never "what is wrong with `0x42d90`" but "who handed it a null second argument".

## The caller, and the descriptor

The faulting call sits in the function at `0x37ea0`, whose prologue is `mov r14, rsi` - so `r14` is
**this function's arg1**. The call is guarded (worklog 737):

```
0x37f3e  movzx edx, byte [r14+0x5b]   ; group-0 present flag
0x37f45  jz    0x37f53                ; absent -> skip
0x37f47  mov   rsi, [r14+0x18]        ; group-0 data pointer  ->  passed as 0x42d90's arg1
0x37f4e  call  0x42d90
0x37f53  movzx edx, byte [r14+0x5c]   ; group-1 present flag, same shape, +0x20 data...
```

So `r14` is a **register-group descriptor**: per-group present flags at `+0x5b, +0x5c, …` and per-group
data pointers at `+0x18, +0x20, …`. The guest walks it group by group, and for each present group hands
its data pointer to the copier. Group 0 is marked present (`[r14+0x5b] != 0`) but its data pointer
(`[r14+0x18]`) is null. That single inconsistency - **present, but no data** - is the whole fault. On a
console the two agree: a group is present only when its data is there, or the flag is zero and the copy is
skipped.

## Where it goes next

The descriptor `r14` is arg1 to `0x37ea0`, passed down from the `0x38xxx` function. So the descriptor is
built above `0x37ea0`, and the bug is whichever step sets the present flag without setting the matching
data pointer. This is almost certainly the seam between the AGC indirect-register producer (worklogs
726-728, which builds the register packet) and this descriptor that is supposed to reference it: the
producer reserves the packet but orbistoun's skeleton does not populate the descriptor's data pointer to
it, so the flag ends up set and the pointer null. The next trace is the descriptor's construction - where
`[r14+0x18]` is meant to be written and `[r14+0x5b]` is set - one more level up, and it is a much smaller
target than the whole workload object: a present-flag and a data-pointer that must be written together.

## Gate state

No code changed - `ORBISTOUN_PEEK` code dumps and disassembly only. `./bin/orbistoun check` was green as
of worklog 736; identity scan clean. No commit.
