# 659. A texture declares its own tiling: the T# swizzle mode decodes

**2026-09-17** - `ImageDescriptor` now carries its tiling mode, decoded from `SQ_IMG_RSRC_WORD3.SW_MODE`,
so a caller can tell whether `crate::tiling` can detile a guest texture - the texture analog of the
colour target's tiling (worklog 657), and the decode side of the texture upload path

## Why this, and why it is not the colour target's mode again

A guest's render target carries its tiling in `CB_COLOR0_ATTRIB3` (worklog 657); a *texture* carries
its own, in the T# resource descriptor, and the two are read from entirely different places. The
texture path (G10 host half, G15) needs the same question answered - is this surface `64KB_R_X` (which
`crate::tiling` detiles), linear, or a mode to refuse - but for the image descriptor, not the colour
register. So the T# gains its tiling, decoded and cited, the same shape the colour target got.

## The measurement

`SQ_IMG_RSRC_WORD3.SW_MODE` is dword 3 bits `[20, 24]` of the image descriptor, from `oops-mesa
src/amd/registers/gfx10-rsrc.json` (the `SQ_IMG_RSRC_WORD3` type). The value is `AddrSwizzleMode`, the
same enum the colour target's `COLOR_SW_MODE` uses (`addrtypes.h:225`, linear `0`, 64KB_R_X `27`), so
the two decodes share it.

## What landed

`crates/orbistoun-gpu/src/registers.rs`:

- **`ColourSwizzleMode` -> `SwizzleMode`.** The enum is the GPU's `AddrSwizzleMode`, not a colour-only
  thing - a texture uses it too - so the colour-specific name was wrong. Renamed (greenfield, no
  external consumers), and `decode_swizzle_mode(field)` added as the general five-bit decoder both
  callers use; `decode_colour_swizzle_mode` now just shifts `COLOR_SW_MODE` out and calls it.
- **`ImageDescriptor.tiling: SwizzleMode`**, decoded from dword 3 in `decode_image_descriptor`. The
  struct doc's "does not carry the tiling" note is replaced: the *mode* is now decoded; the swizzle
  *equation* is still measured only for 64KB_R_X 32 bpp (worklog 653).
- **A pre-existing doc bug fixed on the way past:** a stray copy of the `decode_image_descriptor` doc
  was glued onto the front of the `Scissor` struct's doc comment. Removed, so `Scissor`'s doc starts
  with what `Scissor` is.

## Tests

`an_image_descriptor_decodes_its_base_dimensions_format_and_tiling` gains a `sw_mode` to its encode
helper and a second case: a 1920x1080 texture with `SW_MODE = 27` decodes to `tiling:
SwizzleMode::Tiled64KbRX`, so a decode that ignored dword 3 (as it did until now) would call it linear
and be caught. The linear case (`sw_mode = 0`) is the first assertion.

## Gate state

`cargo test -p orbistoun-gpu --lib` **71 passed, 0 failed**; clippy clean (a `similar_names` warning on
`word3`/`words` cleared by naming it `tiling_word`); fmt clean; `orbistoun-gpu-vulkan` builds on the
rename; identity scan exit 0. No commit.
