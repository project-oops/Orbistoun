# 807. `sceVideoOutSetBufferAttribute2` is implemented, the registered block is decoded into a flipped buffer's shape, and inbox request `-a6b3` is closed — the first rendering-cluster request the fully-owned cube drove to resolution

**2026-09-22** — the wall worklog 806 left, and the first GPU-cluster request from orbistoun's
inbox (`REQ-20260920T0125Z-a6b3`) that the cube exercised its way to. `sceVideoOutSetBufferAttribute2`
was declared and deliberately unimplemented (D500), because the reason it was left alone — *"nothing
here promises to write into the caller's memory"* — held until obSCEne `-83df` measured exactly what
that memory is. It is measured now, so the three changes the request asked for land together.

## What the call does, and the three changes

`sceVideoOutSetBufferAttribute2(attr, pixelformat, tiling, width, height, option, dcc_control,
dcc_clear_color)` fills the caller's 80-byte block with a buffer set's shape. Implemented in
`crates/orbistoun-video/src/lib.rs`:

1. **The call writes the shape** into the block at the offsets `-83df` measured on hardware: tiling
   at `0x4`, width at `0xc`, height at `0x10`, option at `0x18`, format at `0x20`. It writes **only**
   those, and only the bytes each occupies — pitch (`0x0`, `0x8`, `0x14`) reads zero in both measured
   passes and is left untouched rather than assigned a byte nobody identified, and nothing past the
   measured extent is touched. The DCC fields (`0x28`, `0x30`) are the seventh and eighth arguments,
   which arrive on the guest stack past the six registers the trampoline captures, so they are left as
   the caller had them; every value a `presented` rung needs — extent, format, tiling — is in the six.
2. **`video_out_register_buffers` decodes** the block the attribute pointer names into a new
   `BufferShape` (tiling, width, height, format) and holds it on `Port` beside the pointer it already
   kept. A zero pointer, or a block a guest never filled, decodes to a zeroed shape rather than a
   fault.
3. **`last_flipped_buffer` returns `(address, BufferShape)`** — the flipped frame's extent and format,
   not the bare attribute pointer `-420c` stopped at. Its one caller (`report.rs`) ignored the second
   element and still does; `-5e82` is the request that will size a readback from it.

## The correctness signal: the request's own acceptance, unit-tested and cube-exercised

The acceptance test (`the_attribute_block_fills_decodes_and_reads_back_its_shape`) runs the two passes
`-83df` ran: `sceVideoOutSetBufferAttribute2` fills a real block at 1920×1080 **tiled**, it registers,
flips, and `last_flipped_buffer` reads back width 1920, height 1080, tiling 0; a second at 3840×2160
**linear** reads back 3840, 2160, tiling 1; and a byte past the 80 the call writes stays the poison it
was set to. It passes.

The cube exercises the whole path on a real run: `GLCB00001` calls `sceVideoOutSetBufferAttribute2`
then `sceVideoOutRegisterBuffers2`, so the decode runs against the guest's own block, and the call is
no longer a stub — `standing` is now **28 of 28 (0 on stubs)**, up from 27 of 28.

## Why the cube's verdict is `same`, honestly

The call returns **void** — the SDK ignores it — so implementing it fills the block but does not change
the guest's control flow, and the cube makes the identical 28 calls it made in worklog 806. The display
still reports *not ready*, now for a reason with no unimplemented call behind it: a return value or a
computed check further into `agc_display_init`, past the attribute fill and the buffer registration.
That is the next wall, and it is not `-a6b3` — this request asked for the call implemented, the block
decoded, and the shape read back, all of which are done and tested. Closing it on its own acceptance
rather than on the cube's overall verdict is the honest accounting: `-a6b3` is what moved, and it moved
completely.

## Inbox

`REQ-20260920T0125Z-a6b3` marked **RESOLVED** in `C:/tmp/orbistoun/worklog.md` with this outcome. Its
sibling `-9d4e` (restate the knowledge entry with all seven offsets at `known_by = "measured"` and the
`hardware.toml` rows) stays open; the entry's note is updated to say it is implemented and to point at
`-9d4e` for the structured restatement, rather than continuing to claim nothing is established.

## Gate state

`crates/orbistoun-video/src/lib.rs` gains `BufferShape`, the `sceVideoOutSetBufferAttribute2`
implementation, the register-time decode, the widened `last_flipped_buffer`, and the rewritten
acceptance test; `crates/orbistoun-worker/src/report.rs` renames the ignored binding;
`crates/orbistoun-hle/data/knowledge/libSceVideoOut.toml` updates the note. Guest memory is read and
written through single-operation helpers, each `unsafe` block one op with its `// SAFETY:` (principle
4). The compat record advanced itself (`entered - 11 imports, 28 calls, 100% standing`), so the
generated compat and status docs are regenerated with it. `orbistoun-video` tests 9/9,
`./bin/orbistoun check` green, worklog index regenerated, identity scan clean. No commit.
