# 2026-09-03 - (/loop) No branch avoids the read: half the open question is closed

```
no code changed   suites 124   tests 2006   clippy/fmt/identity clean
```

Seventh cron tick. D519 left two readings and refused to choose without a measurement. One of
them needed forty instructions read, and it is now dead.

## Reading (2) - "a branch that on hardware does not reach the read" - is wrong

```text
0x1389205  cmp byte [rbp+0x10],0
0x1389209  je  0x1389246        <- branch one
0x1389216  cmp rax,0x10
0x138921a  jb  0x1389248        <- branch two
0x1389248  ... memcpy ...
0x1389262  mov r14,[0x1A30610]  <- the singleton
0x1389269  mov rbx,[r14+0xa0]   <- FAULT
```

**Both jumps land at or before `0x1389248`, which flows unconditionally into the read.** The
function builds a `std::string` from a character array and then dereferences the memory manager,
every time. No path avoids it.

The last calls before the fault - `strlen` then `memcpy` from eboot addresses - are this
function's own string construction, which is the check that the decode is right rather than
merely plausible.

## How the module has the address

The eboot exports one symbol, so the module cannot import this. It is **installed into a table**
(`lea rcx,[0x13891B0]; mov [0x1AA52B8],rcx`), and a watchpoint shows the slot's whole life:

```text
touched after the access at image+0x138916f; it now holds 0x4000013891b0   <- installed
touched after the access at image+0x13891b0; it now holds 0x4000013891b0   <- called through
```

The second is an indirect call - the breakpoint fires after the access and the instruction
pointer is then the target. **The slot is read once in the entire run, and that read is the call
that faults.**

## So the surviving reading is the first

```text
eboot installs callback  -> [0x1AA52B8] = 0x13891B0
eboot loads the module, calls into it
module calls back through the slot -> 0x13891B0 -> reads the memory manager -> null
...and 0xf269e2 would have created it, after that call returns.
```

**The module invokes an eboot callback before the memory manager exists.** The callback cannot
avoid the read and the singleton has exactly one writer that runs later, so the anomaly is the
invocation itself.

## Still open, and said so rather than left to read as settled

**Why the module invokes it.** It takes a short text label - the fault registers show a
four-character argument - and then does memory accounting, which has the shape of a naming or
scope hook. If a module only reports through such a hook on an unusual path, the real fault is
whatever put it there, and that module's 85 unresolved imports are the obvious suspects.
**Hypothesis, untested; naming it is not adopting it.**

What is closed: no branch inside the callback was ever going to save this, so there is no point
looking for one.

Decision: [D521](../decisions/D521-the-callback-has-no-branch-that-avoids-the-read.md).
