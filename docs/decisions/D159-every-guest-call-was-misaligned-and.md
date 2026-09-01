# D159 - Every guest call was misaligned, and nobody was looking


**decided** · 2026-08-20

The one number that settles it had never been recorded: what `rsp` is when the guest's
`call` lands on us.

The trampoline is entered by a `jmp` from a thunk, and a jump pushes nothing - so the very
first instruction of the trampoline sees exactly what the guest's `call` left behind. One
`mov r11, rsp` before it touches anything captures it. `r11` is the one register free to
carry it: scratch under System V, and already dead because the thunk used it to hold this
address and jumped through it.

### The measurement

System V requires `rsp % 16 == 0` immediately *before* a call. The call pushes an
eight-byte return address, so a callee begins **eight past alignment**, and every compiler
does its own arithmetic from that assumption before using an instruction that moves sixteen
bytes at once.

PPSA28061, on the entry convention that was the default that morning:

```text
abi   370 of 372 calls arrived on a MISALIGNED stack
      first at #0 libc::0x92f57c2dc704346f, rsp 0x600000800f40 (rsp % 16 = 0)
```

Not a rare case. Not a subtle one. **The first call, and effectively all of them**, at
remainder 0 where the convention requires 8. Off by exactly eight, which is the signature
of one missing return address somewhere upstream.

### The cause, and why it was invisible

`Convention::Process` enters the guest by jumping with `rsp` sixteen-byte aligned - correct
for a System V process entry. `Convention::Function` enters by calling, leaving it eight
past - correct for a function. The guest carries whichever it was handed through every
frame it builds afterwards, so the choice propagates into every call it makes back to us.

| convention | conforming calls |
|---|---|
| `Process` | 2 of 372 |
| `Function` | **372 of 372** |

So **this target's entry point is called, not jumped to.** Nothing published says that -
it was established by measuring the guest, which is the oracle this project actually has.

D153 tested exactly these two conventions the same afternoon and recorded "no difference",
because the only instrument then was how far the guest got, and nothing it reached had yet
executed an aligned vector access. That conclusion was not wrong about what it measured; it
was measuring something too coarse to see this. Worth remembering: **a negative result is
only as strong as the instrument that produced it.**

### What it cost, and what fixing it bought

D157 - `sceKernelMapNamedDirectMemory` faulting in host code and parked as unexplained -
was never a bug in the mapping. Every earlier import was small enough that the compiler
never needed a sixteen-byte stack access; the mapping, which takes a mutex and builds a
vector, was simply the first one that did. It was the first *witness*, not the cause.

With the convention corrected, on PPSA28061:

| | before | after |
|---|---|---|
| distinct imports | 38 | **46** |
| calls | 372 | **799** |
| fault | host code, unattributable | `image+0xecda`, in the guest |

PPSA25872 moved further still - from faulting at `image+0x7a` having made no calls at all,
to 1735 calls and a fault eight megabytes into its own image.

### The telemetry stays on, and it reports when it is clean

Two `mov`s in the trampoline and, in the common case, no atomic operations at all -
recording must not change the program it observes (principle 9).

The report prints **every run, including when nothing is wrong**:

```text
abi   799 calls, all on a conforming stack
```

A line that only appeared on failure would be indistinguishable from a line nobody wired
up, which is precisely the state this project was in for its whole life until today.

### It measures; it does not correct

Forcing alignment in the trampoline - `and rsp, -16` - would have made the crash vanish in
one instruction, and it was explicitly rejected. The guest would still have been running
misaligned internally, and the loud, immediate, perfectly-located fault would have become
silent corruption surfacing somewhere with no relation to the cause. That is principle 3's
failure mode, chosen deliberately, which is worse than arriving at it by accident.

The general rule: **when a convention is being violated, measure who is violating it before
making the symptom go away.** The measurement was ten lines and it found a bug affecting
every run this project has ever made. The forced fix would have hidden it permanently.

