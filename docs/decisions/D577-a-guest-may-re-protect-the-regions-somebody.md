# D577 - A guest may re-protect the regions somebody else placed for it

**Status:** measured
**Date:** 2026-09-07

## The refusal, and why it was there

`sceKernelMprotect` changed protection through `AddressSpace::protect`, which refuses a range
the address space does not own. That guard is right and stays: without it a typo re-protects
this process's own code, and the failure surfaces as an unrelated crash.

But **that address space is only one of the two places a guest region is recorded.** The other
is the set of regions somebody else placed and reported to this crate: the executable's image
(D446) and every module's pages (D489). `region_containing` has consulted both since D446,
which is how `sceKernelVirtualQuery` and `sceKernelIsStack` answer for a guest's own code. The
protection call never learned.

So a guest re-protecting its own module was refused for the one reason that does not apply to
it - this crate had not mapped the memory, and this crate is not the only thing that maps.

## What it cost

PPSA03416 asks for 256 MiB of write access across its own module, gets `0x80020016` (`EINVAL`
under the measured vendor encoding), **checks the result**, and takes its failure path - which
calls a routine its compiler believes never returns. It returns, onto the `int3` placed after
it, with `mov ebx, 0x8002000c` (`ENOMEM`) as the next instruction (D576).

| | refused | answered |
|---|--:|---|
| imports | 186 | **192** |
| fault | `modules+0x2e68020`, a trap | `image+0x1389269`, read of `0xa0` |

`image+0x1389269` is the wall this title's record already carried from 2026-09-04. The title is
recorded `flipped` at 192 imports, 100% standing.

PPSA02664 - the title the original implementation was written for - stays at 197 imports and
its own wall, with 2,802 more calls. PPSA25872 is unchanged.

## The rule, and how the guard survives it

The range must lie wholly inside **one** region this process placed for the guest. Ownership
by this crate's own address space is tried first; failing that, the reported regions are
consulted and the protection is applied directly.

- **One region, not the union.** A range crossing from one region into another spans the gap
  between them, and a gap is address space nothing placed. Requiring a single region keeps the
  original property - a range that reaches into memory nobody placed for the guest is refused -
  without enumerating what must not be touched.
- **Containment is checked before either authority acts.** `AddressSpace::protect` files a
  reservation failure for the report when handed a range it does not own (worklog 284), so
  asking it first and falling through would have filed a diagnostic about every successful
  protection of a module's own memory.
- **A host refusal is still a refusal.** Where the region is the guest's and the platform
  declines, the guest is told `EINVAL` rather than success. The whole value of this call is
  that the answer is true, which is what the last two days demonstrated.

`range_within` is pure and saturating, so the containment rule is testable with no mapping in
existence - the split principle 8 asks for, and the one `region_containing` already had. The
test asserts both edges and a length that would overflow, and it was watched failing: with the
lookup forced to cover everything, the case that a region nobody reported is refused fails.

## What this does not establish

**That the protection is remembered.** The reported regions carry a span and nothing else, so a
later `sceKernelVirtualQuery` describes the region as it was placed rather than as the guest
last set it. Nothing reads protection back yet; when something does, that is where to look.

**Nor that the high bits are understood.** Unchanged from the original entry: the value carries
GPU-access and cache bits for which there is no citable layout, and they are still ignored
rather than decoded.

**Nor what PPSA03416 does next.** `image+0x1389269` reads `0xa0` through something null. That is
where it stood on 2026-09-04 and it is not this entry's subject.
