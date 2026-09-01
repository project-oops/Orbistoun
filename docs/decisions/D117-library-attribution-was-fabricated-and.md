# D117 - Library attribution was fabricated, and looked fine

**decided** · 2026-08-19 · found while answering the GPU thread

Every import orbistoun reports carries a library name. **Those names have been wrong
since the import dumper was written**, and nothing noticed because they were wrong in the
one way that does not announce itself: plausibly.

An encoded symbol name carries a small library index. The dumper treated it as an index
into the module's `DT_NEEDED` list. Counting settles it:

| | |
|---|---|
| `DT_NEEDED` entries | 52 |
| Library ids observed | 0..=54, 54 distinct |
| Imports whose id is past the end | 60 |

**An index that exceeds its table is not an index into that table.** The ids come from a
separate vendor import-library structure this crate does not parse, and indexing one list
with another list's ids produced attributions that fit, read naturally, and mean nothing.

What it looked like: `libSceAgcDriver.prx::setsockopt` - a graphics driver exporting a
socket call - and `libSceAgc.prx::sceImeDialogGetStatus`, a command-stream library
exporting text-input dialogs. Reading those is what prompted the check; no test could
have, because a test would have to know the right answer to compare against.

**What is still sound.** The NID is read from the symbol name and is exact. The *name* is
proved by hash collision and is exact. Only the library is invented - so
`sceKernelDirectMemoryQuery` is a real name for a real hash, and only "which library it
lives in" was made up.

**What it cost, concretely.** The GPU thread asked for the NID of the command-buffer
submit function. The obvious way to answer is "look in the command-stream library", and
that answer would have been drawn from a mapping that is noise. A wrong library sends
someone reading the wrong 200 imports.

It also means the vocabulary extension in D084 was built on this: module names were
harvested from `DT_NEEDED`, which is real, but the *pairing* of imports to those modules
was not. The 352 names remain valid - a hash collision does not care which library
anybody thought it was in.

**Not fixed here.** The correct table is a vendor dynamic-section entry that
`orbistoun-elf` does not read yet. Until it does, the honest thing is to stop printing a
library name as though it were known, which is the next piece of work.

<!-- Two threads worked this file in parallel and both reached D118-D120. The loader
     thread's three keep those numbers because they were published; the three below moved
     to D127-D129. Numbers are identifiers, not an ordering, so the gap is deliberate. -->

