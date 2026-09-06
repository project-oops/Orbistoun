# D482 - A title's own module is found by the name that imports it, not by a path

**measured** - 2026-09-03 (user-directed plan, the TitleOwn loader)

PPSA02664 imports four symbols from a library called `Il2CppUserAssemblies`. Nothing answers
them, and that is the title's wall. The module that answers them - all four NIDs, confirmed -
is `Media/Modules/Il2CppUserAssemblies.prx`, sitting in the title's own data.

The loader has to find it. This decides how.

## What the executable actually says, which is less than was assumed

The vendor tables an encoded import name indexes hold **bare names**:

```text
libraries (38), indexed by an import's library id:
    0  Il2CppUserAssemblies
    1  PS5Util
modules (36), indexed by an import's module id:
    1  Il2CppUserAssemblies
    2  PS5Util
```

No path, in either table, and `DT_NEEDED` is a third list that is not what those ids index
(D117). **The executable names a module and says nothing about where it lives.** So a loader
cannot follow a reference; it has to search.

## Where to search, and why not `Media/Modules/`

Three titles in the corpus ship this module. All three put it under `Media/Modules/` - so it is
a convention of whatever built them, and it is **not a platform path**: the platform's own
directories are three fixed ones holding 537 modules, and this is not among them (worklog 328).

A convention is not a guarantee, and hardcoding one is how a loader works for three titles and
silently fails for the fourth. So: **search the title's own tree for a file whose stem is the
library name**, with a `.prx` or `.sprx` extension.

That rule is measured rather than guessed, and the corpus makes the point sharply:

| title | imports from | file on disk |
|---|---|---|
| PPSA02664 | `Il2CppUserAssemblies` | `Il2CppUserAssemblies.prx` |
| PPSA03416 | `Il2CppUserAssemblies` | `Il2CppUserAssemblies.prx` |
| PPSA25872 | `Il2cppUserAssemblies` | `Il2cppUserAssemblies.prx` |

**The filename is the library name, case included.** The third title spells it with a lower-case
`c` in both places. That is what makes an exact match the right rule and a clever one wrong.

### The case question, which turned out not to exist

This was carried for two ticks as "the executable imports 8 from `Il2CppUserAssemblies` and 4
from `Il2cppUserAssemblies`, so matching must handle both spellings". **It does not.** Those
were corpus-wide counts across three executables; each one is internally consistent, and the
variant belongs to a different title entirely. There is no within-module ambiguity to resolve.

Matching is still done case-insensitively, but for a smaller and honest reason: the host
filesystem may not preserve case, and a title whose file and import genuinely disagreed would
be a fact worth discovering rather than a crash. Where several files match, the exact-case one
wins.

## Order: place everything, then relocate everything

A title module may import from another of the title's own. Placing and relocating each in turn
makes the answer depend on which was found first, which is a filesystem-order dependency and
the kind of bug that reproduces on one machine.

So two passes: **place every module first, collect every export, then relocate.** By the time
anything is bound, every address that could be bound to exists. The main executable is
relocated last, for the same reason and because it is the one whose imports this exists to
answer.

## What is deliberately left open

**Whether the console binds these lazily.** A platform loader given a bare name and no path
might resolve at first call rather than at load, in which case the title would load its own
modules explicitly and binding would follow. Nothing here establishes which, and it does not
change this decision: orbistoun binds eagerly at relocation, so it needs the exports available
before the main executable is relocated either way.

Recorded because the two are distinguishable on hardware and would matter the day a title
loads a module *after* start and expects an unbound import to begin working. That is a
different mechanism, and this decision does not provide it.
