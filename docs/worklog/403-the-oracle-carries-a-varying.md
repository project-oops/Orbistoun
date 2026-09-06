# 2026-09-04 - (/loop) The oracle carries a varying

```
G7 re-derived: 120/127 instructions, 8/10 fixtures - the roadmap said 110/127 and 6/10
four instructions left, in two families; ONE of them (VINTRP) needs no capture
suites 137   clippy/fmt/identity clean on both repos
```

Fortieth cron tick. No guest binary (checked, one command).

## Re-derived, and the row was stale twice over

`orbistoun-cli shaders` on the ten fixtures: **8 of 10 shaders complete, 120 of 127
instructions**, and the tool prints its own delta - `FURTHER (+1 shader, +2 instructions),
cleared exp`. The roadmap's G7 row said **110/127 and 6/10**, so it was stale *before* this
week's export work as well as after.

Four instructions remain: VINTRP's three interpolation opcodes and MIMG's `image_sample`.

## The roadmap said both remaining families need a capture. One does not

MIMG does - image sampling needs descriptors and samplers, the resource model's host half.

**VINTRP does not.** Its operands are solved from five and six samples, and what it names is in
the instruction: a destination, a source, an **attribute index** and a **channel**. None of it is
guest register state, so none of it is D104's to refuse. It is the last capture-free family.

## And the oracle could not have checked it, so that came first

The harness's vertex shader emitted a position and nothing else - so the pipeline interpolated
nothing, and a translated interpolation checked against it would have been verified against
material this project generated. The same argument that put the attachment before the draw puts
the varying before the interpolation.

`interpolated_vertex_module` carries a `Location 0` varying, one value per corner, from the same
constant-table pattern as the positions. `passthrough_fragment_module` reads it and stores it out
with nothing in between, so the attachment holds exactly what was interpolated.

**Two tests, because only one of them can be exact.** Equal corners: every barycentric weighting
of one value is that value, so the result is exact and depends on nothing the driver chose.
Differing corners: only that two distant pixels differ - the exact value depends on weights and
sample positions this project did not choose, and pinning them would make it a test of the
driver. Each says what it cannot say: the first would pass a pipeline forwarding corner zero, the
second one interpolating the corners in the wrong order.

Broken twice, in the two stages the varying crosses - a vertex shader that never writes it, and a
fragment input moved to `Location 1`. Both fail both tests, which is worth knowing: **the tests
cannot tell those two apart.**

The `orbistoun-kernel --test sync` failure in the workspace run was the known load-sensitive one;
it passes alone, and was re-run rather than diagnosed.

Roadmap G7, G12 and the refused-instruction list rewritten (check 13).

Decision: [D554](../decisions/D554-the-oracle-carries-a-varying.md).
