# D623 - The dump answered a different question

**Status:** measured
**Date:** 2026-09-08

## Asking about one import and getting a list without it

`ORBISTOUN_DUMP=sceKernelAllocateDirectMemory` on the conformance payload produced a captured-
arguments list containing `libc::sqrt`, `libc::fabs`, `libc::floor` and `libc::ceil` - and not the
import that was named. The same flag works on a title.

Two things were wrong, and the second only became visible once the first was fixed.

## The forced list added to the default set instead of replacing it

Arguments are dumped for every import nothing implements, plus any named with `ORBISTOUN_DUMP`.
The buffer holds `MAX_DUMPS = 512` entries, six per call, and its own comment says *"the
interesting ones all arrive early"* - which is true of a title and not of a guest that calls
hundreds of unimplemented functions before reaching anything worth asking about.

So the payload's unimplemented maths library filled the buffer, and the forced dump - the one
somebody typed a variable to get - was dropped.

**A forced list now narrows the dump to itself.** Somebody who names an import is asking about
that import; the default set is what you get when you have not asked.

## And a dropped dump printed as nothing at all

`dump_arguments` returned silently once the buffer was full. A dump that was wanted and not taken
is indistinguishable, in the report, from a call that passed nothing worth showing - so the tool
answered, and the answer omitted the thing it was asked about, and said so nowhere.

Counted now, and reported:

```text
orbistoun: 45 argument dump(s) wanted after the buffer was full -
           name an import with ORBISTOUN_DUMP to spend the room on it
```

The same shape as D588's dropped readable ranges, which prints for the same reason: *a dropped
range and a wrong pointer print identically*. Here a dropped dump and an uninteresting call print
identically.

## What it did not answer

With the forced dump given the whole buffer, `sceKernelAllocateDirectMemory` still produced
nothing - which now means something. **The payload never calls it through a thunk.** obSCEne's
`020-memory/allocate` reports "allocation was refused" and orbistoun's implementation, called
directly with the probe's exact arguments, answers `0x0` and hands back `0x10000`.

So the refusal happens on a path that does not reach orbistoun's function. The next question is
which one - the run resolves the name to a by-name stub at `0x700000006980`, and whether that stub
and the import-table entry share an index is the thing to establish.

Three tools in one session have reported confidently while omitting what was asked
(D613's ring, D615's surplus registers, this). Each was a *missing "I do not know"* rather than a
wrong value, and each cost more to find than the fix.
