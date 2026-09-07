# 424. The protection call learns about the other half of the map

**2026-09-07** - directed, continuing 423

## What was done

`sceKernelMprotect` consulted one of the two places a guest region is recorded. The address
space this crate hands mappings out of was the one it asked; the regions somebody else placed
and reported here - the executable's image, every module's pages - it had never heard of, even
though `region_containing` has consulted both since D446 and is what makes
`sceKernelVirtualQuery` answer for a guest's own code.

So a guest re-protecting its own module was refused, for the one reason that does not apply to
it. It now succeeds where the range lies wholly inside a single reported region, and is refused
exactly as before where it is covered by nothing (D577).

| Title | before | after |
|---|---|---|
| PPSA03416 | 186 imports, a trap in its own module | **192, `flipped`**, `image+0x1389269` |
| PPSA02664 | 197, `image+0x39f7c` | 197, same wall, +2,802 calls |
| PPSA25872 | 141, call budget | unchanged |

PPSA03416's new wall is the address its record already held on 2026-09-04, so the title is back
where it was and past today's regression.

## Why the guard survives

The refusal existed so a typo cannot re-protect this process's own code, and that property is
kept by requiring containment in **one** region rather than the union of several: a range
crossing between two regions spans the gap between them, and a gap is address space nothing
placed. Containment is decided before either authority acts, because `AddressSpace::protect`
files a reservation failure for the report when handed a range it does not own - so asking it
first and falling through would have filed a diagnostic about every success.

`range_within` is pure and saturating; the test asserts both edges and an overflowing length,
and was watched failing with the lookup forced to cover everything.

`orbistoun-kernel` had no entry in the shared test-address table, so it claimed one. That table
exists precisely so two crates cannot pick the same base, and its own test proves the ranges
stay distinct.

## Surprise

**The fix was already measured before it was written.** The forced-return run in D576 predicted
192 imports and `image+0x1389269`; the implementation produced exactly that, with no diagnostic
and no caveat on the record. Being able to check an implementation against a prediction the
diagnostic made is worth more than it sounds - it says the change did the thing that was
measured, rather than something else that happens to move the wall.

## Next

- `image+0x1389269`, a read of `0xa0` through something null. PPSA03416's standing wall, and
  now reached honestly.
- The protection a guest sets on a reported region is not remembered, so a later
  `sceKernelVirtualQuery` describes the region as placed rather than as set. Nothing reads it
  back yet.
- PPSA02664's `sceAgcDriverAddEqEvent` and PPSA25872's unbound module exports, both unchanged
  and both outside this crate.
