# D604 - A placement that would not step past a conflict

**Status:** measured
**Date:** 2026-09-08

## The record found the bug the record had

Refusals became visible (D603) and the first thing they showed was two entries sharing one call
ordinal, one of them claiming a **guest stack address** as a mapping base. Both were mine:

- The wrapper recorded `args[0]`, which for `sceKernelMapNamedDirectMemory` is the `void **` the
  answer is written back through - a stack address - not a base. It reads through it now, for the
  address the guest actually requested.
- A note at the wrapper *and* at the failing branch inside put one refusal in the list twice.
  One place, which is the rule the successful side already follows.

Neither was visible until the record was read. A diagnostic that has never been looked at is a
diagnostic nobody knows anything about, which is the same sentence principle 3 already applies to
guards.

## What it then showed

```text
82  call 463210  0x0000000000000000 +0x1fe0000  arena  during sceKernelMapNamedDirectMemory  REFUSED: answered 0x7fff0004
```

Base zero means **the guest expressed no preference** - it asked for thirty-two mebibytes
*anywhere* - and orbistoun answered `NoMemory`.

The arena's counter steps an address rather than the map, so a base it hands back can already be
held by a range the guest reserved at a hint inside the arena.
`sceKernelReserveVirtualRange` has retried past exactly that since it was written, sixteen times,
with a comment explaining why. **`sceKernelMapNamedDirectMemory` had the identical hazard and no
mitigation**: it took one address and gave up.

It retries now, and only when the guest named no address - a guest that asked for somewhere
specific is still refused, because quietly moving it is the corruption the surrounding comment
already refuses. The count is named once and shared, so the two paths cannot drift apart again.

## Measured

| | before | after |
|---|--:|--:|
| refusals per run | 2 to 4 | **0** |
| distinct imports, four runs | 192, 192, 193, 193 | **193, 193, 193, 193** |

The import drift D602 chased is gone, and it was never a guest branch: it was how many
thirty-two-mebibyte requests orbistoun declined, with the guest doing less afterwards as the
ordinary consequence.

## What this does not establish

**That the run repeats.** The mapping sequence still gives twenty, one and one identical entries
out of fifty across three pairs. The coarse signal is stable and the fine one is not, which is
where D600 left it and where it remains.

**Nor that sixteen is the right number.** It is what the other path uses, and sharing one constant
is worth more here than choosing a better one - a retry limit that differs between two paths doing
the same thing is the drift this entry exists to stop.

**Nor that a conflict should happen at all.** Retrying past one is a mitigation. An arena whose
counter knew what the guest had already reserved would not conflict, and that is a different
change to a different place.
