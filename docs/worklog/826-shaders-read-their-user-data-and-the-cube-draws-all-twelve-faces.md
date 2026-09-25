# 826. Translated shaders read their user data at entry and the backend pushes it per draw — the cube draws all twelve faces, and Neverball's draws cover the screen

**2026-09-24** — worklog 825 put each draw's user data in the command list and had the backend refuse it
by name until something read it. This is the rest: the translator preloads it, the backend supplies it.

## The translator

- **`wavefront::UserData { first_register, count, block_offset }`**: where a stage's user data lands in
  its scalar registers and where it sits in a **32-word push-constant block** — sixteen words per
  stage, vertex first, **128 bytes**, the smallest `maxPushConstantsSize` a Vulkan device may report.
- **`Wavefront::for_stage` preloads it**: when `count` is non-zero, the module declares the block
  (`buffer::declare_push_constants`, the storage-buffer shape in the `PushConstant` storage class,
  `storage::PUSH_CONSTANT` = 9 from the SPIR-V specification; listed in a mesh module's 1.4 interface)
  and, right after setting the execution mask, loads word `i` of its range into `s[first + i]` — where
  the hardware would have put it before the first instruction. A count of zero declares nothing.
- **`translate_with_user_data`** (in `wavefront` and at the crate root): the existing entry points
  delegate to it with no user data, so every other caller's modules are unchanged; a stage wanting more
  than sixteen words is refused rather than truncated.
- Test `user_data_declares_the_block_only_when_read_and_refuses_too_much`: `s_endpgm` for a fragment
  stage — two words declare one `PushConstant` variable, none declares none and is word for word what
  `translate_windowed_primitive` produces, seventeen is refused.

## The pipeline

`Pipeline::feeding_user_data` (the live path turns it on beside `placing_window_from_shaders`) reads each
stage's layout from the stream: the `USER_SGPR` count from the stage's `SPI_SHADER_PGM_RSRC2` (bits 1-5;
`RSRC2_GS` at register `0x2C8B` and `RSRC2_PS` at `0x2C0B`, both from Mesa's `gfx103.json`), the first
register `s8` for the NGG geometry program — the eight before it are the wave's own, and the cube's
vertex program reads its per-draw word from `s8` — and `s0` for the pixel shader. The layout is part of a
module's cache key. The block's size and stage offsets live in `orbistoun-gpu` for the backend
(`USER_DATA_BLOCK_WORDS`, `USER_DATA_BLOCK_OFFSETS`) and in the translator for the modules; they cannot
import each other, so `the_backends_user_data_block_is_the_translators` pins them equal.

## The backend

Every graphics pipeline layout gains the 128-byte push-constant range, for the fragment stage and the
geometry stage it has (the mesh stage named only when one is built). `VulkanBackend` keeps the block,
accepts `SetUserData` by copying a stage's first sixteen words into its half, and each draw's recording
pushes it after binding its descriptor set. The console-triangle tests are back to asserting that
nothing is refused, as worklog 825 said they would be.

## What the guests draw

```
cube:      29 commands, 0 refused - 582,271 lit pixels, bbox (491, 0) - (1396, 946)
Neverball: 905 commands, 0 refused - every one of 2,073,600 pixels (255, 255, 255, 255)
```

**The cube is there** — all twelve triangles, each face shaded across its vertex colours — where worklog
824 had one face drawn twelve times. It is not yet right: the frame shows the cube's inside, back faces
drawn over front ones. Record B was captured with the depth test and back-face culling on, and orbistoun
decodes that state (`depth_control`, and the rest `REQ-...2ea9` lists) and applies none of it; that is
the next unit.

**Neverball's draws now cover the whole screen, in white.** Its title screen is textured quads, and every
sample reads the one white texel the backend binds by default: the textures the guest's descriptor table
names — whose address now arrives in the pixel shader's `s0:s1` — are not yet bound. Geometry and
coverage are no longer the wall; textures are.

## Gate state

`crates/orbistoun-spirv/src/lib.rs` (`PUSH_CONSTANT`); `crates/orbistoun-translate/src/buffer.rs`
(`declare_push_constants`), `wavefront.rs` (`UserData`, the block, the preload,
`translate_with_user_data`), `lib.rs` (the crate-root entry point, the test);
`crates/orbistoun-gpu/src/backend.rs` (the block constants), `lib.rs`, `pipeline.rs`
(`feeding_user_data`, `user_data_layouts`, `RSRC2_REGISTERS`, the cache key, the test), `agc_driver.rs`;
`crates/orbistoun-gpu-vulkan/src/framebuffer.rs` (the range, the push, `Start`), `lib.rs` (the block,
`SetUserData`), `tests/console_triangle.rs`. All 38 test binaries of the six crates pass.
`./bin/orbistoun check` green, worklog index regenerated, identity scan clean. No commit.
