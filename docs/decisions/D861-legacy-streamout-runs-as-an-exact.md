# D861 - Legacy streamout runs as an exact compute dispatch

**Status:** decided
**Date:** 2026-10-14

Transform feedback from a legacy vertex shader is carried out by the command processor as exact
compute dispatches (D705, D746) of that vertex program. A draw left rasterizing is then drawn as
well, its stores dropped there (D881).

- **Offsets.** `STRMOUT_BUFFER_UPDATE` keeps each streamout buffer's offset, in bytes, across draws
  and submissions, as the hardware does. A begin sets it from the packet (zero) or from a filled
  size in memory. An end stores it. Each streamout draw advances it by what it wrote.
- **The draw.** A `DRAW_INDEX_AUTO` whose registers enable streamout runs the program at
  `SPI_SHADER_PGM_LO/HI_VS` as compute waves of `VS_W32_EN`'s width. Each wave takes whole
  primitives, one thread per vertex, and its `v0` holds the draw's vertex id
  (`ComputeInputs::first_thread`). This holds however the wave's lanes are held: the whole wave in
  one invocation, one lane an invocation (D880) or two (D973). Run one lane an invocation, a wave
  once held its lane's place in the wave alone, so every wave past a draw's first read vertex ids
  from zero. The wave's first vertex is read from the push block at `FIRST_THREAD_PUSH_WORD`, the
  word after the device table's, not compiled in. One module then serves every wave of every draw
  of a program, as `every_wave_of_every_draw_shares_one_module` checks. Compiled in, each first
  vertex was another module name, translation and driver compile. Its user data is the `USER_DATA_VS` words, then the words
  `SO_EN` and `SO_BASEn_EN` add:
  - `streamout_config`, the wave's vertex count in bits 22:16;
  - `streamout_write_index`, the wave's first vertex counted from the draw's, since the offsets
    already carry what earlier draws wrote;
  - each enabled buffer's offset in dwords, as the draw starts.
- **Clamping.** The count is clamped to whole primitives that fit every enabled buffer, which is
  inferred from radeonsi's use of the filled size.
- **Exports.** The pass drops the program's position and parameter exports, under
  `discard_vertex_exports`, which is part of the module's cache key. With `DX_RASTERIZATION_KILL`
  they reach nothing on the hardware either. Without it they are the draw's, made by the draw.
- **Counters.** Each stream keeps the primitives written and the primitives its draws needed room
  for. A `SAMPLE_STREAMOUTSTATS` event (type 32 for stream 0, 1-3 for the others, at event index
  3) writes the two as 64-bit words with bit 63 set, the fence radeonsi waits on
  (`si_query.c`).
- **Loads.** The program loads its buffer descriptor itself, from radeonsi's internal table at
  `{s0, 4}`. A scalar load the buffer tracer gives a slot reads the buffer bound there, as
  `s_buffer_load` already did. The host-side prefix evaluation (D753) also follows the path that
  falls through an `s_cbranch_exec[n]z` jumping past the access, because the access runs only on
  that path.

Refused by name:

- a primitive shader (NGG) or an earlier stage;
- strips and fans;
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
