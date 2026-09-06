# D472 - Published interfaces are ported in bulk; only vendor code is found one wall at a time

**measured** - 2026-09-02 (user-directed: *"stop wasting time incrementally against known public code"*)

## The criticism, and it is correct

Three sessions running, the loop has worked like this: run a title, see where it dies, implement the
one function it died on, run again. That is the right method for a function whose behaviour nothing
documents. It is the wrong method for `setenv`.

Both walls hit today were in the second class. `_Getpctype` (D468) and `setenv` are C library
functions with published specifications, and each cost the better part of a session to *discover* -
discovery being the expensive part, since writing them is an afternoon. Nothing about running the
guest was needed to know they would be wanted: **the import list is knowable before execution**, and
principle 7 is the reason it is (interception is linking, not hooking, so every import is in the
table before anything runs).

## What the inventory actually says

`orbistoun-cli worklist --static-gap`, over the seven modules in `titles/`:

| | needed | missing |
|---|---|---|
| documented - write in bulk, no guest needed | 711 | **452** |
| the title's own modules - load, do not implement | 1 | 1 |
| vendor - the guest is the only oracle | 29,633 | 29,487 |

plus 7,333 imports with no name yet. The documented gap is `libScePosix` 281 and `libc` 171, and it
is a finite, enumerable, mechanical job: the specification is the oracle, so each one can be written
and tested without any guest reaching it.

## Three origins, not two

The distinction is now a type - `orbistoun_hle::origin::Origin` - because two of its three answers
are *not* "write it":

- **`Documented`.** The C library, POSIX, the Itanium C++ ABI, the Dinkumware C11 threads the
  platform ships (D468 established the C runtime is Dinkum-derived, so `_Thrd_join` and
  `_Atomic_fetch_add_4` have specifications too). Written from the standard, tested against the
  standard, **in bulk and out of order**.
- **`TitleOwn`.** A module the title ships in its own directory. PPSA02664 imports `il2cpp_init`
  from `Il2CppUserAssemblies`, which is sitting in `Media/Modules/` beside the eboot - along with
  `PS5Util.prx` and three plugins. `sceKernelLoadStartModule` currently hands back a handle
  *without loading anything*, and nothing anywhere registers a second module's exports, so every
  call into the game's own code lands on a stub. **Implementing these would be reimplementing the
  game.** The premise of this emulator is that guest code runs natively and only the system beneath
  it is ours; the job here is a loader, not a shim.
- **`Vendor`.** No published semantics, so the guest is the only oracle and incremental is the only
  method available. This is where the wall-chasing loop belongs, and only here.

The symbol is consulted as well as the library, because `libc` is not uniformly documented: the
platform's own allocator (`sceLibcMspaceMalloc`) lives in there, and being in a documented library
does not document it.

## Why the existing tooling hid it

`worklist` totals the **call traces**, and says so in its own help: *"A static import dump says what
a module might call; this says what it actually did"*. That is a fact about runs that have happened,
so it ranks the busy things already implemented and cannot mention a function no run has reached.
The static gap is the counterpart, not a replacement - both questions are real, and only one of them
can be answered before the guest gets there.

## What changes

Documented work is now taken in batches by family - string/memory, atomics, C11 threads, stdio,
math, C++ ABI - each tested against its specification. Vendor work stays wall-driven. The next wall
after `_Getpctype` was `TitleOwn` and therefore not a shim to write at all, which is the clearest
possible illustration of why the distinction had to exist.
