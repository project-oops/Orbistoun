# D554 - The oracle carries a varying, before anything interpolates

**decided** - 2026-09-04

With `exp` translated, the question was what blocks the most now. The tool answers it, and the
roadmap's answer was stale.

## Re-derived, and the row was wrong twice over

```text
orbistoun-cli shaders crates/orbistoun-shader/tests/fixtures

shaders      8 of 10 complete
instructions 120 of 127 translatable
  FURTHER  8 of 10 shaders complete (+1), 120 of 127 instructions (+2)
  cleared exp
```

The roadmap's G7 row said **110/127 and 6/10**. The tool's own delta accounts for `+1` shader and
`+2` instructions from this week's export work, so the row was stale *before* that too - it had
been carrying a number nobody re-ran.

Four instructions remain, in two families: VINTRP's three interpolation opcodes, and MIMG's
`image_sample`. That is the whole of it.

## One of the two needs no capture, and the roadmap said both did

The section read: *"Everything still refused needs the resource model or the graphics pipeline."*
For MIMG that holds - image sampling needs descriptors and samplers, which is the resource
model's host half, and that needs a capture.

**VINTRP does not.** Its operand layout is solved from five and six samples, and what it names is
in the instruction: a destination register, a source, an **attribute index** and a **channel**.
Nothing about it is guest register state, so nothing about it is D104's to refuse. It is the last
capture-free instruction family.

## And the oracle could not have checked it

Which is why this tick did not translate it.

The framebuffer harness draws a hand-written vertex shader that emits a position and **nothing
else**. A fragment shader fed by it has no inputs, so the pipeline interpolates nothing - and a
translated interpolation checked against that would have been verified against material this
project generated, which is the precise trap phase 6's ordering was written to avoid. The same
argument that put the attachment before the draw puts the varying before the interpolation.

So: `interpolated_vertex_module` carries a `Location 0` varying, one value per corner, from the
same constant-table pattern as the positions - unchanged, so a failure is about the varying
rather than the geometry. `passthrough_fragment_module` reads that input and stores it to the
output with nothing in between, so what lands in the attachment is exactly what the pipeline
interpolated.

## Two tests, because one of them can be exact and the other cannot

**Equal corners.** Interpolation weights a value by barycentric coordinates, so where the three
corners carry the same value, *every* weighting of them is that value. The expected result is
exact and depends on nothing the driver chose - not the sample positions, not perspective
correction, not rounding.

**Differing corners.** Only that two distant pixels differ. The exact value at a pixel depends on
weights and sample locations this project did not choose, and pinning any of them would make the
test about the driver. The weak claim is still worth having: together with the exact one, it says
the pipeline *interpolates* the attribute rather than forwarding a constant.

Each says what it cannot say. The first would pass a pipeline that forwarded corner zero or
averaged all three; the second would pass one that interpolated the corners in the wrong order.

Broken twice, in the two stages the varying crosses: a vertex shader that never writes its output
fails both, and a fragment input moved to `Location 1` - the two stages disagreeing about where
the varying is - fails both. Different mechanisms, same visible symptom, which is worth knowing:
the tests cannot tell those two apart.

## What is next, and what it will need

Translating VINTRP. `v_interp_p1_f32` and `v_interp_p2_f32` are a pair computing one interpolated
attribute between them, and SPIR-V has no equivalent pair - the hardware interpolates and a
fragment `Input` variable *is* the result. Mapping two instructions onto one read is a modelling
choice with a real failure mode: a shader that used `p1`'s intermediate for anything else would
get a different number. It should be recorded as `assumed` when it is made, with that named.
