# 2026-09-03 - (/loop) A title's module is found by name, and `fakelib/` proved the rule right

```
tests   1961  ->  1966
```

D482 written, and the discovery half of the loader built. It found a directory I had not heard
of, in the first run against real titles, which is the argument for the decision better than
the decision made itself.

## What the executable says, which is less than assumed

The vendor tables an encoded import name indexes hold **bare names and no paths**:

```text
libraries (38), indexed by an import's library id:
    0  Il2CppUserAssemblies
    1  PS5Util
```

So a loader cannot follow a reference to `Media/Modules/`. It has to search. Read rather than
assumed, via a new `imports --libraries`, because the last four ticks were all notes that were
worse than the truth.

## And the case question did not exist

Carried for two ticks as "the executable imports 8 from `Il2CppUserAssemblies` **and** 4 from
`Il2cppUserAssemblies`, so matching must handle both spellings". **It does not.**

Those were corpus-wide counts across three executables. Each is internally consistent:

| title | imports from | ships |
|---|---|---|
| PPSA02664 | `Il2CppUserAssemblies` | `Il2CppUserAssemblies.prx` |
| PPSA03416 | `Il2CppUserAssemblies` | `Il2CppUserAssemblies.prx` |
| PPSA25872 | `Il2cppUserAssemblies` | `Il2cppUserAssemblies.prx` |

The variant belongs to a different title, and **its filename matches its own spelling**. There
was no within-module ambiguity to resolve, and the note that said otherwise was mine. That is
the fifth in a row.

Matching is still case-insensitive, for a smaller and honest reason now written down: the host
filesystem may not preserve case. Exact case wins where two files answer.

## `fakelib/`, which is why the rule is a search

The rule is: **search the title's own tree for a file whose stem is the library name**. Not
`Media/Modules/` - that is a convention of whatever built these titles, and a convention is not
a guarantee.

First run against the corpus, and it found:

```text
Il2CppUserAssemblies    Media/Modules/Il2CppUserAssemblies.prx
PS5Util                 Media/Modules/PS5Util.prx
libc                    sce_module/libc.prx
libSceAmpr              fakelib/libSceAmpr.sprx
```

**Three directories, two of which I would not have hardcoded.** `sce_module/` is at least
platform-mapped; `fakelib/` is a title's own invention that nothing had mentioned. Had the rule
been a path, it would have worked for the module I was looking at and quietly missed the other
two.

## The order, decided but not yet built

Place every module first, collect every export, **then** relocate - two passes rather than one
pass per module. A title module may import from another of the title's own, and doing each in
turn makes the answer depend on which the filesystem offered first. That is the class of bug
that reproduces on one machine.

## What is left open, on purpose

**Whether the console binds these lazily.** A loader given a bare name and no path might
resolve at first call rather than at load, in which case the title loads its own modules
explicitly and binding follows. Nothing here establishes which, and it does not change the
decision - orbistoun binds eagerly at relocation, so it needs the exports before the main
executable is relocated either way. Recorded because the two are distinguishable on hardware
and would matter the day a title loads a module after start and expects an unbound import to
begin working.

## State

`cargo test --workspace` green - **117 suites, 1966 tests**, 0 failures. clippy `--tests`
clean, fmt clean, identity scan clean.

Nothing committed. The day now holds worklogs 292-329 and D466-D482.

**Next**: placement and relocation - give each found module its own base clear of
`DEFAULT_MODULE_BASE`, place them, read their exports, and feed those to the resolver so the
eboot's four `Il2CppUserAssemblies` imports bind to real addresses. `orbistoun-cli imports`
shows them `<unknown>` today; that is the completion condition.
