# D483 - Attribution is half of what identifies an import, and `libc` is the title's

**measured** - 2026-09-03 (user-directed plan, the TitleOwn loader)

D482 found the title's own modules. This places them, indexes what they export, and matches
that against what the executable imports. Three things came out of the first run, and two of
them contradicted something already written down.

## The match is on the hash **and** the library

An encoded import carries a library id alongside its hash. The first version of this matched
on the hash alone, and on the first title it was pointed at it reported **five** imports
binding into `Il2CppUserAssemblies` where the executable's own table names four.

Matching on the hash alone lets an import that names one library bind into a different module
that happens to export the same name. A NID hashes the symbol name and nothing else (D305), so
two modules exporting `memcpy` collide by construction - and which of them a caller meant is
exactly what the library id says.

So: **an encoded import binds only into the module its `library_id` names.** A library id the
executable's own table does not list matches nothing, rather than falling back to a hash-only
search - the fallback is the bug.

A *plain* name carries no attribution (D305), so for those the hash is all there is. Those are
matched across every module, and where more than one answers, the import is **left unbound and
reported ambiguous**. Picking one would be a call into whichever module sorted first.

## The fifth was real, and it is `setenv`

The extra binding was `0xc8275f216d188633`, and the mystery is instructive:

```text
executable's table:   library 0 = Il2CppUserAssemblies,  library 35 = libc
the import:           M4YYbSFfJ8g#A#B   ->  library id 0, module id 1
orbistoun's registry: 0xc8275f216d188633 = libc::setenv
```

**Both are right.** The eboot imports `setenv` *from the title's own IL2CPP module*, which
really does export it - a Unity IL2CPP build links a good deal of the C runtime and re-exports
it. The import was correctly attributed all along.

### What was wrong was the report I checked it against

`orbistoun-cli imports` lists that symbol under `libc`, because `loader::survey` **prefers the
registry's library name over the executable's own attribution** - it answers *who would answer
this*, not *who did the guest say*. That is the right answer for a resolution report and the
wrong one for counting what a title imports from a given library.

Which is what the note saying "four" had done. **A report answers the question it was built
for, and using it for a neighbouring question is how a fact decays.** Sixth instance, and the
first with the cause identified rather than just the error.

## `libc` is not a platform module

The measured 537-module manifest of the console's own libraries (worklog 328) contains no
`libc.sprx`. The platform's is `libSceLibcInternal.sprx`. What answers the `libc` the
executable imports 152 symbols from is `sce_module/libc.prx` - **shipped by the title**, in
its own tree, found by D482's search.

That reframes the fork this opens up:

| | binds into | changes |
|---|---|---|
| the emulator's implementations win | orbistoun's measured `libc` | nothing |
| the title's own modules win | `sce_module/libc.prx`, 152 symbols | every libc measurement |

**Default: the emulator's implementation wins wherever it has one.** It strictly increases
what resolves - the `Il2CppUserAssemblies` wall opens, including `0x6f8b9da539afc9af` - while
changing no measurement the differential and hardware corpora were taken under. It is also one
flag away from its opposite.

The second column is what the console actually does, and it is the honest end state. It is not
reachable yet for a reason that has nothing to do with preference: these modules are **placed
and not relocated**, so binding into them at all is not yet runnable.

## Placed is not linked, and nothing here is wired into a run

A placed module has had its bytes copied and nothing else. Its own `RELATIVE` relocations are
unapplied, so every internal pointer in it still reads as a link-time offset.

**An address into that is worse than a stub.** A stub reports "unimplemented" and stops; a real
address into unrelocated code runs and fails somewhere unrelated - the precise failure mode
principle 3 exists to forbid. So this slice *reports* what would bind, `run` is untouched, and
the binding waits for the relocation pass.

## What the report says today

```text
PPSA02664   Il2CppUserAssemblies 5, libc 152          157 of 583   2818 exported
PPSA03416   Il2CppUserAssemblies 5, libSceAmpr 5, libc 152   162 of 583   2936 exported
PPSA25872   Il2cppUserAssemblies 5, PS5Util 2, libSceAmpr 5, libc 186   198 of 662   3330 exported
```

Zero kind mismatches and zero ambiguities across all three, which is worth stating because
both checks exist to fail and neither has yet.
