# D493 - The null is `.bss`, and binding made the answer invisible

**measured** - 2026-09-03 (segment arithmetic, then a trace with nothing in it)

Two findings that belong together: the first says exactly what the wall is, and the second says
why this build cannot currently see past it.

## The null is `.bss`, not a `.data` the loader failed to copy

The loader already knew, and nothing surfaced it. `PlacedSegment` records the file-backed and
zeroed halves of every segment separately, which **is** the `.data`/`.bss` split. Printed:

```text
segment Il2CppUserAssemblies #4 0x480001d68000  0x194a6c copied  0x26e0dc zeroed  flags 0x6
```

The copied part ends at `0x480001efca6c`. The faulting `rsi` is `0x480001f0c330` - **`0xf8c4`
past it**, inside the zeroed run.

So orbistoun copied everything the file held. That global is zero *because `.bss` is zero*, and
the question is not "what went wrong in the loader" but **"what was supposed to write it"**.

That rules out the last hypothesis that would have implicated the placement path, and it took
one arithmetic comparison against numbers the loader had all along.

### And the caller is one of the four

`+0x13d5f00` is `0x70` into the export at `+0x13d5e90`, which is `0x9fdbed4fe0989d70` - one of
the five imports the eboot binds into this module. It allocates 48 bytes through
`operator new`, calls `+0x13dca44`, and that reads the empty global.

## Binding removed the calls that matter from the trace

D489 bound the eboot's imports to real addresses inside the module. It works. The consequence
was not thought through:

```text
Il2Cpp entries in the trace: NONE
```

Before binding, `0x6f8b9da539afc9af` appeared as **222 calls** with `il2cpp_init` and
`il2cpp_init_utf16` as its first argument - which is how the resolver was found at all. It now
resolves to a real address, so the guest calls it directly and no thunk sees it.

**This is principle 7 working exactly as written.** *Interception is linking, not hooking* - so
what is observed is precisely what resolves to a stub. Binding an import to real code is the
correct answer and it is also the act of switching the instrument off for that call.

The two are not in tension by accident; they are the same decision seen from two sides. Worth
naming before somebody reads a shrinking call count as a regression: **the trace measures what
orbistoun answers, not what the guest does**, and the more of a title's own code runs, the less
of the title's behaviour the trace describes.

## What this leaves

The question - what writes that `.bss` global - is now behind the blind spot. The eboot binds
five imports into this module and the order it calls them in would answer it, and that order is
exactly what stopped being recorded.

**So the next instrument is a trace of bound calls**, and it is not optional: from here on, every
interesting call is one this build has deliberately stopped watching. `is_implemented` already
distinguishes a bound slot from a stubbed one, so the information exists; what does not exist is
anything on the path a bound call takes, because the whole point of binding is that there is no
path - the relocation slot holds the target and the guest jumps.

That makes it a real design question rather than a small feature, which is why it is recorded
here rather than attempted at the end of a session: a thunk that forwards to the module would
restore the trace and reintroduce exactly the indirection principle 7 exists to avoid.
