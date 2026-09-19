# 685. The 166-agc draw and state packets decode, and an indexed-draw stream now walks in a test

**2026-09-17** — inbox `-6e78` (operator): wire the `166-agc` PM4 draw and state packet fixtures into
the decoder so the command processor parses full 3D draw streams without falling through to
unrecognised-packet errors. Its three parts were delivered across this session and earlier work; this
tick verified each against its test and closed the one part that had no stream-walk test.

## What was already there

- **`PACKET3_DRAW_INDEX_2` (0x27) and index setup.** Decoded with `draw_index_2()` (three of five
  body dwords measured, `166-agc/dcb-draw-index`) and `set_index_size()` (the eight-pair sweep
  `166-agc/dcb-set-index-size` fixing `0x400 | (flags<<6) | type`). Both unit-tested in `packet.rs`.
- **The context registers.** `CB_TARGET_MASK`, `CB_COLOR0_BASE`, `CB_COLOR0_ATTRIB2/3`, and the
  depth/stencil/blend state (`DB_DEPTH_CONTROL` 0xA200, `DB_STENCIL_CONTROL` 0xA10B,
  `CB_BLEND0_CONTROL` 0xA1E0) decode in `registers.rs` and are carried on the `Submission` (worklogs
  669–675). `pipeline.rs`'s `a_stream_sets_the_pipeline_state_the_submission_carries` walks a
  `SET_CONTEXT_REG` stream **into that parsed pipeline-state struct** — the acceptance's exact shape.
- **The UCONFIG registers.** `SET_UCONFIG_REG` (0x79) walks, and `GE_CNTL` is exercised in
  `graphics_draw.rs`'s full Type-0 draw stream, which walks clean with every opcode known.
- **Real command buffers.** `measured_packets.rs` and `oracle_gl_cube.rs` walk the FW-12.40 gl-cube
  captures (record A of 2026-09-14) that the request's `20260914-095045-eboot.obs.log` session
  produced.

## What this tick added, and one honest gap

The one opcode the request names by number, `DRAW_INDEX_2` (0x27), was only unit-tested in isolation —
the stream-walk tests all issue `DRAW_INDEX_AUTO` (0x2d). Added
`an_indexed_draw_stream_walks_without_warnings` to `graphics_draw.rs`: it builds a
`SET_UCONFIG_REG_INDEX` (0x7a, the measured index-width selector) followed by a `DRAW_INDEX_2` packet,
walks it, and confirms the stream is consumed whole with the indexed draw recognised as a command
rather than an unknown packet. So item 1 is now demonstrated in the acceptance's stream-walk form.

**`GE_PC_ALLOC` is not implemented, and deliberately so:** it appears in the request's want-list but in
no captured stream (the gl-cube records carry `GE_CNTL`, `VGT_PRIMITIVE_TYPE` and the SPI/CB
registers, not `GE_PC_ALLOC`). Decoding a register no measurement contains would be inventing a
fixture, which principle 3 forbids; it waits on a capture that carries it.

## Gate state

`orbistoun-gpu` stream-walk suite green (`graphics_draw` 3, `pipeline` 23, `measured_packets`,
`oracle_gl_cube`, `vocabulary`, `dcb_wiring`, `measured_builders`); `clippy -p orbistoun-gpu
--all-targets -D warnings` clean; `./bin/orbistoun prose` exit 0; `cargo fmt --check` clean; identity
scan clean. No commit.
