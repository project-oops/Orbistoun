# D283 - The wall needed two things right at once, and the sweep varied one at a time


**decided** · 2026-08-26 · `image+0xafc959` moved, to an address predicted before the run

Twenty-three functions had been eliminated as the source of the missing region base at the
`image+0xafc959` wall - every call `PPSA02664` makes, dyed on its **return value**, and swept
on its **offset-zero out-parameter**. The fault never moved. The conclusion drawn was that
nothing the guest calls supplies the base.

That conclusion was wrong, and the reason is the shape of the sweep rather than any of the
measurements in it. `sceKernelReserveVirtualRange` needs **both** to be right:

| varied | outcome |
|---|---|
| return `0x0` alone | fault unchanged at `0xfffe0` |
| base planted at `*arg0` alone | fault unchanged at `0xfffe0` |
| **both together** | **fault moves to the predicted address** |

The guest checks the return before reading the out-parameter. Plant the base and answer an
error, and it takes the failure path and never looks. Answer success and plant nothing, and
it reads a zero. Each half alone is a clean negative, and two clean negatives read exactly
like proof of absence.

**One-at-a-time sweeps cannot see a two-condition dependency**, and this is the first one
here that needed the pair. That is a property of the tool, not of this function, so it will
recur: `orbistoun-propose::turn` sweeps arguments and diagnostic axes independently, and the
`SweepArguments` step is defined as *"exhaustive rather than ranked - six slots and two
sentinels is twelve boots"*. Exhaustive over one axis is not exhaustive.

**The evidence is a prediction, not a movement.** D224 and D226 are the standing warning that
an intervention which moves a wall is not a diagnosis - a poke can buy progress with a wrong
answer. So the claim here is arithmetic and was written down first:

```
base 0x11000000 -> predicted 0x110fffe0 -> observed 0x110fffe0
base 0x22000000 -> predicted 0x220fffe0 -> observed 0x220fffe0
```

`planted + arg1 - 0x20`, where `arg1 = 0x100000` is the length the guest passed. The fault
address is a computed function of the planted value across two independent trials. That ties
the movement to the intervention by a relationship rather than by coincidence of timing,
which is the second observation D227 asks for.

So the call shape is established from our own measurements: `arg0` is an in-out `void **`
holding zero on entry - "you choose the address" - `arg1` is the length, and the granted base
is written back as a **full 64-bit word**, not the four bytes a `int *` out-parameter takes
(D210, D272). `arg3 = 0x40000` is alignment-shaped and remains **assumed**; nothing measured
yet depends on it.

