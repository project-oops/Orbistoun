# D630 - The title's own code was mapped, and stubbed anyway

**Status:** measured
**Date:** 2026-09-08

## Eighty-eight per cent of every call this project has recorded

`worklist` ranks imports by calls across the whole corpus, and one entry is not close to the
others:

```text
      CALLS  SHARE  MODULES  IMPORT
   19689012  87.6%        1  PS5Util::0xf948d02a4f9f5ace
     595200   2.6%        6  libkernel::sceKernelDlsym
```

Nineteen and a half million calls, one guest, no name. It is PPSA25872 - **2% standing**, the worst
in the corpus - and the run report is unambiguous about the shape:

```text
! PS5Util::0xf948d02a4f9f5ace is 98% of 20000000 calls - the guest is repeating it rather than
  progressing
    a guest that keeps asking the same question has not accepted the answer
```

## `PS5Util` is not a platform library

It is `/app0/Media/Modules/PS5Util.prx` - a file the **game ships**, which orbistoun places,
relocates and starts:

```text
orbistoun: 2 module(s) started: PS5Util.prx (1 initialiser(s), handle 0x40),
           Il2CppUserAssemblies.prx (1 initialiser(s), handle 0x41)
```

So there is no implementation to write. The code the guest wants is the code the guest brought, and
it is mapped in this process.

## And it exports exactly what the executable asks for

Established from tool output alone - `exports` against the module, `imports` against the
executable, and the two lists intersected:

| module | imports the eboot makes | symbols the module exports | matched |
|---|---|---|---|
| `PS5Util.prx` | 2 | 7 | **2** |
| `Il2cppUserAssemblies.prx` | 4 | 279 | **4** |

`0xf948d02a4f9f5ace` is `PS5Util + 0x2a0`.

**Six for six.** Every import the executable makes from a module the title ships is exported by
that module, the module is placed and started, and all six are answered with orbistoun's
`Unimplemented` placeholder instead.

The guest then calls one of them nineteen million times, which is a program that asked a question,
was told nothing, and asked again until the budget ran out.

## Why this is written down rather than fixed

Resolving an import to a placed module's export is the loader's job, and this project's brief
confines the work to user-space library and ABI-stub implementations. So this is a diagnosis, and
it is complete enough to act on without further investigation:

- the module is present, placed, relocated and started;
- the NID it is asked for is in its export table, at a known offset;
- nothing about it needs a hardware measurement, a name, or a new capability.

There is a real question underneath it that is **not** obvious, and it is why this record does not
simply say "link them": a title also ships `libc.prx`, and 186 of the executable's imports come
from it. For those, orbistoun's own implementation is very likely the better answer than the
game's copy, and D005's whole architecture is that an import is intercepted. So the rule cannot be
"prefer the title's module"; it has to distinguish a library this project implements from a module
only the game could have written. `PS5Util` and `Il2cppUserAssemblies` are plainly the second kind.

Recorded with that distinction stated, because a fix that got it wrong would replace this wall with
a subtler one.
