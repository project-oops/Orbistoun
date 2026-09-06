# D556 - The out-parameter is the whole of the wall, and the capture that could answer it does not exist yet

**Status:** measured
**Date:** 2026-09-04

## The question

`sceAgcCreateShader` is where PPSA02664 stops: `read of 0x50 at image+0xf56e09`. Three things
about it were unknown, and each would be answered differently.

1. Is it the **return value**? A stub answers `0x7fff0001`, which has the high bit clear and is
   therefore positive; a guest checking `if (ret < 0)` would read that as success.
2. Is it the **out-parameter**, or is the out-parameter merely the first thing that goes wrong
   after some earlier mistake?
3. Can obSCEne's existing captures say what the object looks like, or is a console day needed?

## What was measured

**The return value is not it, and this was already written down.** `orbistoun-cli knows
sceAgcCreateShader` records that the caller tests the answer against `0x8a6c003d` *specifically*
rather than for failure. Before reading that, this session forced a negative vendor-shaped return
with `ORBISTOUN_RETURN="sceAgcCreateShader:0xffffffff80d11000"` and got the same fault at the same
address - which confirms the record, at the cost of a run. **Third time this session that an
experiment re-derived something `knows` already held** (D547, D555, and this). The habit that
would have caught all three is one command, and it is now the first step in the worklog's method.

**The out-parameter is the whole of it, and that is measured rather than argued.** At the fault
`rsp` is `0x6000007fc4e0`, and the faulting `rsi` was loaded by `mov rsi,[rsp+0x38]` -
`0x6000007fc518`, which is byte-for-byte the arg0 the call was given. So arg0 is a
pointer-*to*-pointer: the call is expected to write a pointer *into* it, and the guest
dereferences what it finds there. Planting any readable pointer -
`ORBISTOUN_WRITE=sceAgcCreateShader:0+0:0x6000007fc000` - carries the guest **past
`image+0xf56e09` entirely**, to a different fault elsewhere.

Under D227 that is an extent, **not a diagnosis**: the guest is now walking a structure that is
not one, so where it stops next says nothing at all and is not recorded as progress. What the
experiment establishes is the narrow thing it can - that nothing else about this call site is
blocking, so no further work on arguments, arity or return values can move this wall.

**Two fields of the object are observed**, by decoding the guest's own instructions rather than
by inference: `8b 46 50` reads a dword at `+0x50` and `0f b6 d0` keeps only its low byte, so
`+0x50` is read wide and used as one byte; `48 8b 46 30` then reads a quadword at `+0x30`.
Nothing here says what either means, and both are recorded as edges rather than as a layout.

**obSCEne cannot answer it as its probe runs today.** Four captures record **472 `libSceAgc`
symbols as `absent`** - including `OBS|sym|libSceAgc|sceAgcCreateShader|absent|current` in each -
and `OBS|sysinfo|modlink/gpu` states that no GPU library is mapped in the probe's process. This
is the sixth stated blocker checked against what the sibling repository already has in eight
ticks, and the first of the six that turned out **not** to be stale.

## The decision

Record the structure and the extent; **do not implement a shader object**. Principle 3 forbids
exactly the move that is available here - inventing a plausible object and a plausible pointer to
put in a place a guest will dereference. The wall is not moved by a better guess about the
layout; it is moved by a capture taken where libSceAgc is mapped, and that ask is now written in
obSCEne's `docs/backlog/022` as a condition on the probe rather than as a symbol to census.

## What this does not establish

**What the object is for**, how large it is, or whether `+0x30` and `+0x50` are two fields of one
structure or one field read two ways by two paths. Only that two offsets are read, in that order,
by a guest that was given nothing.

**Nor that the same layout holds for any other title.** This is one caller in one binary. The
knowledge base already notes PPSA28061 calls the function eleven times with arg0 at an *image*
address rather than on the stack, and nothing here was checked against it.
