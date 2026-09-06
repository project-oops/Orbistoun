# 2026-09-03 - (/loop) A `nid` verb, and a hypothesis that did not survive it

```
tests   1991  ->  1991   (a verb over an existing hasher, and a measurement)
```

D491 left the question of what fills a module's globals, having killed the constructor answer.
The next candidate was the platform's start convention - and the sibling project had already
written it down.

## Checking the siblings first, which the conventions say to do

`grep module_start` across orbistoun's decisions returned only D491's own mention. Across
obSCEne it returned `docs/MODULE-FORMAT.md`, holding this:

> **The strongest remaining hypothesis is the entry convention.** A module for the hardware
> exports `module_start` and a loader calls *that*, rather than jumping to the ELF entry point.

Formed from two independent emulators mapping obSCEne's module, printing its entry point, and
declining to execute it. A good hypothesis, and testable here for free: PPSA02664 ships three
modules the platform's own loader **does** start.

## Which needed a verb this project did not have

Every export of a vendor module is hashed, so asking "does this export `module_start`" means
hashing the name. `orbistoun-cli` had no way to do it - the hasher exists in `orbistoun-nid`
and nothing surfaced it, and the suffix is a run input rather than a constant (principle 5), so
doing it by hand is not on offer.

```text
0xcf833c78728aa305  module_start
0x74b7eff1accc902a  module_stop
0x2d68465e7b96e57d  module_prolog
```

## Validated before it was believed

A negative from an unchecked tool is not a negative. So the hasher was pointed at a name whose
answer is known:

```text
memcpy -> 0x7b50e125c4417543
libc.prx exports:  0x7b50e125c4417543  +0x3890  Q3VBxCXhUHs#D#A
```

End to end, on a vendor-encoded export in the same file the real question is about.

## The answer (D492)

**None of `Il2CppUserAssemblies.prx`, `PS5Util.prx` or `sce_module/libc.prx` exports any of the
three.** The platform starts those modules regardless, so exporting `module_start` is not what
makes a module startable.

Noted in obSCEne's `MODULE-FORMAT.md` beside the hypothesis, because it is evidence that
project would otherwise spend a hardware session collecting - and it weakens rather than
disproves: it says nothing about why two emulators decline to run *obSCEne's* module, only that
this particular answer has lost its reason.

## Where that leaves the wall

The module does run (D489), so something reaches it - and the trace says what. The executable
calls `Il2CppUserAssemblies`' own exported resolver `0x6f8b9da539afc9af` **222 times**, first
argument `il2cpp_init`, `il2cpp_init_utf16` and so on. That is a module **driven by its
caller**, not started by a loader, and the null at `[0x480001f0c330 + 8]` is inside that path.

Explicitly not concluded: nothing establishes that the resolver should have filled that global.
It is the mechanism in use and the one the fault sits in, which is two facts and not an answer.

## State

`cargo test --workspace` green - **117 suites, 1991 tests**, 0 failures. clippy `--tests`
clean, fmt clean, identity scan clean on both repositories.

Nothing committed. The day holds worklogs 292-342 and D466-D492.

**Next**: what the resolver answers. It is called 222 times and it is the module's own code, so
its return is observable - and if it answers null for `il2cpp_init`, the global that is null and
the lookup that fails are the same story.
