# 2026-09-02 - (/loop) The loader's answers were already right; the real gap is exports

```
hardware claims    15  ->  18   (7 outstanding, 2 opaque)
tests            1955  -> 1957
```

Went to build the TitleOwn loader. Read first, and found that three of its five measured
conditions **already pass** - and that the work I had scoped was scoped at the wrong layer.

## The doc comment described code that is no longer there

`sceKernelLoadStartModule` opened with:

> **Refused, and that is the honest answer.** orbistoun places one executable at load and has
> no way to bring another in afterwards, so every request fails.

The function below it does nothing of the sort. It answers libkernel's well-known handle,
refuses a `/system` path with the not-found errno, and hands a fresh handle to anything under
`/app0`. Somebody implemented the measured behaviour and left the comment saying the opposite.

**That comment is why the loader work looked bigger than it is.** Both the plan and the
outstanding entries carried "orbistoun's loader loads nothing, so it has no module table to
answer from" - which described the comment rather than the code. Rewritten to say what the
function does, and what it still does not.

## Three measurements that were already satisfied

Using the probe's exact paths and call form:

| path | console | orbistoun |
|---|---|---|
| `libkernel.prx` | `0x2001` | `0x2001` |
| `/system/common/lib/libc.prx` | `0x80020002` | `errno::NO_ENTRY` |
| `/system/common/lib/libSceFios2.prx` | `0x80020002` | `errno::NO_ENTRY` |

All three are now assertions. That is the third tick running where an outstanding entry's
stated reason was worse than the truth, so it is no longer a coincidence: **a reason written
when something is deferred decays, and nothing re-reads it.** The three ticks were the mutex
type mapping (already in the source), the memory-query "harness" (one call), and this.

## The two that can never be asserted, and a third list for them (D481)

The console answered `0x15` and `0x14` for two `/app0` modules. Both captures agree, so the
gate calls them constants - and **asserting either would be wrong.** A handle is allocated by
the loader; `0x15` says that console had already placed twenty modules. Orbistoun answers
`0x40` upward and its own comment says the value is opaque.

Leaving them in the work queue would say "not done yet" about something that will never be
done. `OPAQUE` names them instead, with the reason, and the gate requires every constant to be
in exactly one of three lists. The arithmetic is asserted, not printed - checked by dropping a
category and watching the count come back 25 against 27.

What *is* asserted is the shape, which is a property of the call: a title's own module must
load rather than be refused, and two loads must answer different handles.

## The real gap, which is one layer down

`Il2CppUserAssemblies.prx` and `PS5Util.prx` are not in `sce_module/` - they are in
**`Media/Modules/`**, which is why looking for them beside the vendor libraries found nothing.

And the corpus survey settles how the title reaches them: **8 imports from
`Il2CppUserAssemblies`, 2 from `PS5Util`** - plus 4 more under a case-variant spelling,
`Il2cppUserAssemblies`, which is worth knowing before anything matches library names. Those are
**import-table entries, resolved at relocation time**, not runtime `sceKernelDlsym` calls.

So the wall is not in `sceKernelLoadStartModule` at all. It is that the main executable's
imports have nothing to bind to, which means:

1. **`orbistoun-elf` reads imports and has no notion of exports.** There is `RawImport` and no
   `RawExport`; the dynamic symbol table is walked for undefined symbols only. This is the
   piece that does not exist at all.
2. The loader must **place and relocate the title's own modules before relocating the main
   executable**, and feed their exports into the resolver.

Neither is a guess: the import counts are measured off the title's own bytes, and the absence
of export handling is a grep.

**Not started this tick, deliberately** - it is a design with a load-order question in it
(which module places first, how a case-variant library name matches, what happens when a
title's module imports from another of its own), and starting it at the end of a long tick is
how the last three misleading notes got written.

## State

`cargo test --workspace` green - **117 suites, 1957 tests**, 0 failures. clippy `--tests`
clean, fmt clean, identity scan clean. Hardware: 18 claimed, 7 outstanding, 2 opaque, 27
constants.

Nothing committed. The day holds worklogs 292-326 and D466-D481.

**Next**: exports in `orbistoun-elf` - self-contained, testable against the title's own
`.prx` files which are on disk, and the thing everything else waits on. Then the load-order
design, which is D482.
