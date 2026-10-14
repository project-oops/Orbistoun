# D861 - Legacy streamout runs as an exact compute dispatch

**Status:** decided
**Date:** 2026-10-14

Transform feedback from a legacy vertex shader, with the rasterizer killed, is carried out by the
command processor as an exact compute dispatch (D705, D746) of that vertex program.

- **Offsets.** `STRMOUT_BUFFER_UPDATE` keeps each streamout buffer's offset, in bytes, across draws
  and submissions, as the hardware does. A begin sets it from the packet (zero) or from a filled
  size in memory. An end stores it. Each streamout draw advances it by what it wrote.
- **The draw.** A `DRAW_INDEX_AUTO` whose registers enable streamout runs the program at
  `SPI_SHADER_PGM_LO/HI_VS` as one compute wave of `VS_W32_EN`'s width, one thread per vertex.
  Its user data is the `USER_DATA_VS` words, then the words `SO_EN` and `SO_BASEn_EN` add:
  - `streamout_config`, the vertex count in bits 22:16;
  - `streamout_write_index`, zero, since the offsets already carry what earlier draws wrote;
  - each enabled buffer's offset in dwords.
- **Clamping.** The count is clamped to whole primitives that fit every enabled buffer, which is
  inferred from radeonsi's use of the filled size.
- **Exports.** The program's position and parameter exports are dropped. With
  `DX_RASTERIZATION_KILL` they reach nothing on the hardware either, so the translator drops
  them under `discard_vertex_exports`, which is part of the module's cache key.
- **Loads.** The program loads its buffer descriptor itself, from radeonsi's internal table at
  `{s0, 4}`. A scalar load the buffer tracer gives a slot reads the buffer bound there, as
  `s_buffer_load` already did. The host-side prefix evaluation (D753) also follows the path that
  falls through an `s_cbranch_exec[n]z` jumping past the access, because the access runs only on
  that path.

Refused by name:

- rasterization left on;
- a primitive shader (NGG) or an earlier stage;
- strips and fans;
- more vertices than one wave;
- a base vertex index;
- a stream other than the first;
- indexed draws;
- an offset taken from the streamout unit's filled size;
- a non-zero packet offset, whose unit `DATA_TYPE` is not established.

**Why:** GCTS00001's `KHR-GL30.clip_distance` cases read their vertex outputs back through
transform feedback. radeonsi turns NGG off for streamout on this generation and submits a legacy
vertex shader. orbistoun runs every vertex program as an NGG mesh shader and has no legacy vertex
path, so the command processor stopped at the begin packet and at the draw. A streamout pass that
draws nothing is a compute program over guest memory. The exact dispatch path already writes back
precisely the words such a program stores.

**Rejected:**
- **Vulkan transform feedback** (`VK_EXT_transform_feedback`). It would capture the translated
  mesh shader's outputs rather than the guest's own stores, and it needs a legacy vertex stage
  that orbistoun does not have.
- **Synthesising the stores from the export values.** That would restate the program's own
  address arithmetic, and the guest's stores are the ground truth.
