# D167 - The video-output handle, and what a large negative sweep is worth


**decided** · 2026-08-20

A quarter of a million candidate names proposed against seventeen unnamed hashes; **one
confirmed**: `libSceVideoOut::0xb9b56b04b654a0ac` is `sceVideoOutRegisterBuffers2`.

That name matters more than the count suggests. It was observed being called with
`0x7FFF0001` as its first argument - the guest passing our own unimplemented code back to
us as a video-output handle, because `sceVideoOutOpen` had answered with it. Naming the
call is what turns "an unnamed hash got a bad handle" into "the guest registered display
buffers against a handle we never gave it".

`Buffers` was added to the object vocabulary so the repository derives the name from its
own grammar rather than from this session having found it. **A name confirmed and not
written into the vocabulary is an assertion again** (D155).

### The negative result is the more useful half

Sixteen hashes did not match, across `libSceUlt`, `libSceAgcDriver`, `libSceAmpr`,
`libScePad`, `libkernel` and `libc` - and the sweep covered every combination of
forty-odd verbs against fifty-odd objects with eight tails, in both verb-object and
object-verb order, for every module observed.

So those names are **not** built from the vocabulary this project has. That is worth
knowing precisely: it says extending the current word lists is not the way to reach them,
and the effort belongs in a different pattern instead. The most-called one,
`libc::0xa75420e43cad1cdc` at seventy-six calls, sits consistently between
`sceKernelAllocateMainDirectMemory` and `sceKernelMapNamedDirectMemory` with a stack
address as its argument - which is the shape of something building the *name* string the
map call takes. A `printf`-family name, in other words, and none of the obvious spellings
matched.

The `sceKernel<posix-name>` pattern flagged under D155 is the outstanding idea here: the
vendor wrapping standard names, which would derive from the FreeBSD list already harvested
rather than from the invented vocabulary. It needs a solver change rather than a data edit,
and it is the next thing worth building for naming.

