# 2026-09-03 - (/loop) The wall resolves to a real address, and `libc` turns out to be the title's

```
tests   1966  ->  1974
```

D482 found the title's own modules. This places them and matches their exports against what
the executable imports. The measured completion condition is met:

```text
binds Il2CppUserAssemblies  0x6f8b9da539afc9af  r8mvOaWdi28#A#B   0x5000013dbc80
```

**PPSA02664's wall now resolves to a real address inside a placed module** rather than to a
stub. All five of its `Il2CppUserAssemblies` imports do.

## Which is five, and the note said four

The first run reported five where the executable's table names four, and the extra one was
worth chasing. Two separate things were wrong, one in the code and one in my note.

**In the code:** matching was on the hash alone. An import that names one library could bind
into a different module exporting the same name - a NID hashes the symbol name and nothing
else (D305), so that collision is by construction. Fixed: an encoded import binds only into
the module its `library_id` names, and an unattributed plain name that two modules both answer
is **left unbound and reported ambiguous** rather than guessed at. Five tests, three of them
negative.

**In the note:** the fifth binding was real. It is `setenv`, and the eboot genuinely imports it
*from the title's own IL2CPP module*, which really does export it - an IL2CPP build links a
good deal of the C runtime and re-exports it.

What said otherwise was `orbistoun-cli imports`, which lists it under `libc` because
`loader::survey` **prefers the registry's library name over the executable's own attribution**.
That is right for a report about who would answer a symbol and wrong for counting what a title
imports from a library. I had used it for the second question.

Sixth decayed note in a row - and the first where the cause is identifiable rather than just
the error. **A report answers the question it was built for.** Reaching for the nearest one is
how a fact rots.

## `libc` is shipped by the title, not by the console

The 537-module manifest of the console's own libraries (worklog 328) has no `libc.sprx`. The
platform's is `libSceLibcInternal.sprx`. What answers the `libc` the eboot imports 152 symbols
from is **`sce_module/libc.prx`, in the title's own tree**, which D482's search finds because
it searches rather than walking to a path.

So the fork is bigger than the five Il2Cpp symbols:

| | binds into | changes |
|---|---|---|
| emulator's implementations win | orbistoun's measured `libc` | nothing |
| title's own modules win | `sce_module/libc.prx`, 152 symbols | every libc measurement |

Defaulted to the first (D483): it opens the wall while changing nothing any measurement was
taken under, and it is one flag from its opposite. The second is what the console does and is
the honest end state - it is simply not reachable while these modules are unrelocated.

## Placed is not linked, and `run` is untouched

A placed module has had its bytes copied and nothing else - its own `RELATIVE` relocations are
unapplied, so every internal pointer still reads as a link-time offset. **An address into that
is worse than a stub**: a stub says "unimplemented" and stops, whereas real-looking code with
unrelocated pointers runs and dies somewhere unrelated.

So this reports and binds nothing into a run. `orbistoun-cli imports --own --placed` says what
would bind, and to which module.

## Three titles

```text
PPSA02664   Il2CppUserAssemblies 5, libc 152                            157/583   2818 exported
PPSA03416   Il2CppUserAssemblies 5, libSceAmpr 5, libc 152              162/583   2936 exported
PPSA25872   Il2cppUserAssemblies 5, PS5Util 2, libSceAmpr 5, libc 186   198/662   3330 exported
```

Zero kind mismatches and zero ambiguities on all three. Worth stating rather than passing over:
both checks exist to fail, and neither has yet, so neither is known to work on real input.

## State

`cargo test --workspace` green - **117 suites, 1974 tests**, 0 failures. clippy `--tests`
clean, fmt clean, identity scan clean.

Nothing committed. The day holds worklogs 292-330 and D466-D483.

**Next**: the relocation pass. Each title module needs its own thunk table and data-block base,
and the run's data-symbol table is a `OnceLock` - so the named data symbols of every module
have to be merged and installed once rather than per module. Only after that can any of this
be handed to a guest.
