# D359 - Entering at `main` skips the initialisation the program needed


**decided** · 2026-08-29

Both payloads that run reached their own `find_pid`, reported a failure through their own
diagnostic path, and then executed `instruction fetch from 0x0`. It blocked everything: no
guest reached a socket call, so Stage 2 of [PAYLOADS.md](../PAYLOADS.md) was aimed at something
unreachable.

Four candidates. Three were eliminated by experiment rather than argument:

| candidate | test | result |
|---|---|---|
| the `sysctl` refusal | patched it to answer zero-length success | still jumped to null |
| `signal` answering `SIG_DFL`, which is zero | made it answer a recognisable marker | still **exactly** zero |
| the zeroed data-import storage (D323) | filled every page with a marker | still exactly zero |
| **the guest's own `.bss`** | filled it with `0xB5` | **the fault changed completely** |

```
.bss zeroed   instruction fetch from 0x0     (both payloads)
.bss = 0xB5   read of 0xffffffffffffffff     klogsrv image+0x28fc, ftpsrv image+0x819c
```

Identical in both, which makes it a property of the shared runtime rather than of one
program.

### What it means

**The `main` shortcut has a cost, and this is it.** D343 entered past `__crt_start` because
`main` is a real symbol and the handoff structure is not derivable. That worked - eight
functions and a printed banner came out of it - and it was never free: `__crt_start` is the
program's *initialisation*, and skipping it leaves globals holding whatever `.bss` holds.
Which is zero. Which is also what an uninitialised function pointer looks like.

So the guest calls a global that nothing set, and `0x0` is not a mystery - it is the correct
value of a variable whose initialiser never ran.

**Zeroed `.bss` remains right.** C guarantees it and the guest is entitled to it. The fill is
a diagnostic that deliberately breaks the contract to find out who was depending on the
initialiser rather than on the zero.

### Why nothing else could have found it

A guest jumping to null and a guest reading a global nobody set produce the **identical
fault**, and no amount of reading `0x0` separates them. Stack, heap and direct memory each
had a poison already (D325); `.bss` did not, and it is the one that mattered here.
`ORBISTOUN_BSS_FILL` is the fourth, and it is off by default - including when the value does
not parse, because a typo silently turning it on would make an ordinary run behave like a
diagnostic one.

### What this does to the plan

`PAYLOADS.md` treated the handoff structure as *the* research problem and entering at `main`
as a way round it. That is now half true: `main` gets a program running and printing, and it
does **not** get it past its own initialisation.

Two routes, and they are the ones already written down. Work out the handoff structure so
`__crt_start` can run - its three routes are unchanged - or find what `__crt_start`
initialises and set it, which is the same question asked from the other end. **The first is
now clearly worth more**, because it is one structure against an unknown number of globals.

