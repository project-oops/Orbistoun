# D451 - PPSA21564 boots once the sceLibcMspace allocator family (and bcmp) answer real values


**measured** - 2026-09-01 (user-directed /loop)

Surveying the non-PPSA02664 titles for loop-tickable work (D450 left PPSA02664's tlsf wall as a
focused, non-loop task), PPSA21564 stood out: it faulted `write to 0x7fff0001` at 131 calls -
orbistoun's own `Unimplemented` placeholder (`0x7FFF_0001`) written through as a pointer. The call
immediately before the fault was `sceLibcMspaceMalloc(0x0, 0x48)`: an allocator that, unimplemented, handed
back the placeholder, which the guest took as its 72-byte buffer and wrote into. This is the `malloc` wall
(D128) under another name.

**The fix.** `sceLibcMspace*` is the platform's exposed Doug Lea `mspace` allocator - an *mspace* is an
independent heap arena, and the family (`Malloc`/`Calloc`/`Realloc`/`Free`) is dlmalloc's
`mspace_malloc`/... with the arena as a leading argument. Implemented in `orbistoun-libc` by delegating to
the heap this crate already owns, **ignoring the arena handle**. That is sound because a guest only touches
mspace memory *through this same family*: `allocate` writes a block header that `free` reads back
independent of any arena, so an allocation and its release agree whether or not the handle is real. The two
arguments of `sceLibcMspaceMalloc` (space, size) were confirmed by argument dump; the siblings follow the
published dlmalloc shape. A guest that created its own mspace over a specific region and then checked which
addresses came back would notice - nothing observed does, and that is a later refinement.

`bcmp` was implemented in the same pass: it is `memcmp` (its contract is only equal-vs-not-equal, which
`memcmp`'s result satisfies), it was called 16k times in one boot, and an unimplemented nonzero placeholder
reads as "always differ" - which can wedge a comparison loop.

**The result, measured and repeatable.** PPSA21564 goes from a 131-call fault to **~550k calls with no
fault** - it now runs to the call budget instead of crashing. Stub rate fell from ~7% to ~4% once `bcmp`
was also answered. The remaining hot-loop wants are `scePthreadGetthreadid` (22.5k calls, needs the thread
registry) and a few singletons (`_ZSt14_Random_devicev`, `_init_env`, `sceKernelGetGPI`,
`pthread_key_create`); none is fatal within the budget.

`fmt`/`clippy`/`cargo test`/the knowledge audit pass for the touched crates. The change is additive - new
symbols only - so titles that do not import the mspace family are unaffected.
