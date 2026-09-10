# D594 - The wall is a library the dump replaced, not three functions

**Status:** measured
**Date:** 2026-09-08

## Asking the replacement what it needs

Five iterations treated PPSA03416's wall as three `sceKernelApr*` functions. One command asks the
thing that actually calls them:

```text
orbistoun-cli imports titles/PPSA03416-app0/fakelib/libSceAmpr.sprx
99 imports, 58 unresolved
```

**Fifty-eight.** The replacement library implements the Ampr API in guest code (D592) and needs
fifty-eight things from `libkernel` that orbistoun does not provide. Three of them were declared
this week and it made no difference, which is now unsurprising rather than mysterious.

What it needs, by family:

| family | what it looks like |
|---|---|
| `sceKernelApr*` | nine resolve variants - prefix, for-each, ids-only, ids-and-sizes - plus `GetFileSize`, `GetFileStat`, and four submit spellings including two `_TEST` ones |
| `sceKernelWrite*Command` | **fourteen** - `Map`, `Map2`, `MapDirect`, `Remap`, `MultiMap`, `Unmap`, `ModifyProtect`, `ModifyMtypeProtect`, most with a `WithGpuMaskId` twin |
| direct memory | `sceKernelGetDirectMemorySize`, `sceKernelAvailableDirectMemorySize` |

The `Write*Command` family is what writes commands **into** a command buffer, and it is entirely
absent. That settles the contradiction three entries have carried: a header claiming one command
of twenty bytes over storage that scans as zero for eight kibibytes (D594's scan, not an
assumption about the first thirty-two bytes). Nothing wrote a command because the functions that
write commands are not there.

## What the scan and the watchpoints ruled out on the way

- **The storage is genuinely empty.** Eight kibibytes of it, scanned word by word rather than
  read at the front and assumed.
- **The encoder's global is `__stack_chk_guard`**, the one data import the fakelib names, served
  as a zeroed page (D323). `mov rbx,[rip+…]; mov rax,[rbx]` in its prologue is a stack canary
  load, not the command state it looked like.
- **The command-buffer header is bookkeeping only** (D593), which agrees: the bookkeeping is the
  fakelib's own and the payload was always somebody else's job.

## Why the count matters more than any one name

A wall of three functions is a session's work. A wall of fifty-eight in one library is a
different kind of object: it says the title's asynchronous file path cannot be reached by
implementing what the *title* imports, because the title imports a shim, and the shim imports a
platform surface this project has barely touched.

**It also names the next fifty-eight things to do**, ranked by nothing yet, but enumerated - which
is more than this wall has offered before.

## What this does not establish

**Measured and corrected by D595: the title reaches three of them.** Under `ORBISTOUN_RESOLVE=all`
every import gets a reporting stub, and only the resolve, the submit and the wait are called - no
`sceKernelWrite*Command` at all. The caveat below was right and the framing above it was too
broad.

**That fifty-eight is the number that matters.** Unresolved means orbistoun does not declare it;
some may never be called. Which of the fifty-eight this title's path actually reaches is a
measurement nobody has taken, and taking it is cheaper than implementing them.

**Nor that the `Write*Command` family belongs to the file path at all.** Every one of them names
a *memory mapping* operation - map, remap, protect - so they may be a command-buffer API for
address-space work that the same shim happens to use. The file path and the mapping path being
the same mechanism is a reading, not a measurement.

**Nor that implementing them is the right goal.** These are the imports of a *shim somebody else
wrote* to make this dump run under emulation. Serving them is compatibility with that shim rather
than with the platform, and whether that is in scope is a question for `docs/SCOPE.md` rather
than something to settle by writing the code.
