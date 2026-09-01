# D244 - An illegal instruction has an address, and it is not one the guest asked for


**decided** · 2026-08-25 · the last unexplained result in a sweep of every import

Planting a sentinel at `arg1` of `scePthreadMutexattrInit` reported `Moved` - *"something
downstream moved rather than the address being computed from it"*. Both sentinels produced
the **identical** fault address, which is the tell: nothing was computed from either value.

The fault was an **illegal instruction**. Those exceptions carry no address parameters, so
the reporter fills the address field with the instruction pointer - a real number that is
not somewhere the guest tried to touch. `fault - sentinel` over it is arithmetic across two
unrelated things, and its inconsistency was being read as a weak finding rather than as a
category error.

`FaultSite::TOUCHED` and `FaultSite::AT_THE_INSTRUCTION` now hold the two lists, published
by the crate that defines the field, and `orbistoun-worker` emits from them rather than
writing the strings again. `Finding::Derailed` reports what actually happened: the plant
broke control flow, with how far the guest got attached.

**The correction inside the correction is the part worth keeping.** The first version also
treated *reaching fewer imports* as disqualifying, imported from the axis sweep where it is
exactly right. Run against the live title it reclassified five correct findings. The two
sweeps ask opposite questions: poisoning a region and getting less far means the poison
broke the run, while planting a sentinel in a pointer and getting less far is what
**success** looks like - the guest now dies at the sentinel instead of surviving to the
wall that prompted the experiment. Only `touched` gates the arithmetic. Pinned by
`getting_less_far_does_not_disqualify_a_consistent_offset`, which was first written
asserting the opposite.

Every import of the live title now has a definite classification - 5 dereferenced pointers,
1 derailed run, 17 unmoved, and **no `Moved` at all**. The negative conclusion is unchanged
and stronger for having nothing ambiguous left in it.

