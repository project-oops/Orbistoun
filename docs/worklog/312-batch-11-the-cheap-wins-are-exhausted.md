# 2026-09-02 - (/loop) Bulk port batch 11: two inits resolved, and the cheap wins are exhausted

```
documented   715 needed, 266 missing   ->   715 needed, 264 missing
```

Two functions. The batches have run their course, and this records why as well as the result.

## The check that was supposed to find easy wins found none

Batch 10's lesson was "grep before writing - *missing* means resolves to nothing, not unwritten".
So this tick ran that across the whole gap at once: for each of the 266 names, does `fn <name>`
exist anywhere in the crates?

Three hits. **All three were false positives**, and the reason is worth keeping:

- `sync` matched a corpus-synchronisation function with nothing to do with POSIX `sync(2)`.
- `pthread_rwlock_init` and `pthread_barrier_init` matched Rust functions that implement the
  **vendor** signatures - they are registered as `scePthreadRwlockInit` and
  `scePthreadBarrierInit`, and they read the trailing name argument.

**The Rust identifier is not the guest symbol name.** The registration table is the truth, and a
grep over `fn` names is a heuristic that will happily point at a function whose signature is
wrong for the caller. Following it here would have re-created precisely the D385 fault - a
POSIX-arity call reading whatever the guest left in the register the vendor form uses for a name.

So: **zero genuinely written-but-unwired functions** among the 264 remaining. Everything left
needs writing, and a good share of it is in the refused categories.

## What the false positive did turn up

Chasing it found the *correct* resolution to a standing refusal. Batch 4 refused to delegate
`pthread_rwlock_init` and `pthread_barrier_init` to their vendor twins because the arities differ
by the name argument - right then, and still right. But the project had already solved this shape
for the rwlock: a separate `posix_pthread_rwlock_init` entry point that never reads `args[2]`,
with a comment explaining the bug it exists to avoid.

The barrier had no such twin, so it has one now, split the same way: `pthread_barrier_init`
(vendor, four arguments) and `posix_pthread_barrier_init` (POSIX, three), sharing one
`barrier_init` once the name has been resolved by whoever had one. Both bare POSIX spellings are
now declared and delegated to the POSIX-signature entry points. The refusal is resolved rather
than reversed - which is the distinction that matters, because the reason it was refused has not
stopped being true.

## Where this leaves the batches

Ten batches took the documented gap from **452 to 264** - 188 functions, all written from
specifications and tested against them, no guest run involved. The last three ticks moved it by
30, 1 and 2. The remaining 264 are the ones that need real implementation work or sit behind a
standing refusal, so the per-tick yield from here is a handful.

**Recommendation: stop the batches** and spend the effort on the two structural items instead,
both of which have been queued for several ticks and neither of which is blocked on knowledge:

1. **The TitleOwn loader.** `sceKernelLoadStartModule` hands back a handle without loading
   anything, and nothing registers a second module's exports, so PPSA02664's own
   `Il2CppUserAssemblies.prx`, `PS5Util.prx` and three plugins never load. Unbuilt code, not an
   unknown, and it unlocks PS5Util's 36k corpus calls.
2. **The obSCEne differential.** obSCEne and `orbistoun-cli serve` answer the same command
   protocol, so one driver can diff real hardware against this emulator live. It is built, it
   caught sixteen libc failures once, and it has never been run against any of this work. Around
   ninety functions have now been written without the only check that catches *implemented but
   wrong* - and today already showed, twice, how long a wrong-but-plausible result survives when
   nothing is looking for it.

## State

clippy `--tests` clean, fmt clean, kernel/posix tests pass, identity scan clean, nothing
committed.
