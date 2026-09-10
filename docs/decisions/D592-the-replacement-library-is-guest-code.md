# D592 - The replacement library is guest code, and the index is read

**Status:** measured
**Date:** 2026-09-08

## The three-iteration mystery, settled in one command

`sceAmprAprCommandBufferConstructor` and `sceAmprAprCommandBufferReadFile` are imported by
PPSA03416 and never called - established under `ORBISTOUN_RESOLVE=all`, which gives every import
a reporting stub, so it is not a resolution failure. Three iterations treated that as the sharpest
unexplained fact about this wall.

`orbistoun-cli exports` on the replacement library answers it:

```text
titles/PPSA03416-app0/fakelib/libSceAmpr.sprx  -  118 exports
```

and **all five known `sceAmpr*` NIDs are among them**.

So the guest does call them. The calls go into the fakelib's own code, which orbistoun placed as
a module and the processor executes natively - guest code calling guest code, which the loader
resolves and no thunk ever sees. That is principle 7 working exactly as designed: interception is
linking, and nothing links here.

**Which makes the architecture legible.** The dump replaced the vendor's `libSceAmpr` with a shim
that implements the command-buffer API in guest code, builds the buffer itself, and calls the
three `sceKernelApr*` functions in `libkernel` - the ones orbistoun does stub. Those three are
the whole contract, and `ampr_emu.index` (D591) is the table they are meant to answer from.

## The index is parsed, and it answers in-run

`orbistoun_fs::amprindex` reads the format D591 decoded. It refuses rather than salvages: an
unknown version, a wrong stride, a truncated file or a bad magic all parse to nothing, because a
size invented from half a table is exactly the plausible answer principle 3 refuses. The negative
tests were watched failing.

Installed from `orbistoun-worker` through the hook D589 built, for the same reason - the index is
a file, and the filesystem is a sibling subsystem.

```text
orbistoun:   the index has /app0/Media/globalgamemanagers as entry 26, 224748 byte(s)
```

The guest's exact question, answered from the title's own data, in the run that asks it.

## Two clean negatives, over spaces rather than points

**Where the answers go is not the gate.** `ORBISTOUN_APR_ANSWER` writes the identifier and the
size through a named pair of arguments; all six permutations of the three out-parameters were
run, and the title says *"Unknown error occurred while loading"* in every one. That is the whole
space, not a sample.

**Nor is the full stack.** Index answer plus delivery plus forced completion, three permutations:
the same message every time.

So the resolve call is not what the fakelib is waiting on, which agrees with the marker
experiment of D589 and now covers the case where the markers are the *right values*.

## What is left, stated exactly

The fakelib builds a command buffer whose header claims one command of twenty bytes over storage
that reads as zero, and submits it. **Where it wants the bytes is the only remaining unknown**,
and it is an unknown about the fakelib's own encoding rather than about the platform.

That encoding is in the fakelib, which is guest material at rest - the same category as the index
and as a module's import table. Reading it is admissible and has not been attempted.

## What this does not establish

**That the index is complete for this purpose.** It gives a path, a size and a position. Whether
the identifier the fakelib wants is that position is untested - all six placements failed, which
is consistent with the position being wrong *and* with the placement being wrong, and the two are
not separated.

**Nor that the fakelib's ReadFile succeeded.** The header's count says one command and the
storage reads as zero, which is a contradiction nothing here resolves. A ReadFile that failed
part-way and left the count set would look identical.
