# The number nobody had recorded: every guest call was misaligned


Asked the obvious question that had never been asked - what is `rsp` when the guest's
`call` actually lands on us? The trampoline is entered by a `jmp`, and a jump pushes
nothing, so its very first instruction sees exactly what the guest left. One `mov r11, rsp`
captures it; `r11` is the one register free to carry it, being scratch and already dead
after the thunk jumped through it.

```text
abi   370 of 372 calls arrived on a MISALIGNED stack
      first at #0 libc::0x92f57c2dc704346f, rsp 0x600000800f40 (rsp % 16 = 0)
```

Remainder 0 where the convention requires 8. Off by exactly eight - one missing return
address - on the first call and effectively every call after it.

The cause was this morning's own work. Entering the guest by jump leaves `rsp` aligned,
which is right for a *process*; entering by call leaves it eight past, which is right for a
*function*. The guest carries whichever it was handed through every frame it builds.

| convention | conforming calls |
|---|---|
| `Process` | 2 of 372 |
| `Function` | 372 of 372 |

**This target's entry point is called, not jumped to.** Nothing published says so; the
guest said so.

D153 tested those two conventions a few hours earlier and recorded "no difference" - true
of what it measured, and the instrument was how far the guest got, which is far too coarse
to see this. A negative result is only as strong as the instrument behind it, and that is
the lesson worth more than the bug.

### What it was costing

D157, parked an hour earlier as an unexplained regression, was never a bug in the mapping.
Every earlier import was small enough that the compiler never needed a sixteen-byte stack
access; the mapping takes a mutex and builds a vector, so it was the first one that did.
The first witness, not the cause.

| PPSA28061 | before | after |
|---|---|---|
| distinct imports | 38 | **46** |
| calls | 372 | **799** |
| fault | host code, unattributable | `image+0xecda`, in the guest |

PPSA25872 moved further still: from faulting at `image+0x7a` having made no calls at all,
to 1735 calls and a fault eight megabytes into its own image.

### What is deliberately not done

`and rsp, -16` in the trampoline would have fixed the crash in one instruction. It was
rejected on purpose: the guest would still have been running misaligned internally, and a
loud, immediate, perfectly-located fault would have become silent corruption surfacing
somewhere unrelated. Measure who is violating a convention before making the symptom go
away - the measurement was ten lines and found a bug affecting every run this project has
ever made.

The telemetry stays on and prints even when clean (`abi 799 calls, all on a conforming
stack`), because a line that only appears on failure cannot be told apart from a line
nobody wired up.

