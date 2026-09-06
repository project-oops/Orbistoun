# D551 - The export was already decoded, and its blocker had half expired

**decided** - 2026-09-04

The plan named G11 step (a) - *"solve the export's operand layout by probe"* - as the next
capture-free item. It has been done for some time.

## Checked by decoding, not by reading the table

`opcode-operands.toml` carries an EXP entry solved from ten samples. That is the table saying so;
the decoder saying so is a different claim, and it is the one that matters:

```text
0xF8000000 | (target << 4), second word 0x03020100
target=0    Immediate(0)  Vector(0) Vector(1) Vector(2) Vector(3)
target=1    Immediate(1)  Vector(0) Vector(1) Vector(2) Vector(3)
target=12   Immediate(12) Vector(0) Vector(1) Vector(2) Vector(3)
```

`operands_decoded` is true, which is separately meaningful: an empty operand list with that flag
set means the family genuinely takes none, and without it means nobody has taught the decoder
the family. The decoder is careful about the difference and this checks the right one.

**Nothing asserted any of it**, which is how it stayed listed as work. `tests/export_decode.rs`
now does - the assertion that was missing, not a new capability.

Broken twice, and the second break is the interesting one. Deleting the EXP entry fails on
`operands_decoded`. Moving the target field one bit across leaves target 0 decoding **as zero**
and fails only on target 1: **a test that used `mrt0` alone would have passed a wrong field
position.** Three targets, three answers, for the same reason the framebuffer harness checks
every pixel.

## The blocker had half expired

`orbistoun-translate`'s `BLOCKED` entry for `exp` read:

> exporting needs a render target to export to, **and there is no concept of one yet** - every
> translated module today is a compute dispatch writing to a storage buffer…

The first clause stopped being true two ticks ago. There is a concept of a render target: a
colour attachment, a render pass, a graphics pipeline, and a fragment shader's `Location 0`
output reaching attachment zero - proved on a device by `orbistoun-gpu-vulkan/tests/draw.rs`
(D549, D550).

`BLOCKED` is not decoration. Its whole purpose is to separate *"nobody has looked at this"* from
*"this is waiting on a subsystem"*, because those rank differently and a list that cannot tell
them apart sends effort at whichever is most frequent. An entry claiming a whole missing
subsystem, when what is actually missing is one execution model, ranks itself far too heavily.

Rewritten to say the one thing that is still true: **every module this translator emits is a
compute dispatch, so an export has nowhere to go inside it.** A `Fragment` execution model with
output variables is the missing piece.

The clause that survives untouched is the one that matters most: mapping the export onto the
storage buffer instead would still be inventing a destination, and a shader that appears to work
while writing its colour somewhere arbitrary is worse than one that refuses.

## Three in three ticks, in three different files

D543 found a stated blocker wrong about its mechanism. D546 found one right about its mechanism
and in the wrong list. This one had a reason that expired underneath it because **the tick that
invalidated it was my own, two ticks earlier**, and nothing connected the two.

That is the sharpest version of the pattern so far. The other two were written by somebody who
then stopped looking; this one was written before a change I made, and I did not go back. Check
13 exists for exactly this - *when a change closes a gap, grep for the gap's own words* - and
what it needs is for the words to be searched at the moment the gap closes, not two ticks later
when something else happens to walk past.

## What is left of the export

One thing that needs nobody: emit `Fragment` modules with output variables. One thing that needs
a capture: which attachment a target index selects, and what targets above the render-target
range mean - the mapping D104 refuses to invent, and which decoding does not settle.
