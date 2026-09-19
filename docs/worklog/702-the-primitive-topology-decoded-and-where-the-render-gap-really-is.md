# 702. The primitive topology, decoded and carried - and where the render gap really is

**2026-09-19** — inbox `-0c58`: "a draw carries no primitive topology, and the backend assembles every
draw as a triangle list." Two things came out of this: the topology is now **decoded and carried**,
and a load-bearing finding about where the render actually assembles triangles - which is **not** where
0c58's acceptance points.

## Decoded, from the register that tells the draws apart

`VGT_GS_OUT_PRIM_TYPE` (dword `0xA29B`, from `oops-mesa src/amd/registers/gfx103.json` byte 166508)
carries the **output** primitive type, enum `VGT_GS_OUTPRIM_TYPE`: POINTLIST 0, LINESTRIP 1, TRISTRIP
2, RECTLIST 3. Measured against the captures, this is the register that distinguishes the draws: the
point record writes `0`, the triangle and gl-cube records `2`. The input-assembly `VGT_PRIMITIVE_TYPE`
(`0xC242`) reads `TRILIST` (4) for **all three**, so it cannot tell a point draw from a triangle one -
the output register can.

New in `orbistoun-gpu`: a `PrimitiveTopology` enum (named values plus `Other(u32)` for anything the
register can hold but this does not map, so an unhandled topology is refused by its raw field rather
than read as a triangle - the rule `SwizzleMode` follows), `decode_primitive_topology`,
`primitive_topology_at`, and a `label`. `Pipeline::submit` reads it and carries it on
`SubmissionReport::primitive_topology`. Tested: the point record's submission reports a point list, the
triangle record's a triangle strip; the decode names all four values, `Other`s the rest, and each
labels itself (the unmapped one by its raw field, never a fabricated name). This is 0c58's acceptance
(1), end to end.

## The finding: the acceptance points at a cosmetic knob

0c58 asks for `build_pipeline` to take the topology instead of its fixed
`vk::PrimitiveTopology::TRIANGLE_LIST`. **That would change nothing that renders.** orbistoun draws with
a **mesh** pipeline (D688), and Vulkan ignores `input_assembly_state` topology for a mesh pipeline -
the mesh shader's own output declaration decides the primitives. Proven, not assumed: flipping that
line to `POINT_LIST` and re-running `-f50b`'s console-triangle test reproduced the triangle **exactly
as before**. Making `build_pipeline` take the topology would satisfy the acceptance's letter and assert
a value with no effect - the "plausible output" principle 3 forbids.

Where "assembles every draw as a triangle list" is literally true is
`orbistoun-translate/src/wavefront.rs:398`: the mesh entry point is emitted with
`OUTPUT_TRIANGLES_EXT`, unconditionally, and the whole mesh-output declaration is triangle-shaped -
`PRIMITIVE_TRIANGLE_INDICES_EXT`, a `uvec3` index array, triangle connectivity. Making a point draw
render as points needs that output made topology-specific (execution mode, primitive-indices built-in,
index vector type, connectivity, vertex/primitive counts) **and** the decoded topology threaded
through `submit` into the translation call. That is a substantial, correctness-sensitive change across
`orbistoun-translate` and `orbistoun-gpu`, validated against the point record rendering as a point -
its own unit, not folded in under a cosmetic one.

## Status

0c58 stays **open**. Acceptance (1) is done and the topology is decoded, cited and carried; (2)/(3)/(4)
are the render, and the render's triangle assembly is the translator's hardcoded mesh output, not
`build_pipeline`. This worklog is the durable record of that, so the next attempt starts at
`wavefront.rs` rather than at the cosmetic knob.

## Gate state

`./bin/orbistoun check` passes end-to-end (all checks passed): `orbistoun-gpu` clippy `-D warnings`
clean, tests pass (8 in `primitive_draw`, two new), fmt clean, prose exit 0, `status --check` exit 0,
doc gate clean, identity scan clean. Corpus unchanged. No commit.
