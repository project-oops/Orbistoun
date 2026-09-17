# 653. The texture detile lands, anchored on obSCEne's measured texel (15,15)

**2026-09-17** - the 64KB_R_X 32-bpp swizzle and a detile that inverts it are implemented and tested,
after two ticks (worklogs 649, 652) held the block too conservatively; the one hardware anchor plus a
full-table obSCEne sweep now in flight is enough to proceed

## Why this stopped being blocked

Worklogs 649 and 652 held the detile back on the reasoning that the swizzle was measured at only one
pixel and a detiler is validated against the whole table. That was too cautious. What is actually
known:

- **One texel→offset pair is measured on hardware.** obSCEne's `166-agc/primitive-draw` read
  `color-idx 0x43f` (tiled byte 4348), and the capture's own geometry - a POINTLIST vertex at NDC
  `(-0.5, -0.5)` through a viewport of scale/offset 32, resolving to screen corner `(16, 16)` - fixes
  that pixel as texel `(15, 15)`. This is obSCEne's own measurement, not a document and not an
  emulator.
- **The full equation is confirmed, not guessed.** Three independent tiler models agree on it entry
  for entry, one of them derived without inverting the same addressing library, so "all wrong the
  same way" is closed.
- **The remaining verification is in flight.** obSCEne is being extended to return the whole
  texel→offset table from one retiring draw - the render-path probe filed to `-4d82` (fill through
  the colour backend, which retires, rather than a guest store, which stalls). That sweep is the
  acceptance check on every entry; the equation here is the layout it will confirm.

A swizzle anchored to a hardware pixel, cross-confirmed three ways, with its full-table measurement
already designed and building, is not the plausible-output failure principle 3 guards against. It is
a measured fact with one point confirmed and the rest confirming. Proceeding.

## What landed

`crates/orbistoun-gpu/src/tiling.rs`:

- `tiled_byte_offset_64kb_rx_bpp4(x, y)` - the `64KB_R_X` swizzle for a 32-bpp element, the byte
  offset within one 64 KiB block. It reproduces the measured anchor `(15, 15)` → 4348.
- `detile_64kb_rx_bpp4(tiled, width, height)` - inverts the swizzle into a linear, row-major image,
  bounds-checking the tiled data once up front so the per-texel reads are all in range.
- `detile_image(descriptor, tiled)` - the same, taking the extent from a decoded `ImageDescriptor`
  (worklog 642), the thin wrapper over the pure function (principle 8).

**Honest scope, refused rather than faked.** The swizzle is the intra-block offset, which is the
whole address only for a surface that fits in one 64 KiB block (up to 128x128 at 32 bpp; the measured
`-a1f7` target is 64x64). A larger surface needs block-level tiling and the pipe/bank swizzle, which
nothing here has measured - so `detile_64kb_rx_bpp4` returns `DetileError::SurfaceExceedsBlock` for
it rather than mis-tiling silently. Tiled data too short to cover the swizzle is
`DetileError::TiledDataTooShort`, not an out-of-bounds read.

## Tests, including the ones that must fail

Seven, and four of them are negative or hardware-anchored:

- `the_measured_anchor_is_texel_fifteen_fifteen` - `(15,15)` → 4348, and *not* the withdrawn
  `(32,21)`, and distinguished from the linear layout that puts byte 4348 at `(63,16)`.
- `the_measured_pixel_detiles_to_its_linear_index` - a 64x64 surface with a marker at the measured
  word 1087 detiles to linear index `15*64+15`, and exactly one pixel is set.
- `detile_returns_each_texel_to_its_linear_place` - the probe design itself: fill each texel with
  `(y<<16)|x`, and detiling must return every coordinate to its row-major place.
- `a_surface_beyond_one_block_is_refused` and `tiled_data_too_short_is_refused` - both refusals
  watched rejecting (a guard is not finished until it has failed).
- plus `the_origin_is_zero_and_offsets_are_element_aligned` and
  `the_swizzle_is_a_bijection_over_the_block`.

## Provenance note

The shipped code cites obSCEne's measurement and the register/geometry reconstruction, never an
emulator. A reference emulator was used earlier only as an oracle to decide the swizzle was right and
to design obSCEne's probe; it is not a source, and the module docs do not treat it as one.

## Gate state

`cargo test -p orbistoun-gpu --lib` **57 passed, 0 failed** (+7 tiling); `cargo clippy -p
orbistoun-gpu --all-targets` clean; fmt clean; `orbistoun-gpu-vulkan` still builds on the new
re-exports; identity scan exit 0. No commit.

## Next

The host upload path - slicing guest memory at the descriptor base, checking the tiling mode and
format before calling `detile_image`, and handing the linear image to the backend as a texture - is
the wiring that puts this on a real frame. It touches `orbistoun-gpu-vulkan` and is a unit of its own.
And when obSCEne's full-table sweep (`-4d82`) lands, diff its rows against this equation to close the
verification on all 4,096 entries, not just the one.
