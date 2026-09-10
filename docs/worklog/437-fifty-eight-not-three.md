# 437. Fifty-eight, not three

**2026-09-08** - directed, continuing 436

## What was done

Followed the disassembly one more step, then stopped and asked a better question.

**The step**: the add-command site calls an encoder at `0x480002e752e0`, and only increments the
count when it returns success. The encoder's prologue loads a global that turns out to live in
orbistoun's thunk **data-block** region - which for a moment looked like the missing command
state and is `__stack_chk_guard`, the one data import the fakelib names, served as a zeroed page
(D323). A stack canary load.

**The scan**: the command storage is genuinely empty - eight kibibytes of it, word by word,
rather than the thirty-two bytes every previous reading looked at and generalised from.

**The better question**: ask the replacement library what *it* needs.

```text
orbistoun-cli imports .../fakelib/libSceAmpr.sprx
99 imports, 58 unresolved
```

Fifty-eight. Including **fourteen `sceKernelWrite*Command` functions** - the family that writes
commands into a command buffer - and nine `sceKernelAprResolveFilepaths*` variants, four submit
spellings, `GetFileSize`, `GetFileStat` (D594).

**Which settles the contradiction three entries have carried.** Nothing wrote a command into the
buffer because the functions that write commands are not there.

## Surprises

- **Five iterations treated this as three functions.** It is a library. The three declared this
  week made no difference, which stopped being mysterious the moment the shim was asked rather
  than inferred from.
- **`orbistoun-cli imports` on the shim was one command** and would have been available at any
  point since the fakelib was noticed. The lesson is the same one as the format record and the
  guest's own error strings: the answer was in something the project can already print.
- **The `Write*Command` family is entirely memory-mapping vocabulary** - map, remap, protect,
  unmap - not file vocabulary. Whether the file path and the mapping path are one mechanism is
  now an open reading rather than an assumption.

## A scope question, flagged rather than answered

These are the imports of a shim *somebody else wrote* to make this dump run under emulation.
Serving them is compatibility with that shim, not with the platform. Whether that belongs in this
project is a `docs/SCOPE.md` question and not one to settle by writing the code, so it is written
down and left.

## Next

- Which of the fifty-eight this title actually reaches. Measuring that is far cheaper than
  implementing them, and `ORBISTOUN_RESOLVE=all` plus the shim's own import list is most of it.
- The scope question above, which changes what "next" means.
- The frontier test's flakiness while a session runs titles - three times now.
