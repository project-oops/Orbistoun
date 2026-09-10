# D601 - Guest-visible blocks belong in the memory crate

**Status:** decided
**Date:** 2026-09-08

## The question the ninth leak posed

`orbistoun_fs::open::next_handle` handed a `FILE *` from the host heap, so it was a different
address every run - the leak D584 fixed at eight sites and missed here because that search was
scoped to one crate.

It could not be fixed where it was found. The shared allocator lived in `orbistoun-kernel`, and
`orbistoun-fs` is a **sibling subsystem** - the relation that keeps the clocks in `orbistoun-hle`
rather than in either library that reads them (D536). The options were to move `blocks` down the
spine, to add a sideways dependency, or to install it through a hook.

**Recorded as a structural question rather than answered by whichever import was easiest to add**
(D597), and settled deliberately: `blocks` moves to `orbistoun-mem`.

## Why there

Guest-visible memory is what `orbistoun-mem` is for. Principle 4 puts guest memory access in it
by construction - *"if a subsystem crate needs a raw pointer, the abstraction is in the wrong
place"* - and a block a guest dereferences is exactly that.

Both subsystems already sit above it on the spine, so **nothing moves sideways and no dependency
is added to reach it**: `orbistoun-kernel` already depended on `orbistoun-mem`, and
`orbistoun-fs` gains a downward dependency, which the spine permits.

`GUEST_BLOCK_BASE` keeps its address and changes owner in `docs/ADDRESS_MAP.md`, which the map's
own test checks against the source.

## What it fixed, and what it did not

A `FILE *` repeats now, and so does every other handle - `scePthreadSelf` answers
`0x5e2d00000a20` in three consecutive runs.

**`orbistoun-abi` still hands the guest host heap addresses.** `process_argument_block` and
`sentinel_argument_block` are given to the guest at entry and are `Box::leak`, and that crate has
**no orbistoun dependencies at all** - `orbistoun-mem` appears there only under
`[dev-dependencies]`. Adding a real one to reach the allocator is a structural change to a
deliberately leaf crate, which is a different decision from this one and is not smuggled in with
it. The comment at the site says so.

So the count was never nine. It was nine in the two crates anybody had searched.

## What this does not establish

**That handles repeating makes a run repeat.** Three runs immediately after this gave 193, 192,
193 distinct imports. The drift is smaller than it was and it is not gone, and D600's claim that
"the determinism work landed" is withdrawn there - five agreeing runs is not a measurement, as
that entry's own caveat said before its headline contradicted it.

**Nor that `orbistoun-mem` is where a *guest-visible* thing generally belongs.** This is one
allocator for opaque handles. A structure with a layout the guest reads through is a different
kind of object, and putting it here because this worked would be the reasoning this entry exists
to avoid.
