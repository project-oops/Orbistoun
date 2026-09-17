# 628. Indexed draws are decoded from their own measured body, not dropped for "separate state"

**2026-09-16** - the command-stream decoder counted a game's indexed draws as no draws at all;
correcting a stale belief about where `DRAW_INDEX_2` keeps its count closes that

## The gap

`draw_calls` (`crates/orbistoun-gpu/src/registers.rs`) turns a walked PM4 stream into the draws a
submission asked for. It decoded `DRAW_INDEX_AUTO` (auto-generated indices) and tracked the instance
count, but **explicitly refused the indexed draw**, with a docstring that said: *"The indexed draw is
not extracted. It takes its count from separate state and an index buffer in guest memory, and
inventing either is what this crate refuses to do."*

That belief was wrong, and the measurement already in the tree said so. The `DRAW_INDEX_2` builder
(`packet::build::draw_index_2`, worklog 534) has a body obSCEne dumped whole:
`[max_size, addr_lo, addr_hi, index_count, initiator]`. So the index **count is body word 3** and the
index-buffer **address is words 1-2** - both in the draw packet itself, not separate state. Nothing
needs inventing to read them.

The consequence of the refusal: a title that draws indexed geometry - the common case for real meshes -
produced **zero** `DrawCall`s from its draw packets, so `submission.report.draws` counted none and the
pipeline emitted no draw command for them. The frame looked empty to everything downstream.

## The fix

- `DrawCall` grows a `kind: DrawKind` - `Auto { vertices }` or `Indexed { indices, address }` -
  replacing the bare `vertices` field, so an indexed draw carries the count and the index-buffer
  address it read from its own body.
- `draw_calls` decodes `DRAW_INDEX_2` (opcode `0x27`): address from words 1-2, count from word 3. A
  body too short to hold word 3 is **dropped**, not read past - a short packet is a desync, and
  fabricating a count from whatever followed the packet is exactly the wrong reading.
- `pipeline.rs` emits `RenderCommand::DrawIndexed { indices, instances, first_index }` for an indexed
  draw (the variant and the Vulkan backend's handling of it already existed - only the emission was
  missing) and counts it as a draw. The decoded index-buffer **address is deliberately not bound
  yet**: binding it is a separate command the backend still needs, so the count is emitted and the
  address waits rather than being half-used.

## Provenance

All of it is the inverse of a measured encoder. `packet::build::draw_index_2` is the measured writer;
`draw_calls` is now its reader, and the round-trip test builds a `DRAW_INDEX_2` and reads back exactly
what went in. AMD's public PM4 field order, obSCEne's builder dump - no console firmware.

## Made to fail

- `an_indexed_draw_is_decoded_from_its_own_measured_body` - a `DRAW_INDEX_2` with a known address and
  count of 36 decodes to `DrawKind::Indexed { indices: 36, address }` with the running instance count.
  Fails against the old code, which produced no `DrawCall` at all.
- `a_short_indexed_draw_is_dropped_rather_than_read_past` - a body two words long yields no draw.
- `an_auto_draw_reads_its_vertices_and_the_running_instance_count` - pins the `Auto` variant and the
  instance tracking the reshape carried over.

## Gate state

`cargo test -p orbistoun-gpu` 35 pass (3 new); `orbistoun-gpu-vulkan` green (it consumes the now-emitted
`DrawIndexed`); `cargo clippy -p orbistoun-gpu -p orbistoun-gpu-vulkan --all-targets` clean.

## Next, when it is reached

An indexed draw still needs its index buffer **bound** (address + the index width from
`SET_UCONFIG_REG_INDEX`, whose value is measured: `0x400 | (flags << 6) | type`) before a backend can
execute it. The address is decoded and waiting; a `BindIndexBuffer`-shaped command is the next step,
and it pays off when the render path runs rather than now.
