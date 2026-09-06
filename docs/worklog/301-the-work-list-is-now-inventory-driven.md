# 2026-09-02 - (/loop) 452 documented functions were knowable all along; the work list is now inventory-driven

User's criticism, and it lands: *"we know we need all of this stuff, so why are we adding it
piecemeal rather than one big port"*. Both walls hit today - `_Getpctype` and `setenv` - are C
library functions with published specifications, and each cost most of a session to **discover**.
Writing them is an afternoon; discovery was the expensive part, and none of it was necessary: the
import list is knowable before execution, which is exactly what principle 7 buys.

## What was actually missing was a report

`worklist` totals call traces and says so in its own help - *"a static import dump says what a module
might call; this says what it actually did"*. That is a deliberate choice and it is right for vendor
code, but it means the tool can only ever name functions a run has already reached. Nothing answered
"what is imported and unimplemented", which for a documented function is enough to act on.

Added `orbistoun-cli worklist --static-gap`. Over the seven modules in `titles/`:

| | needed | missing |
|---|---|---|
| documented - write in bulk, no guest needed | 711 | **452** |
| the title's own modules - load, do not implement | 1 | 1 |
| vendor - the guest is the only oracle | 29,633 | 29,487 |

plus 7,333 imports with no name yet. The documented gap is `libScePosix` 281 and `libc` 171.

## The classification is a type, with tests (D472)

`orbistoun_hle::origin::Origin` - `Documented` / `TitleOwn` / `Vendor` - because two of the three
answers are not "write it". `TitleOwn` encodes today's other finding: PPSA02664's `il2cpp_init`
imports come from `Il2CppUserAssemblies.prx`, sitting in the title's own `Media/Modules/` beside
`PS5Util.prx` and three plugins. `sceKernelLoadStartModule` hands back a handle **without loading
anything** and nothing registers a second module's exports, so calls into the game's own code land
on stubs. Implementing them would be reimplementing the game.

The classifier consults the symbol as well as the library, since `libc` is not uniformly documented -
`sceLibcMspaceMalloc` lives there and is not a standard function. Five tests pin that, including the
two that would have been got wrong.

## One thing I nearly shipped

The first version of the summary printed **"1 needed, 2 missing"** - impossible on its face, because
`needed` deduped by symbol name while the gap summed per-library sets. Fixed by counting missing
names flat as well, and the reason is in a comment: a symbol two libraries both name is one job. A
report that prints an impossible number is the failure principle 3 is about, and it was in a tool
built *to* fix a reporting blind spot.

## Not done, deliberately

`setenv` was half-written this tick and abandoned mid-edit when the direction changed - the file is
untouched (`grep -c "fn setenv"` is 0). It belongs in the first batch rather than as another
one-off, which is the whole point.

## Next

Batch the documented gap by family - string/memory, atomics, C11 threads, stdio, math, C++ ABI -
each written from its specification and tested against it, no guest required. Vendor work stays
wall-driven. After the batches, the `TitleOwn` loader is the single highest-value structural piece:
it would unlock `Il2CppUserAssemblies`, `PS5Util` (36k calls in the corpus aggregate) and three
plugins at once.

clippy `--tests` clean, fmt clean, orbistoun-hle tests pass, identity scan clean, nothing committed.
