# 829. The guest's blend state is applied per draw — and Neverball's one-colour frame turns out not to be a blending artefact

**2026-09-24** — worklog 828 read Neverball's uniform dark-blue frame as a translucent fade quad drawn
opaque, and named blending — one of the three decoded state fields `REQ-...2ea9` lists as reaching the
backend with nothing reading it — as the wall. This applies blending, and the frame answers the hypothesis.

## What was built

- **`RenderCommand::SetBlend(BlendControl)`** (`orbistoun-gpu` `backend.rs`): colour target zero's blend
  state for the draws that follow. **Per draw**, because a translucent quad and the opaque geometry
  before it differ only in this state: `push_geometry_commands` emits it before a draw whenever the
  value of `CB_BLEND0_CONTROL` in force there changed (the register now `pub(crate)`), decoded with the
  existing `decode_blend_control`.
- **`framebuffer::blend_attachment`** (`orbistoun-gpu-vulkan`): the Vulkan colour-blend attachment the
  state asks for — factors and combine functions one to one (`gfx103.json` `BlendOp`/`CombFunc`), alpha
  blended with the colour factors when `SEPARATE_ALPHA_BLEND` is off, as the hardware does. **Refused by
  name** rather than approximated: the constant-colour factors (the blend constant registers are not
  decoded), the dual-source ones, the two `BOTH_*` forms, and reserved codes. `build_pipeline` builds each
  draw's pipeline with it; `VulkanBackend` keeps the latest `SetBlend` and refuses a draw whose state does
  not map before building anything. `DispatchError::Unsupported` carries such a name from the framebuffer.
  The draw's start became a struct (`Start`), now that it carries five things.
- **Device test `a_guest_blend_state_mixes_a_translucent_draw_over_the_last`** — `2ea9`'s acceptance for
  blending: green, then red at half alpha under `SRC_ALPHA`/`ONE_MINUS_SRC_ALPHA`/add. Without the state
  the centre is pure red; with it, about half of each. A constant-colour factor refuses the draw.

## What the guests say

```
Neverball: SetBlend { SrcAlpha, DstPlusSrc, OneMinusSrcAlpha, enable: true }
           907 commands, 0 refused - every one of 2,073,600 pixels still (0, 0, 25, 255)
cube:      SetBlend { enable: false }  - 30 commands, 0 refused, 582,271 lit pixels (unchanged)
```

Neverball's stream does turn on standard alpha blending, and the backend now honours it — and the frame
does not change at all. **So the single colour is not a blending artefact**: with blending live, every
draw still produces the same colour everywhere. That points elsewhere — most likely at what the pixel
shader samples: one texture of 16x128, and a frame that is one of its texels multiplied through, suggests
every fragment samples the same texel, i.e. the texture coordinates the vertex stage exports are not the
ones reaching the sample. That is the next question, and it is now a narrow one.

`REQ-...2ea9` stays open: blending has its reader outside `orbistoun-gpu`'s tests and its device test;
depth and stencil control do not yet (the cube needs the depth test and back-face culling, worklog 826).

## Gate state

`crates/orbistoun-gpu/src/backend.rs` (`SetBlend`), `registers.rs` (`CB_BLEND0_CONTROL` visibility),
`pipeline.rs` (the per-draw emission); `crates/orbistoun-gpu-vulkan/src/framebuffer.rs` (`Start`,
`blend_attachment`, `Bound::blend`, the pipeline's blend state), `compute.rs`
(`DispatchError::Unsupported`), `lib.rs` (the backend's blend, the refusal, the name, the test). All 32
test binaries of `orbistoun-gpu`, `orbistoun-gpu-vulkan` and `orbistoun-worker` pass. `./bin/orbistoun
check` green, worklog index regenerated, identity scan clean. No commit.
