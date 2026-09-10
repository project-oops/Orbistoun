# 435. The library was guest code all along

**2026-09-08** - directed, continuing 434

## What was done

**One command settled three iterations of mystery.** `orbistoun-cli exports` on the replacement
`fakelib/libSceAmpr.sprx` reports 118 exports, and all five known `sceAmpr*` NIDs are among them.
The guest does call the command-buffer constructor and the read-file function; the calls go into
the fakelib's own code, which orbistoun places and the processor runs natively. Guest code calling
guest code, which no thunk sees - principle 7 working as designed (D592).

So the architecture is legible: the dump replaced the vendor library with a shim that implements
the Ampr API in guest code and calls the three `sceKernelApr*` functions in `libkernel`. Those
three are the whole contract, and `ampr_emu.index` is what they answer from.

**The index is parsed and answers in-run.** `orbistoun_fs::amprindex` reads the format decoded in
D591, refuses anything that is not it, and is installed through the hook built for the delivery
experiment:

```text
orbistoun:   the index has /app0/Media/globalgamemanagers as entry 26, 224748 byte(s)
```

## Two negatives, over spaces rather than points

- **All six placements of the identifier and size** across the three out-parameters: the title
  says *"Unknown error occurred while loading"* in every one. That is the whole permutation
  space.
- **Index answer plus delivery plus forced completion**, three permutations: the same.

So the resolve call is not what the fakelib waits on, and this now covers the case where the
planted values are the *right* ones - which the marker experiment of D589 could not.

## Surprises

- **The thing that made it unexplainable was that it was working correctly.** A call orbistoun
  never sees is what "interception is linking, not hooking" guarantees for guest-to-guest calls,
  and three iterations read the silence as the guest declining to call.
- **The oracle earned its keep immediately.** Six permutations, one boot each, graded on a string
  the guest prints rather than on an import count that drifts by one. No re-runs, no ambiguity.

## Next

- The fakelib's command encoding, which is the only remaining unknown and is guest material at
  rest - the same category as the index and a module's import table. Admissible, not attempted.
- Whether the identifier the fakelib wants is the entry's position. Untested and not separable
  from the placement question by the runs made so far.
- The contradiction that has stood for three iterations: a header claiming one command of twenty
  bytes over storage that reads as zero.
