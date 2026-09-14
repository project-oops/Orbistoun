# 558. The console's primitive shader translates into a mesh module

**2026-09-14** - orbistoun-translate, orbistoun-spirv, orbistoun-gpu and orbistoun-cli, after
worklog 557, implementing D688

The GL cube's vertex program - an NGG primitive shader oops-sdk wrote, a console ran, and
oracle record A captured - now translates. It comes out as a 171,095-word mesh module that
`spirv-val` accepts.

Both records' shaders that are not the textured one translate. The corpus census says
**FURTHER, 4 of 6**, and the only blocker left in it is the texture sample.

## 1. The three instructions, and what each became

D688 named the correspondence; this implements it.

| guest | host |
|---|---|
| `s_sendmsg sendmsg(MSG_GS_ALLOC_REQ)` | `OpSetMeshOutputsEXT` with the counts read out of `m0` |
| `exp prim` | the triangle's three indices, written into the index array |
| `exp pos0` | vertex `n`'s `Position`, for the lane that is vertex `n` |
| `exp param<k>` | vertex `n`'s output at location `k` |

The counts are **values**, not literals: the guest writes them into `m0` and the translation
reads that register back, so a shader that computed its counts translates as readily as one
that wrote `0x1003`. Where the two counts sit inside `m0` is the one assumption in this whole
translation - D688 records it and `REQ-20260914T1720Z-9c4a` asks the console to settle it. For
this shader every candidate split gives the same answer.

The primitive's index packing was read off our own shader: `0x20280600`, which the comment
beside it calls "vertices 0, 1, 2 with edge flags", is exactly `0 | 1 << 10 | 2 << 20` with bits
9, 19 and 29 set. So the indices are nine bits at 0, 10 and 20, and the edge flags are the bits
between. The flags are not translated: they say which edges a wireframe draws, the host decides
that from its own state, and reading a flag into an index would be a number that looks like a
vertex.

## 2. Three things the toolchain told me rather than my assuming them

Each was a refusal from something that checks, and each is written down where it bit.

- **The structural checker asked for a row.** The emitter's own module check refused
  `OpSetMeshOutputsEXT` with *"no row in the shape table, so this module cannot be checked -
  add one rather than trusting the result"*. That is the guard doing exactly what it says.
- **A 1.4 entry point lists every global variable**, not only the inputs and outputs - which
  the version constant's comment predicted when it was added yesterday. The register files and
  buffers do not exist when the header is written, so the entry point is now written **last**,
  which the builder allows because it keeps header instructions in ordered slots by opcode. The
  same property lazy capabilities rely on.
- **A mesh shader may not read its own outputs.** Every other write in the wavefront model
  keeps what was there where the lane is inactive, by loading the old value and selecting
  against it. A mesh module cannot, so the vertex writes are unconditional.

That last one is an assumption and is recorded as one. It is safe exactly when the vertices a
guest emits are its **low lanes**, which is how a primitive shader is arranged - it narrows the
mask to `(1 << n) - 1` and each of those lanes is a vertex, so an inactive lane's slot is past
the declared count and nothing reads it. A shader with a sparse vertex mask would write a
vertex it did not mean to emit. Nothing here can check that: the mask is a runtime value.

## 3. What moved elsewhere

- The submission pipeline maps the guest's **vertex** slot to the mesh stage. Until yesterday
  it attempted one as a compute dispatch, which reported an instruction rather than a policy;
  now there is a stage to send it to.
- The corpus census sweeps the mesh stage too. Without that a vertex program still reported as
  untranslatable, for a reason about the question rather than the shader - the same fault the
  sweep was built to avoid.
- The oracle test **failed on purpose** and is updated. It pinned "one module, for the one
  shader that translated" and named the condition under which it would want changing: *if the
  mesh stage landed, this test is the record of it*. It landed, and the test caught it rather
  than the improvement passing unnoticed.

## 4. What is not claimed

The module is valid; it has not been drawn. Its vertex fetch reads the guest's vertex buffer
through the memory window, which in a test holds nothing, so a frame from it today would be a
triangle of zeros. Drawing the console's own geometry needs that window filled with what the
console had, which is the next question rather than this one.

And the frame hash in the oracle record is still a different claim from any of this.

## 5. Files

- `crates/orbistoun-translate/src/model.rs` - `send_message`, the two mesh export paths, the
  measured export target codes, `s_sendmsg` out of `BLOCKED` and into `SUPPORTED`.
- `crates/orbistoun-translate/src/wavefront.rs` - `Stage::Mesh`, the output declarations, the
  parameter pre-pass, the entry point written last, and the unmasked vertex store.
- `crates/orbistoun-spirv/src/lib.rs` - the shape row for the new opcode.
- `crates/orbistoun-gpu/src/pipeline.rs` - the vertex slot maps to the mesh stage.
- `crates/orbistoun-cli/src/main.rs` - the census sweeps it.
- `crates/orbistoun-gpu/tests/oracle_gl_cube.rs`,
  `crates/orbistoun-gpu-vulkan/tests/console_fragment.rs` - the pins and the new measurement.

## Next

1. Draw with it: fill the memory window with the console's vertex data and run the translated
   mesh module and the translated pixel shader together. That is the first frame this project
   could compare against a console's.
2. The image subsystem, for record B.
3. `REQ-20260914T1720Z-9c4a`, which settles the one assumption above.
