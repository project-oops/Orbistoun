# 690. The detiling module, and why it models one mode and refuses the rest

**2026-09-17** — inbox `-8c34` (operator): a clean-room RDNA2 GFX10 detiling module for `4KB_S` and
`64KB` swizzles, common formats, and round-trip verification tests. The `64KB_R_X` 32-bpp half is
done and more than round-trip-tested; the rest is blocked by a **measured** impossibility, and this
records why so the request is not re-opened against the same wall.

## What is done, and exceeds the ask

`crates/orbistoun-gpu/src/tiling.rs` detiles the `64KB_R_X` 32-bpp swizzle - the mode every guest
**colour target** uses (obSCEne measured `cb0-tiling-mode 0x1b` on every draw). Its tests already
include the round-trip 8c34 asks for:

- `detile_returns_each_texel_to_its_linear_place` - lay each texel's `(y<<16)|x` at its swizzled
  offset, detile, and confirm it comes back row-major. That is *linear → swizzled → linear* against a
  reference pattern, exactly the acceptance's wording.
- `the_swizzle_is_a_bijection_over_the_block` (injective over all 16,384 texels - invertible).
- And beyond round-trip, worklog 686 detiles the **whole recorded console frame** (4,096 measured
  pixels), a hardware check a round-trip cannot give.

So for the mode a render pass actually reads back, the module and its tests are complete.

## Why `4KB_S` and the block-compressed formats are refused, not written

Not a choice to skip them - a measured wall. obSCEne established (`-6e0f`, resolved) that **no
reachable AGC API exposes surface detiling on retail**: a compute `image_store` into a tiled surface
faults the GPU pipe (`GPU_FAULT_SUSPENDPOINT_TIMEOUT`). The `64KB_R_X` swizzle was obtainable *only*
by rendering into such a surface and reading it back - which works for a render target and not for a
texture. `4KB_S` is a texture tiling; there is no measurement of it and no current way to get one.

This project does not derive a swizzle from a manual, because **a wrong one passes every round-trip
and every linear test and corrupts only genuinely tiled surfaces** (principle 3) - the one failure
the module exists to prevent. So `4KB_S` and block-compressed layouts are refused by name
(`detile_colour_target_refuses_a_mode_it_does_not_model` covers it) until a measurement exists, and
are not needed before a backend samples textures at all. Block-compressed data needs no decode
regardless: Vulkan consumes it natively (roadmap G15, worklog 683). The module doc now carries this
reasoning so a reader meets it in the code rather than re-deriving 8c34.

## Gate state

`orbistoun-gpu` tiling tests 16 pass (round-trip and bijection included); `clippy -p orbistoun-gpu
--lib -D warnings` clean; `./bin/orbistoun prose` exit 0; `cargo fmt --check` clean; identity scan
clean. No new code - a doc-comment change and a verify-and-resolve. No commit.
