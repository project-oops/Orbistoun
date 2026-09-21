# 737. The caller of 0x42d90 found: the memcpy is gated, and the container field is at +0x3c8

**2026-09-20** — with `sceAgcInit` ruled out (worklog 736), the wall is back to worklog 725's open
question: where does the null container come from. This tick traced one level up from the faulting
`0x42d90`, and the answer reframes the fault from "a field nobody filled" to "a path that should not have
been taken".

## The stack is at a fixed base, so the call chain is readable without new tooling

The guest omits frame pointers, so `walk_frames` finds nothing (worklog 724). But `rsp` and `rbp` are
**stable across runs** - `rsp = 0x6000007fc018`, `rbp = 0x6000007fc0b0` every time - because the stack
sits at a fixed base under the identity mapping. So `ORBISTOUN_PEEK=0x6000007fc018+0x200` dumps the live
stack at the fault, and the return addresses into the title's own code (`0x4000000xxxxx`) read straight
out of it: `0x42ebd` (the memcpy call inside `0x42d90`), then `0x37f53`, then `0x3840b`. No stack-walker
was needed - a fixed-base stack plus the existing peek is a call chain. That is worth keeping for any
future no-frame-pointer fault.

## What the caller does

`0x37f53` is the return address of `0x42d90`'s caller, so the call site is just above it. Disassembled
from the loaded bytes (`ORBISTOUN_PEEK=0x400000037ec0+0xa0`):

```
0x37ec2  mov   rbx, rdi              ; rbx = this function's arg0
...
0x37f32  lea   r12, [rbx+0x3c0]      ; a sub-object inside rbx
0x37f39  test  r14, r14
0x37f3c  jz    0x37f64
0x37f3e  movzx edx, byte [r14+0x5b]  ; a gate byte
0x37f43  test  edx, edx
0x37f45  jz    0x37f53               ; [r14+0x5b] == 0  ->  SKIP the call entirely
0x37f47  mov   rsi, [r14+0x18]       ; arg1
0x37f4b  mov   rdi, r12              ; arg0 = rbx+0x3c0
0x37f4e  call  0x42d90               ; the faulting path
```

Two things fall out of this. First, the container `0x42d90` reads is **`[rbx+0x3c8]`**: inside `0x42d90`
the argument becomes its `rbx`, `r13 = [rbx'+8]`, and `rbx' = rbx+0x3c0`, so `r13 = [rbx+0x3c8]`. That is
the null the memcpy dies on - a field at `+0x3c8` of the object this function received as arg0.

Second, and more important, **the call is gated**. `0x42d90` runs only when `[r14+0x5b]` is non-zero. If
that byte is zero, the guest jumps straight over the call and never touches the container. So the fault is
not simply "orbistoun failed to fill `[rbx+0x3c8]`" - it is "orbistoun took the `[r14+0x5b] != 0` branch,
which on hardware may be the branch not taken." A gate byte deciding whether a whole graphics submission
happens is exactly the kind of thing an upstream synthesised value sets, and the wrong value there sends
the guest down a path whose data was never prepared because the path is not meant to run.

## Where this points next

Two threads, both one level up. `rbx` (the object whose `+0x3c8` is null) is this function's arg0, passed
from `0x3840b` - so its construction, and whether `+0x3c8` is ever written, is traced there. And `r14`
(whose `+0x5b` gate decided the branch, and whose `+0x18` is the memcpy's other argument) is the object to
identify: what sets `[r14+0x5b]`, and is orbistoun making it non-zero where the console leaves it zero.
The second is the more promising, because a gate is a smaller, more testable thing than a container: if
`[r14+0x5b]` should be zero here, that is one byte to explain, not a whole object to populate.

## Gate state

No code changed - this tick was `ORBISTOUN_PEEK` stack and code dumps and disassembly. `./bin/orbistoun
check` was green as of worklog 736 and nothing here touched the tree; identity scan clean. No commit.
