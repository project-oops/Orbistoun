# 712. The mesh output assembles the primitive the stream set, not always a triangle

**2026-09-19** — inbox `-0c58`, the remaining half: worklog 702 decoded `VGT_GS_OUT_PRIM_TYPE` and
carried the topology on the submission, but found the fix mis-located - orbistoun draws with a mesh
pipeline, and Vulkan ignores a mesh pipeline's input-assembly topology (D688), so "assembles every
draw as a triangle list" was literally the hardcoded `OUTPUT_TRIANGLES_EXT` and the `uvec3` index
output in `orbistoun-translate`. This makes the mesh output the shape the stream asked for.

## The mesh output is three things that must agree

A mesh module states its own output topology, and three declarations have to match or a driver
rejects it: the output **execution mode**, the per-primitive **index built-in**, and the **width**
of the index element. All three now come from the decoded primitive:

| primitive | execution mode | index built-in | index element |
|---|---|---|---|
| point | `OutputPoints` | `PrimitivePointIndicesEXT` | `uint` (scalar) |
| line | `OutputLinesEXT` | `PrimitiveLineIndicesEXT` | `uvec2` |
| triangle | `OutputTrianglesEXT` | `PrimitiveTriangleIndicesEXT` | `uvec3` |

- A new `MeshPrimitive {Points, Lines, Triangles}` in `orbistoun-translate` carries the mapping and
  the index count; `orbistoun-spirv` gained the point/line constants (`OutputPoints` 19,
  `OutputLinesEXT` 5269, `PrimitivePointIndicesEXT` 5294, `PrimitiveLineIndicesEXT` 5295).
- `emit_header` emits `primitive.output_mode()`; `declare_mesh_outputs` builds the index element at
  the primitive's width (a scalar for a point, a vector otherwise) and decorates it with the
  matching built-in; `write_mesh_indices` stores a scalar or composes a vector accordingly.
- The `exp prim` unpack (`mesh_primitive_export`) reads the primitive's index count from the same
  packed word - one nine-bit field for a point, two for a line, three for a triangle.

## The seam, and the refusal

`pipeline.rs` maps the decoded `PrimitiveTopology` onto a `MeshPrimitive` and threads it through a
new `translate_windowed_primitive` (the plain `translate_windowed` stays, defaulting to a triangle,
so no other caller changed). The primitive salts the shader cache key, because a point and a
triangle from the same bytes are different modules. A **rectangle list** and any unmeasured value
have no mesh-primitive shape and are **refused by name** rather than drawn as a triangle - the
plausible-output principle 3 at the last step before a picture (acceptance 3, watched failing in a
test).

## What is measured, what is assumed, what is not yet verified

- **Verified:** the topology→primitive mapping and its refusals (`orbistoun-gpu` unit tests); that
  the triangle path is unchanged (86 gpu + 84 translate tests, the f50b device triangle render, and
  `./bin/orbistoun check` all pass - `primitive_salt(Triangles)` is zero, so a triangle draw's key
  and module are byte-identical to before).
- **Assumed, and marked so (principle 1):** that a point or line packs its one or two `exp prim`
  indices in the same low-to-high fields a triangle uses its three. The measured packing (oracle
  record A) is a triangle; a point or line record would settle it. `mesh_primitive_export` says this
  in a comment rather than in silence.
- **Not yet verified — the acceptance closer:** the point record (`agc-primitive-draw-fw1240`)
  rendering as a point, and acceptance (2)'s device test building a point and a strip pipeline that
  are not triangle lists. The new point/line SPIR-V paths are **not exercised on a device yet** - a
  bad module would fail loudly at validation (honest failure), and the improvement over drawing a
  point as a triangle stands, but the render is the confirmation and is the next unit. Left there
  deliberately rather than claimed.

## Gate state

`./bin/orbistoun check` passes end-to-end (all checks passed): the whole workspace compiles, clippy
`-D warnings` clean, 86 gpu + 84 translate tests and the device tests pass, docs/prose/fmt clean,
identity scan clean. No commit.
