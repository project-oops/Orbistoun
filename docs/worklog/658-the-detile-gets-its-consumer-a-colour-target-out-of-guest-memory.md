# 658. The detile gets its consumer: a colour target out of guest memory

**2026-09-17** - `detile_colour_target` bridges the decoded pipeline state (base, extent, tiling mode)
to the pixels, reading a `64KB_R_X` colour buffer out of a guest-memory window and refusing every
other case by name; the register decodes of the last four ticks now have a caller

## Why this closes a loop rather than adding another decode

The last four ticks decoded the colour target piece by piece - correlation (654), base (655), target
mask (656), tiling mode (657) - each tested against synthetic streams but none with a consumer. That
is the decode-ahead the roadmap sanctions, but a decode nobody calls is a decode nobody has really
exercised. This is the caller: it takes the register writes and a guest-memory window and produces the
linear surface, which is the whole point the base, extent and mode were decoded for.

## What landed

`crates/orbistoun-gpu/src/tiling.rs`:

- `ColourSurface { width, height, pixels }` - a detiled target, pixels row-major.
- `ColourTargetError` - `Incomplete` (base or extent unset), `UnsupportedTiling(mode)` (a mode not
  modelled, **including linear** - read straight would be a different path, not this one),
  `OutsideWindow` (the base is not in the guest slice), `Detile` (the swizzle's own refusal).
- `detile_colour_target(writes, guest, window_base)` - `colour_target_at` for base and size,
  `colour_swizzle_mode_at` for tiling, the base mapped into `guest` (whose first word sits at
  `window_base`), then `detile_64kb_rx_bpp4`. It detiles only `64KB_R_X`; anything else is a refusal,
  never a wrong swizzle (D010).

Re-exported from `lib.rs`.

## Tests, every refusal watched failing

Four:

- `detile_colour_target_reads_the_stream_and_places_the_measured_pixel` - a 64x64 `64KB_R_X` target
  based part-way into the window: the measured pixel at tiled byte 4348 comes back at linear
  `15*64+15`, with the base-to-window offset applied, and exactly one pixel set.
- `detile_colour_target_refuses_incomplete_state` - extent and tiling set, no base -> `Incomplete`.
- `detile_colour_target_refuses_a_mode_it_does_not_model` - `ATTRIB3` = 0 (linear) ->
  `UnsupportedTiling(Linear)`, not read with the tiled swizzle.
- `detile_colour_target_refuses_a_base_outside_the_window` - a base below the window ->
  `OutsideWindow`, not a read from a negative offset.

## What is still not here

The **host upload path proper** - handing this `ColourSurface` to the backend as a texture or a
readback target, over a real frame - is the next step and is backend work (`orbistoun-gpu-vulkan`).
This is the pure computational core it will call: given the state and the memory, here are the pixels.
And it is still 32-bpp `64KB_R_X` only; `4KB_S`, other bit-depths and the multi-block case remain
refusals awaiting their measurements (inbox `-8c34`).

## Gate state

`cargo test -p orbistoun-gpu --lib` **71 passed, 0 failed** (+4); clippy clean; fmt clean;
`orbistoun-gpu-vulkan` builds on the new re-exports; identity scan exit 0. No commit.
