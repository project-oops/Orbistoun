# D670 - The refusal a guest cannot see is a success

**Status:** decided
**Date:** 2026-09-10

## What the fault register said

PPSA02664 and PPSA03416 both die at `image+0x39f7c`, and two titles sharing a fault address is
the strongest signal in the corpus. The register dump at the fault:

```
rbx = 0x7fff0001
rsi = 0x542e2e00776f6c66
```

`rbx` is callee-saved and holds **orbistoun's `Unimplemented` placeholder** - the guest kept
the value from a failed call. `rsi` is not a pointer at all: little-endian, those bytes read
`"flow\0..T"`. The faulting instruction is `mov edx, [rsi + rdx*4]`, indexing off a string.

The guest had taken a refusal, believed it, and built a pointer out of what came after.

## D009 optimised for the wrong reader

Placeholders avoid the high bit *deliberately*, "so an unimplemented stub leaking into
guest-visible behaviour is obvious in a trace rather than plausible" (D009).

Obvious in a trace. **Invisible to the guest.** Every error this platform has been measured
returning sets the high bit - the kernel's `0x8002_xxxx`, audio's `0x8026`, videoout's
`0x8029`, net's `0x8041_01xx`, the pad's `0x8092`, sysmodule's `0x805A` - so a guest checks
`rc < 0`. `0x7FFF_0001` is **positive**. A guest that checked its return value correctly was
told the call had worked.

That is the failure principle 3 exists to prevent, sitting inside the mechanism principle 3
built. `StubPolicy` defaults to `Unimplemented` rather than `Ok` for exactly this reason, and
`Unimplemented` was `Ok` all along to anything that looked.

## Measured, not reasoned

The policy is data (principle 5), so the question cost one run. `default_return = { raw =
0xF7FF0001 }` - the same value with the high bit set - and PPSA02664 again:

| | `0x7FFF_0001` | `0xF7FF_0001` |
|---|---|---|
| ends | fault at `image+0x39f7c`, read of `0xffffffffffffffff` | **the guest called `exit`** |
| says | nothing | `sceCommonDialogInitialize() failed 0xf7ff0001` |

**Two observations of different kinds**, which is what principle 3 requires of an intervention
that moves a wall. The first is where it stopped; the second is the guest naming, in its own
words, the function it stopped on. A wrong answer could have produced either alone. Producing
both, consistently, is the guest agreeing with the diagnosis.

And the second is worth more than the first: `image+0x39f7c` took three sessions to
characterise and named nothing. `sceCommonDialogInitialize` is a work item.

## The value

`0xF7FF_0000`, which is the old base with the high bit set:

- **Negative**, so the guest's own check catches it.
- **Not `0x80`**, which every measured vendor code begins with, so it still cannot be read as
  firmware behaviour - the half D009 was right about.
- **Same low half**, so a reader who knows `0x7FFF_0001` recognises `0xF7FF_0001` on sight.

Setting the high bit made one new case reachable: a guest that widens the code to sixty-four
bits now produces `0xFFFF_FFFF_F7FF_0001` rather than the same positive number, so
`placeholder_named` accepts the sign-extended form. Without that, a fault on a widened refusal
reads as an unrecognised address, and sends a reader to debug this codebase instead of to
implement the function - which is the one distinction that lookup exists to make.

## The reach numbers were inflated, and will fall

Every `FURTHER` earned past an unimplemented call was earned by a guest being told the call
worked. Those numbers are not measurements of how much of the interface a guest reached; they
are measurements of how far it got before the lie caught up.

So this is expected to read as `BACK` across the corpus, and `BACK` is the correct answer. The
frontier is a best-ever record and will not show it, which is worth saying plainly rather than
letting the file imply nothing changed: **the committed frontier is now a record of runs taken
under a placeholder that read as success**, and the numbers under it are not comparable with
what follows.

## Six tests failed on a number rather than on a behaviour

`crates/orbistoun-kernel/tests/posix.rs` wrote `0x7FFF_0002` and `0x7FFF_0003` out as literals.
Two copies of one constant, and the second copy is the one that goes stale - the same shape as
the twelve knowledge files nothing loaded (D668) and the `machine-profiles.toml` that still
said `ps5` after the rename (D663). They derive from `GuestError` now.
