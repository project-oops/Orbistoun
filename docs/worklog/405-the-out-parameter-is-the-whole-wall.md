# 405. The out-parameter is the whole of the wall

**2026-09-04** - (/loop)

## What was done

Went at `sceAgcCreateShader` three ways and got one answer, one confirmation of something already
recorded, and one negative that is worth as much as either.

**The extent, measured.** `ORBISTOUN_WRITE=sceAgcCreateShader:0+0:0x6000007fc000` plants a
readable pointer in the out-parameter, and the guest goes **past `image+0xf56e09` entirely** and
faults somewhere else. The derivation that predicted it: at the fault `rsp` is `0x6000007fc4e0`
and the faulting `rsi` came from `[rsp+0x38]` = `0x6000007fc518`, which is exactly the arg0 the
call was given - so arg0 is a pointer-to-pointer and the call is expected to write *into* it.

Under D227 the run afterwards is worth nothing on its own: the guest is walking a structure that
is not one, so the new fault address is not a wall and is not recorded. What the plant does
establish is the narrow claim - nothing else about this call site blocks, so no choice of arity,
argument or return value can move it.

**Two fields, read off the guest's own instructions.** `8b 46 50` takes a dword at `+0x50`;
`0f b6 d0` keeps only its low byte; `88 54 24 58` stores that byte. Then `48 8b 46 30` reads a
quadword at `+0x30`. Recorded as edges on the knowledge entry, not as a layout - what either
holds is not established and inventing it is the thing principle 3 forbids.

**The blocker is real, and it is the sixth checked.** obSCEne's captures cannot answer this:
**472 `libSceAgc` symbols recorded `absent`** across four captures, and `OBS|sysinfo|modlink/gpu`
saying no GPU library is mapped in the probe's process. Five of the last six stated blockers
turned out stale when checked; this one does not. `docs/backlog/022` now carries the ask as a
condition on *how a capture is taken* - where libSceAgc is loaded - rather than as another symbol
to add to a census.

## The surprise, and it is a method one

**An experiment re-derived something the knowledge base already held, for the third time this
session.** The hypothesis was that `0x7fff0001` has the high bit clear, so a guest testing
`if (ret < 0)` reads it as success; forcing `0xffffffff80d11000` changed nothing. That is a clean
refutation - but `orbistoun-cli knows sceAgcCreateShader` already said the caller tests against
`0x8a6c003d` **specifically**, which is a sharper statement of the same fact and was written
before this session started.

D547 was this. D555 was this at larger scale - a claim that there were no guest binaries, made
without looking in the title library. This is the third. The cost each time is a run; the fix is
one command. **`knows <name>` before experimenting on a function** is now the first step, and it
is written here rather than in a decision because it is a habit, not a design choice.

## What this does not show

That any of it generalises past one caller in one binary. PPSA28061 calls the same function
eleven times with arg0 at an image address rather than on the stack, and nothing here was checked
against that.
