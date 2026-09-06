# D515 - Starting the modules took the wall down, and the guest reached its frame loop

**measured** - 2026-09-03 (before and after, both deterministic, six interleaved runs)

D514 named the wall: `sceKernelLoadStartModule` did the `Load` half and never the `Start`
half, so a placed module's `DT_INIT_ARRAY` never ran, its C++ constructors never ran, and a
global one of them fills read back as null. This runs them.

```text
before   44 distinct imports,      2,077 calls, fault: read of 0x8 at modules+0x13dca44
after   161 distinct imports, 20,000,000 calls, no fault - reached the call budget
```

## Both halves were already built

D514 called this "a substantial piece of work" needing two things. Both existed.

**Entering guest code from a shim** is `orbistoun_kernel::thread::call_guest(entry, args)` -
a synchronous call on a fresh reentrant stack, in the same crate as `load_start_module`,
built for `call_once` initialisers, `atexit` handlers and sort comparators. Nothing new was
needed at all.

**Reaching the module table from the kernel** has an exact precedent one screen away:
`orbistoun_kernel::note_region`, which the loader already calls for every placed module
because "a module's pages live in this loader's own address space, which the kernel's runtime
map never sees" (D446). `note_module_initialisers` sits beside it and is the same shape.

So the estimate in D514 was wrong in the direction that matters, and for the reason D510
named: **the queue is the last place to learn the work was done.** The cost was written from
the shape of the problem rather than from the tree.

## What is recorded, and what is deliberately read late

The loader records `DT_INIT`, the runtime address of `DT_INIT_ARRAY`, and its length. **It
does not read the array's contents**, because those are function pointers relocation writes
and the recording happens in the same function that has just written them. The address is
recorded; the pointers are read at start time, which is after linking by construction.

The link-relative to runtime step is `base + vaddr`, because a title's module links at zero -
which is why its entry point reads `0x0` and its exports report as bare offsets. A module that
did not link at zero would need its load bias instead. Nothing in the corpus does; it is
written down in `initialisers_of` rather than inlined, because that is where it will be wrong.

## The comparison was wrong first, and the fix was to report more

The first version matched the guest's path against the loader's name directly. The guest asks
for `/app0/Media/Modules/Il2CppUserAssemblies.prx`; the loader knows the module by the
**import library name** it answers to, `Il2CppUserAssemblies`, with no extension. Nothing
matched, and the run reported:

```text
2 module(s) got a handle and were NOT started (the loader recorded no initialisers for them)
```

Which reads as *the loader never placed them* and is a different problem entirely. Two wrong
inferences followed before it was measured:

- **`orbistoun-cli imports` shows zero mentions of `PS5Util`, so PS5Util is not placed.**
  False. `title_modules_for` derives what to place from the executable's **library table**,
  not its import list. A negative from a filter is a fact about the filter (check 4).
- The paths themselves were first identified by arithmetic on the `snprintf` lengths before
  each load - `0x1f` is 31 and `/app0/Media/Modules/PS5Util.prx` is 31 characters. Right, and
  not evidence.

The fix for both was the same and it was not a better inference: **the report now names what
the loader placed.**

```text
loader placed with initialisers: Il2CppUserAssemblies(1 init), PS5Util(1 init), libc(1 init)
```

One line, and the question stops being answerable by argument. Breaking the stem match now
prints the placed list and the not-started list side by side, which makes the bug read as
what it is rather than as a missing module.

## Where the guest is now

It runs to its **frame loop** and spins there:

```text
9,794,399 calls (48.9%)  libSceVideoOut::sceVideoOutIsFlipPending
9,794,398 calls (48.9%)  libkernel::sceKernelWaitEqueue
  135,635 calls ( 0.6%)  libc::memcmp
```

411,084 calls of real work, then a present loop asking whether a flip has finished and waiting
on an event queue - neither of which orbistoun ever answers, so it never leaves. **That is the
next wall, and it is a different kind of wall**: not a missing mechanism but an unanswered
question, in the two subsystems principle 6 says are reached last.

## What this does not establish

**That the constructors did the right thing.** They ran and the guest proceeded, which is a
1-bit oracle answering yes - the strongest signal this project has for most functions, and not
a claim that any particular global now holds a correct value.

**That every module should be started.** `libc` is placed and carries an initialiser, and its
initialiser does **not** run, because the guest never asks for it. Only an explicit
`sceKernelLoadStartModule` starts anything. Whether the platform runs a placed module's
initialisers at load time for modules bound as imports is unmeasured, and answering it by
starting everything would be inventing behaviour to make something work.

> **Measured in D520, and the shortcut answer is no.** `ORBISTOUN_START_MODULES` runs every
> placed module's initialisers before entry: the run falls from 181 distinct imports and
> 415,415 calls to 15 and 271, faulting on orbistoun's own placeholder used as an address. It
> does not show what the console does - it shows that orbistoun has no working libc at that
> point - but the question is no longer answerable by trying it.

**And the test asserts bookkeeping, not execution.** A test cannot manufacture executable
relocated guest text, so it pins that a recorded module is reported as started rather than as
missing, and that a start which ran nothing is called out separately - a module whose array
held nothing callable produces the same handle as one that ran every constructor it has.
