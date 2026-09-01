# 2026-08-29 - The null jump was the shortcut coming due


Both payloads jumped to null inside their own `find_pid`, and it blocked everything: no
guest reached a socket call. Four candidates, three eliminated by experiment (D359):

```
sysctl refusal          answered zero-length success   still null
signal answering zero   answered a marker instead      still exactly zero
zeroed data storage     filled every page with markers still exactly zero
the guest's own .bss    filled with 0xB5               THE FAULT CHANGED
```

`.bss` zeroed gives `instruction fetch from 0x0` in both; `.bss` filled gives `read of
0xffffffffffffffff` at `image+0x28fc` (klogsrv) and `image+0x819c` (ftpsrv). Identical
shape, so it is the shared runtime rather than one program.

**Entering at `main` skips `__crt_start`, which is the program's initialisation.** Globals
hold what `.bss` holds - zero - and zero is also what an uninitialised function pointer
looks like. The guest calls a global nothing set. `0x0` was never a mystery; it was the
correct value of a variable whose initialiser never ran.

Zeroed `.bss` stays right - C guarantees it. `ORBISTOUN_BSS_FILL` is a diagnostic that
breaks the contract deliberately, the fourth of a family (stack, heap, direct, bss), off by
default including when the value fails to parse.

### Worth keeping

A guest jumping to null and a guest reading an uninitialised global produce the **identical
fault**, and no amount of staring at `0x0` separates them. The only thing that did was
making zero stop being the value.

And it revises the plan rather than advancing it: `PAYLOADS.md` had entering at `main` as a
way round the handoff structure. It is a way past its *first* wall, and the structure is
back to being the problem - now clearly worth more than the alternative, being one structure
against an unknown number of globals.

