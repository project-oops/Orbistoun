# D492 - Retail title modules do not export `module_start`

**measured** - 2026-09-03 (three modules, a validated hasher, no match)

D491 left the question of what fills a module's globals. The obvious remaining answer was the
platform's own start convention, and obSCEne records it as *"the strongest remaining
hypothesis"* in `docs/MODULE-FORMAT.md`:

> A module for the hardware exports `module_start` and a loader calls *that*, rather than
> jumping to the ELF entry point.

**None of PPSA02664's three shipped modules exports it.**

## What was checked

`orbistoun-cli` had no way to hash a name, so this added one - the hash is this project's own
(`orbistoun-nid`) and the suffix is a run input rather than a constant (principle 5), so doing
it by hand is not available.

```text
0xcf833c78728aa305  module_start
0x74b7eff1accc902a  module_stop
0x2d68465e7b96e57d  module_prolog
```

None of the three appears among the exports of `Il2CppUserAssemblies.prx` (247 symbols),
`PS5Util.prx`, or `sce_module/libc.prx`.

## Which is only worth anything because the tool was checked first

A negative result from an unvalidated tool is not a result. So the hasher was pointed at a name
the answer is known for:

```text
memcpy -> 0x7b50e125c4417543
libc.prx exports:  0x7b50e125c4417543  +0x3890  Q3VBxCXhUHs#D#A
```

It agrees, on a vendor-encoded export, end to end. The negatives above are therefore negatives
rather than a spelling mistake.

## What it means for the sibling project

obSCEne's hypothesis was formed from two emulators mapping its module, reporting an entry point,
and declining to execute it - which is exactly what a missing start convention would look like.
This does not disprove it for **obSCEne's** module, but it removes the reason to believe it:
three retail modules that the platform's own loader does start carry no such export, so
exporting `module_start` is not what makes a module startable.

Recorded here and noted in `MODULE-FORMAT.md`, because it is evidence obSCEne would otherwise
spend a hardware session collecting.

## What is left

The module runs - PPSA02664 executes its code (D489) - so something reaches it. The candidate
this leaves is the executable itself: it calls `Il2CppUserAssemblies`' own exported resolver
`0x6f8b9da539afc9af` **222 times**, with names like `il2cpp_init` and `il2cpp_init_utf16` as
the first argument. That is a module being driven by its caller rather than started by a
loader, and the null at `[0x480001f0c330 + 8]` sits inside that path.

**Not concluded**: nothing here establishes that the resolver is what should have filled the
global, only that it is the mechanism actually in use and the one the fault is inside.
