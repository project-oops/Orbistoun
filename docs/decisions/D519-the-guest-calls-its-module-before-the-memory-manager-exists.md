# D519 - The guest calls into its module before the memory manager exists, and D518 blamed the wrong guard

**measured** - 2026-09-03 (watchpoints, and the guest's own call graph)

D518 ended by naming a cause. The cause was wrong, and the way it was wrong is worth more than
the correction.

## The disproof

D518's chain was: the singleton at `0x1A30610` has one writer at `0xf1b3cb`; that store is
guarded twice; the second guard passes; **therefore the first guard fails.**

The instruction two before that call loads the allocator object from another global:

```text
0xf1b394  mov rdi, [rip+0xb6be75]     -> 0x1A87210
0xf1b39b  mov esi, eax
0xf1b39d  mov edx, 0xffffffff
0xf1b3a2  call 0xEEE410
```

So watching `0x1A87210` says whether the call site is reached at all:

```text
ORBISTOUN_WATCHPOINT=0x400001a87210+8:rw
  touched after the access at image+0xf14e51; it now holds 0x740001481b00
  touched after the access at image+0xf154f7; it now holds 0x740001481b00
  touched after the access at image+0xf157a2; it now holds 0x740001481b00
```

**Three sites, and none of them `0xf1b39b`.** The recorder holds 32 distinct sites and used
three, so there was room for a fourth - the negative is about the run, not about the
instrument (check 3). The global is set, and the code that would consume it never executes.

## The mistake, named plainly

Two guards were read out of a disassembly, one was eliminated by measurement, and the other
was declared the cause **without checking that either one ran**. Elimination is not
demonstration. It is the failure principle 3 describes - reporting more than the measurement
supports - committed inside a decision whose subject was measurement, which is the part worth
remembering.

The cheap check that would have caught it is the same one that caught it here: before asking
*why a branch went the wrong way*, ask *whether the branch was reached*.

## What is actually true

The singleton's only writer sits in a function starting at `0xf138a0`, and that function has
exactly one caller in the whole binary:

```text
nearest prologue before 0xf1b3cb: 0xf138a0
direct callers of 0xf138a0:       ['0xf269e2']
```

And `0xf269e2` runs **after** the call that faults:

```text
0xf269c4  call 0xf23970        <- the fault happens inside here, and it never returns
0xf269c9  test al, al
0xf269cb  je   0xf269db
             ... either branch falls through to ...
0xf269e2  call 0xf138a0        <- the memory manager is initialised HERE
```

So **the singleton is null because nothing has initialised it yet, and that is correct at that
moment.** The wall is not a missing initialisation. It is that the guest reaches code needing
the memory manager *before* the line that creates it.

## How it gets there

Inside `0xf23970`, the guest loads `Il2CppUserAssemblies.prx` and then calls into it through
its own import table:

```text
0xf23a06  call sceKernelLoadStartModule   -> handle 0x41
0xf23ac3  call 0x1659E80                  -> `ff 25` PLT stub -> GOT slot 0x19935b0
```

The eboot imports four symbols from `Il2CppUserAssemblies`, and the module exports
`0x9fdbed4fe0989d70` at `+0x13d5e90` - which is the frame the fault reports one level down
(`+0x13d5f00`, `0x70` into it). The module then calls back into the eboot, and *that* code
reads the singleton.

Full chain, all of it measured rather than assumed:

```text
eboot init (0xf269c4)
  -> 0xf23970            loads Il2CppUserAssemblies.prx
    -> PLT 0x1659E80     -> module export +0x13d5e90
      -> back into eboot -> reads 0x1A30610 -> null -> read of 0xa0
...and only after 0xf23970 returns would 0xf269e2 have created it.
```

## What this does not establish, and it is the whole remaining question

**Whether hardware takes this path at all.** Two readings survive and nothing here separates
them:

- The module's export does something different on hardware - it does not call back this early -
  and orbistoun has put the module in a state that makes it.
- The path is normal and the eboot code the module calls has a branch that, on hardware, does
  not reach the singleton read.

Both are testable, neither is tested, and picking one now would be the same mistake this
decision exists to correct. What is settled is the shape: **a call ordering, not a missing
write.**

> **The second reading is dead - see D521.** The callback's two conditional jumps both land at
> or before the instruction that flows into the singleton read, so **no path through it avoids
> the dereference**. And the eboot cannot be handing the module this address by export - it
> exports one symbol - it installs it into a slot at `0x1AA52B8`, which a watchpoint shows
> being written once and read once, and that read is the call that faults. The first reading
> survives: the module invokes the callback before the memory manager exists.

Worth noting alongside: this ordering only became reachable because D515 started the modules.
That is not evidence D515 is wrong - the binding through the PLT predates it - but a guest that
could not previously reach this code is a guest whose ordering nobody has checked.
