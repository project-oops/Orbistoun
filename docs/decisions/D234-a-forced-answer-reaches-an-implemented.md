# D234 - A forced answer reaches an implemented function, and the implementation still runs


**decided** · 2026-08-25 · found by running out of unimplemented candidates

`ORBISTOUN_RETURN` (D230) was consulted only where nothing was implemented. That is where
the walls looked like they were, so it was the obvious place - and it left the one surviving
explanation for `image+0xafc959` untestable: an **implemented** function handing back
zero-as-success where the guest wants a pointer, which is the D125 class and the reason
`STUB_RETURNS` exists at all.

Every unimplemented import in that title is now eliminated as the source of the missing
base - all seven return values in one run, `7 answered`, fault byte-identical - so what
remains is implemented or is not a call.

**The implementation still runs; only its answer is replaced.** Skipping the handler would
suppress its side effects too, and then a moved fault says only that the program was
changed, which is already known. Running it and discarding the return keeps every other
observable identical, so a moved fault means the *answer* mattered.

The lookup is one atomic load that short-circuits on any ordinary run, so the busiest import
in the corpus - implemented, called ninety-nine million times - pays a predictable branch
and no table lookup (principle 9).

### The positive control

Forcing `memalign` to answer `0x700000000000` moved the fault to **`0x700000000010`** - the
dye plus sixteen, at a different instruction. The mechanism demonstrably works, which is
worth more than it sounds: every negative result this family of diagnostics produces is only
as good as the evidence that it *can* produce a positive one.

That dye collided with the stubs region, so the guest died before reaching the wall. Re-run
with `ORBISTOUN_MAP` reserving the dyed address, the guest ran on and hit the original wall
unchanged - so `memalign` is eliminated with the program behaving normally, rather than by a
run that crashed early somewhere else (D232 is the same hazard from the other direction).

**Pair a forced pointer with a mapping.** A dye that is not mapped answers a different
question than the one being asked.

