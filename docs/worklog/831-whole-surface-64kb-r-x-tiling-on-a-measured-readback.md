# 831. Whole-surface 64KB_R_X tiling, both directions, on a display-size hardware readback

**2026-09-24** — worklog 830 left Neverball waiting on its first submission's fence, and running that
submission's draws at submit honestly means writing their pixels back where the guest reads them: its
1920x1080 `64KB_R_X` colour target. orbistoun's detiler stopped at one 64 KiB block (128x128 at 32 bpp)
and refused anything larger (`SurfaceExceedsBlock`), because until now the only readback was one texel.

## The ground

obSCEne run 18 (`REQ-20260921T1640Z-1d5e`, `obscene/reports/hardware/20260921-run18-eboot.obs.log:5417-5442`,
arm `arm1-rx-1080p`) rendered one draw into a 1920x1080 `64KB_R_X` target (`cb0-tiling-mode 0x1b`) and
into a linear control, then detiled the first and compared: `detile-matches 0x1fa400` (all 2,073,600
pixels), `detile-mismatches 0`, `block0-mismatches 0`, `multiblock-mismatches 0`, with `0xbdd7d`
(776,573) pixels actually drawn. The detile it ran is the open-toolchain SDK's (`src/agc/agc_tiler.c`):
row-major 64 KiB blocks, `ceil(width/128)` per row, no per-block rotation, and an in-block XOR basis
`x = {0x4,0x8,0x80,0x100,0x2200,0x800,0x8400}`, `y = {0x10,0x20,0x40,0x1100,0x200,0x400,0x4800}`.

That basis equals orbistoun's own `tiled_byte_offset_64kb_rx_bpp4` at all 16,384 texels, checked first
in a script and now as a test. So the whole-surface rule rests on a full hardware readback, not on
software models agreeing.

## The change

`crates/orbistoun-gpu/src/tiling.rs`:

- `tiled_byte_offset_64kb_rx_bpp4_surface(x, y, width)`: the block index times 64 KiB, plus the in-block swizzle;
- `surface_words_64kb_rx_bpp4(width, height)`: every block the surface touches, whole;
- `detile_surface_64kb_rx_bpp4`: guest to linear;
- `tile_surface_64kb_rx_bpp4`: linear to guest, the direction the write-back needs; it leaves an edge block's padding alone.

Both directions refuse data that doesn't cover the surface's blocks. The one-block `detile_64kb_rx_bpp4`
and its refusal are unchanged: callers move to the whole-surface functions as they need them.

Five tests:
- inside block 0 the address equals the one-block swizzle, at widths 128, 129 and 1920;
- blocks are row-major and unrotated, with (128,0) → 64 KiB, (0,128) → block 15 at 1920 wide, and 129 wide rounding to two blocks;
- the in-block swizzle equals the measured basis at every texel;
- a 300x150 surface round-trips tile→detile, which covers partial edge blocks;
- short data is refused in both directions.

The round-trip test alone can't see a wrong block order, since any bijection round-trips. The row-major test pins exact addresses, so it would fail on one.

## Next

Execute a submission's draws at submit, write the frame back with `tile_surface_64kb_rx_bpp4` into the
guest's target, and let the fence after them retire from work that ran (D705). That is Neverball's second
frame.

## Gate state

`./bin/orbistoun check` green, worklog index regenerated, identity scan clean. No commit.
