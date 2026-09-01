# The sweep gains a second dimension, and a wall opens


`sceKernelReserveVirtualRange` had been eliminated as the source of the missing base at
`image+0xafc959`, along with twenty-two other functions. It was the answer. The elimination
varied return values and out-parameters **separately**, and this call needs both: the guest
checks the return before reading the slot, so planting a base under an error answer sends it
down the failure path, and answering success with nothing planted gives it a zero. Two clean
negatives that read as proof of absence (D283).

Proved by prediction rather than by movement, because D224 is the standing warning that a
wall which moves is not a diagnosis:

```
base 0x11000000 -> predicted 0x110fffe0 -> observed 0x110fffe0
base 0x22000000 -> predicted 0x220fffe0 -> observed 0x220fffe0
```

Then `ORBISTOUN_MAP` behind the planted base, and the guest **wrote and carried on** -
`FURTHER`, one new import, five more calls, a new fault of a different shape. Nothing in that
chain needed to know what "reserve a virtual range" means; the name is a label on a hash and
the contract came off the guest's own behaviour (D284).

### What the sweep now does

`experiment::sweep` crosses planted sentinels with what the call is forced to answer -
twenty-four boots instead of twelve, about 3.2 seconds. `Finding::OutParameter` carries the
condition it holds under, because without it the finding is false: "slot 0 is an
out-parameter at `-0x20`" is true only when the call also answers zero.

Two tests carry it, and the negative one is the point: **the same mock guest reads as a clean
`Unmoved` when only one axis moves.** That is what twenty-three functions looked like.

### The correction that mattered more than the fix

`experiment.rs` documented *why* it did not sweep returns, quoting `orbistoun-thunk`: *"A
stub policy can change what a function answers. Nothing could change what a function does."*
Half right, and the right half hid the other: the side effect was indeed what mattered, and
the return **gated** it. The module header now says so, because leaving the old reasoning in
place would have re-taught the blind spot to whoever read it next.

### Surprises worth keeping

- **`RETURN_SENTINELS` already existed**, with a comment naming this exact wall - *"the wall
  this exists for computes `base + 0xfffe0`"* - and was referenced only from a test. The
  differencing rule, the `Agreement` type and `Finding::OutParameter` were all in place. The
  gap was one nested loop and a signature, not a design.
- **The return is a condition, not a sentinel.** Differencing answers "did the guest compute
  an address from this?"; the gate question is "does success unlock the path at all?", and
  only one value answers that. Sweeping return sentinels alongside argument sentinels would
  have been a product of two different questions.
- `spawn` took `Option<&Axis>` - one diagnostic per run - which is the shape that made the
  blind spot structural rather than accidental. It takes a slice now.

