# 559. The translated mesh module validates, draws nothing, and the window says why not

**2026-09-14** - orbistoun-gpu-vulkan, after worklog 558

Worklog 558 translated the GL cube's primitive shader into a mesh module a validator accepts.
This was meant to draw with it and with the console's pixel shader together. It does not draw,
and the useful part of the unit is knowing *which* way it fails.

## 1. What was built to find out

Two things the draw path did not have:

- **Seeding.** A translated shader fetches its vertices out of guest memory, so a draw that
  wants to see anything has to put something there. `draw_mesh_over` writes words into the
  guest-memory window before the draw. Where a guest address lands in that window is the
  module's own mask, which the caller can compute: the cube's vertex buffer sat at
  `0x200900000` and its three vertices land at words 0, 12 and 24 of a 64-word window.
- **Read-back.** The same window, handed back after the draw. The attachment says what was
  drawn and the window says what was stored, and a guest's shaders do both - the cube's vertex
  program writes a canary word at the end of every run.

## 2. The finding

The attachment keeps its clear colour, with the console's pixel shader and with a constant
colour shader alike - so the geometry is what is missing, not the shading.

And the window still holds **exactly what the test seeded**, at the word the shader's own
canary store would have overwritten. That distinguishes two failures that look identical from
the attachment:

| | |
|---|---|
| the shader ran and produced a degenerate triangle | the window would hold its canary |
| the shader did not run | the window is untouched |

It is the second. The module is valid - `spirv-val` accepts it, the validation layer is silent
on the draw - and nothing executes.

Three things are therefore *not* the cause, each checked rather than assumed: the counts, which
the disassembly shows being computed from `m0` as three vertices and one primitive exactly as
the guest wrote them; the indices, which decode from the guest's packed word to 0, 1, 2; and
the descriptor plumbing, which the layer would have named.

## 3. What was fixed on the way

- **The descriptor layout declared its bindings for the fragment stage only.** A mesh module
  uses binding 1 too - the guest's primitive shader writes memory - and the layer called the
  pipeline invalid. It is both stages now.
- **`vertexPipelineStoresAndAtomics`** joins the features the device requests where it offers
  them. It permits storage writes from every stage before the fragment one, which includes the
  mesh stage, and the cube's vertex program writes its canary.
- The epilogue's comment said the device does not request fragment stores, which stopped being
  true at D689. It now says what the epilogue is *for* rather than what a stage may do.

## 4. Where this stops, honestly

The test reports rather than asserts, and says so: what it reports is an open question, not a
property to defend. It asserts one thing - that the window is unchanged - so that the day the
shader does run, the test fails and is the record of it, exactly as the oracle test was for the
mesh stage landing.

The next thing to try is a translated mesh module far smaller than this one: a few hand-written
guest instructions that declare, set a position and export a primitive, translated the same
way. If that draws, the difference is what the cube's shader does *besides* declaring and
exporting, and the 171,095-word module is not the place to find it. If it does not, the fault
is in the translation of the stage itself and that small module is where to look.

## 5. Files

- `crates/orbistoun-gpu-vulkan/src/framebuffer.rs` - `draw_mesh_over`, the seeding, the
  read-back, and the binding stage flags.
- `crates/orbistoun-gpu-vulkan/src/compute.rs` - the vertex-pipeline stores feature.
- `crates/orbistoun-gpu-vulkan/tests/console_fragment.rs` - the draw, and what it reports.
- `crates/orbistoun-translate/src/wavefront.rs` - the epilogue comment.

## Next

1. The small translated mesh module described above.
2. The image subsystem, for record B's textured shader.
3. `REQ-20260914T1720Z-9c4a`, which settles the payload split D688 assumes.
