# 660. detile_texture completes the texture detile side, sharing a core with the colour target

**2026-09-17** - `detile_texture` reads a T#'s base, extent and tiling and detiles it out of a
guest-memory window - the texture analog of `detile_colour_target` (worklog 658) - and the two now
share one `detile_surface` core, with the surface and error types renamed off "colour" since a
texture uses them too

## Why this closes the decode side

Last tick (659) gave `ImageDescriptor` its tiling mode; the piece it was decoded *for* is this: given
a texture descriptor and the guest memory, produce the linear pixels. `detile_colour_target` already
did exactly that for a colour buffer read from registers, and a texture wants the identical thing from
a descriptor. So the two are one operation with two front ends, and are now written that way.

## What landed

`crates/orbistoun-gpu/src/tiling.rs`:

- **`detile_surface(base, width, height, tiling, guest, window_base)`** - the shared core: refuse
  unless `64KB_R_X`, map the base into the window, `detile_64kb_rx_bpp4`. Both bridges call it.
- **`detile_colour_target`** is now the thin register-reading front end (`colour_target_at` +
  `colour_swizzle_mode_at` then the core); **`detile_texture(descriptor, guest, window_base)`** is the
  thin descriptor-reading one. A texture descriptor always carries base and extent, so `detile_texture`
  never returns `Incomplete`.
- **`ColourSurface` -> `Surface`, `ColourTargetError` -> `SurfaceError`.** Both are shared by colour
  targets and textures now, so the "colour" in the names was wrong - the same reasoning that renamed
  `ColourSwizzleMode -> SwizzleMode` in 659. The error messages lost their "target" wording too.

## Honest scope

`detile_texture` applies the 32-bpp `64KB_R_X` swizzle, so it checks the *tiling* (refusing anything
but `Tiled64KbRX`) but trusts the caller on the *format*: a different element size is a different
swizzle, and the descriptor's format-to-bpp decode does not exist yet, so guarding on it would be a
check against a value nothing computes. Documented as the caller's precondition, the same line
`detile_image` already draws. A format-to-bpp decode and a bpp check are the follow-on.

## Tests

Two, mirroring the colour-target pair: `detile_texture_reads_the_descriptor_and_places_the_measured_pixel`
(a 64x64 `64KB_R_X` T# based part-way into the window, the measured pixel at byte 4348 landing at
linear `15*64+15`, one pixel set) and `detile_texture_refuses_a_linear_descriptor` (a linear T# is
refused by its mode, watched failing).

## Gate state

`cargo test -p orbistoun-gpu --lib` **73 passed, 0 failed** (+2); clippy clean; fmt clean;
`orbistoun-gpu-vulkan` builds on the renames; identity scan exit 0. No commit.
