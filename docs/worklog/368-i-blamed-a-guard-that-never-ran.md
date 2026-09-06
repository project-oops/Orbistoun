# 2026-09-03 - (/loop) I blamed a guard that never ran

```
no code changed   suites 124   tests 2006   clippy/fmt/identity clean
```

Fifth cron tick. D518's conclusion was wrong; this corrects it and replaces it with a chain
that is measured end to end.

## The disproof

D518 reasoned: one writer, two guards, the second passes, **therefore the first fails.** The
instruction two before that call loads the allocator object from a global, so watching it says
whether the call site is reached at all:

```text
ORBISTOUN_WATCHPOINT=0x400001a87210+8:rw
  touched at image+0xf14e51 ... 0xf154f7 ... 0xf157a2 ; holds 0x740001481b00
```

**Three sites, none of them `0xf1b39b`.** The recorder holds 32 distinct sites and used three,
so there was room for a fourth - the negative is about the run, not the instrument (check 3).

The global is set. The code that would consume it never executes.

## The mistake, named

Two guards were read out of a disassembly, one eliminated by measurement, and the other
declared the cause **without checking that either one ran**. Elimination is not demonstration -
the failure principle 3 describes, committed inside a decision whose subject was measurement.

**Before asking why a branch went the wrong way, ask whether the branch was reached.**

## What is actually true

```text
nearest prologue before the store: 0xf138a0
direct callers of 0xf138a0:        ['0xf269e2']

0xf269c4  call 0xf23970     <- the fault happens in here, and it never returns
0xf269e2  call 0xf138a0     <- the memory manager is initialised HERE
```

**The singleton is null because nothing has initialised it yet, and that is correct at that
moment.** Not a missing write - a call ordering.

Inside `0xf23970` the guest loads `Il2CppUserAssemblies.prx` and calls into it through its own
import table (`0xf23ac3` -> `ff 25` PLT -> GOT `0x19935b0`); the module exports the NID the
eboot imports at `+0x13d5e90`, which is the frame the fault reports one level down. The module
calls back into the eboot, and that code reads the singleton.

```text
eboot init -> 0xf23970 -> PLT -> module +0x13d5e90 -> back into eboot -> read 0x1A30610 -> null
```

## What is not established, and it is the whole remaining question

Whether hardware takes this path at all. Either the module's export does not call back this
early on hardware and orbistoun has put it in a state where it does, or the path is normal and
the eboot code it calls has a branch that does not reach the read. **Both testable, neither
tested**, and choosing one now would repeat the mistake above.

Worth noting: this ordering only became reachable because D515 started the modules. Not
evidence D515 is wrong - the PLT binding predates it - but a guest that could not previously
reach this code is a guest whose ordering nobody has checked.

Decision: [D519](../decisions/D519-the-guest-calls-its-module-before-the-memory-manager-exists.md).
D518 corrected inline, its title and index status changed.
