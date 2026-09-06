# D521 - The callback has no branch that avoids the read, so one of D519's two readings is dead

**measured** - 2026-09-03 (the guest's own code, and a watchpoint on the slot it is installed in)

D519 left two readings of the wall and refused to choose between them without a measurement.
One of them can be settled by reading forty instructions, and it is now settled.

## Reading (2): "the eboot code has a branch that on hardware does not reach the read"

It does not. The whole function, entry to fault:

```text
0x13891b0  push rbp; mov rbp,rsp; push r15..rbx; sub rsp,0x98
0x13891c4  mov  r13,[0x1992A10]          the stack-guard pointer
0x13891cb  lea  r12,[rbp+0x10]           the argument, a character array by value
0x13891de  ...                           zero four locals, set one to 0x48
0x1389205  cmp  byte [rbp+0x10],0
0x1389209  je   0x1389246                <- branch one
0x138920e  call 0x1658960                strlen
0x1389216  cmp  rax,0x10
0x138921a  jb   0x1389248                <- branch two
           ... allocate, copy ...
0x1389246  xor  ebx,ebx
0x1389248  mov  rdi,r14; mov rsi,r12; mov rdx,rbx
0x1389251  call 0x16588F0                memcpy
0x1389256  mov  byte [r14+rbx],0
0x1389262  mov  r14,[0x1A30610]          <- the singleton
0x1389269  mov  rbx,[r14+0xa0]           <- FAULT
```

**Both conditional jumps land at or before `0x1389248`, which flows unconditionally into the
singleton read.** There is no path through this function that does not dereference it. It
builds a `std::string` from a character array and then reads the memory manager, every time.

The last calls before the fault - `strlen` then `memcpy`, from eboot addresses - are this
function's own string construction, which is the check that the decode is right rather than
plausible.

## How the module has the address at all

The eboot exports exactly one symbol, so the module cannot import this function. It is
**installed into a table**:

```text
0x1389148  lea rcx,[rip+0x61]        -> 0x13891B0
0x1389168  mov [rip+0x71c149],rcx    -> 0x1AA52B8
```

And a watchpoint on that slot shows both halves of its life:

```text
ORBISTOUN_WATCHPOINT=0x400001aa52b8+8:rw
  touched after the access at image+0x138916f; it now holds 0x4000013891b0   <- installed
  touched after the access at image+0x13891b0; it now holds 0x4000013891b0   <- called through
```

The second is an indirect call: a data breakpoint fires after the access and the instruction
pointer is then the *target*, which is the function itself. **The slot is read once in the
whole run, and that read is the call that faults.**

## So the surviving reading is the first one

The chain, every step measured:

```text
eboot installs callback           0x1389130 -> [0x1AA52B8] = 0x13891B0
eboot loads Il2CppUserAssemblies  0xf23a06
eboot calls into the module       0xf23ac3 -> PLT -> module +0x13d5e90
module calls back through the slot            -> 0x13891B0
0x13891B0 dereferences the memory manager     -> null -> read of 0xa0
...and 0xf269e2 would have created it, after 0xf23970 returns.
```

**The module invokes an eboot callback before the memory manager exists.** Since the callback
cannot avoid the read, and the singleton has exactly one writer which runs later (D518, D519),
the anomaly must be that the callback is invoked at all at this point.

## What is still open, stated so it is not read as settled

**Why the module invokes it.** A plausible shape - it takes a short text label and then does
memory accounting, and the fault registers show a four-character argument - is that it is a
naming or scope hook that a module reports through. If a module only reports through it on an
*unusual* path, then the real fault is whatever put the module on that path, and orbistoun's
85 unresolved imports in that module are the obvious suspects. **That is a hypothesis and it
is not tested**; naming it here is not adopting it.

What is closed is that no branch inside the callback was ever going to save this, so there is
no point looking for one.
