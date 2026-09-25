# 833. An invented mip level lost the device, and Neverball draws five frames of its title scene

**2026-09-24**: worklog 832 left Neverball's second draw submission ending in
`ERROR_DEVICE_LOST` on a mesh draw. The cause was in the Vulkan backend's texture upload, not in the
shader translator.

## Naming the draw

A device error said only `draw (mesh)`. `drive` now names:

- the command it came from (index and full form);
- the shaders bound at it;
- each stage's user data;
- the bound texture's extent.

When a drive fails, the worker also writes `failed-submission.txt`, one numbered command per line,
and every module as `failed-module-<id>.spv` into the traces directory.

The failing draw was command 149 of 783. It is an ordinary 3-vertex draw with the same two shader
modules frame one ran 450 draws with, and it is the **first draw after a 256x256 texture was bound**.
Frame one only ever bound 16x128.

## The cause

`create_texture` always gave a texture a second level holding one sentinel texel (`COARSE_TEXEL`).
The sampling harness uses it to prove an `image_sample_l` level operand reaches the instruction.
The copy into that level named the **full halved extent**:

- for 256x256, a 128x128 copy (64 KiB) from a 4-byte staging region, which read far past the staging
  buffer and lost the device;
- 16x128 over-read too (8x64 texels, 2 KiB), apparently without leaving mapped memory.

The code's own comment said the halved extent was "for every texture this harness binds, one by
one". That was true of the 2x2 test textures and not of a guest's.

It was also wrong without the over-read. A guest texture got a level it does not have, so a
minified sample would have read the sentinel colour instead of the guest's texels.

## The fix

The upload layout moves into a pure function, `staged`:

- A guest's texture has the one level it carries. The second level is an opt-in, `Bound.coarse_level`,
  which only `draw_with_texture` (the level-operand harness) sets.
- When the coarse level is present, it is filled with the sentinel across its whole halved extent, so
  the copy reads exactly what the buffer holds at any size.

Two unit tests pin both halves: a 256x256 guest texture stages exactly its own texels at one level,
and the harness level stages 128x128 sentinels while a 2x2 still stages the single sentinel. All
Vulkan device tests pass, the sampling ones included.

## What moved

**Neverball now draws five frames at submit**, 783-910 commands each, 0 refused, each written back.
The latest one drawn is kept as `latest-drawn-frame.rgba` in the traces directory; it is overwritten
each frame, so a run that ends mid-frame keeps the last picture the guest completed. It shows the
**title scene's backdrop**:

- the galaxy starfield;
- the planet at the lower left;
- the game's own textures through its own shaders.

White starburst spikes radiate across the right half. That is geometry going wrong, most likely
vertices near the eye with no depth or clip state applied (`REQ-...2ea9`).

The seventh submission's draws did not run. Its new fragment shader does not translate:

> instruction at 0x3c0 cannot be translated: this instruction clamps its result to [0, 1], which is
> not translated

It is the **output clamp modifier**, refused by name. It is the next unit, and a small one.

The cube (GLCB00001) is unchanged: `frames-confirmed: 0x5`, 5 of 5 submissions to completion.

## A note on method

Neverball, oops-gl and oops-mesa are all ours and open. The shader Neverball's second frame needed is
one oops-mesa compiled from GLSL we hold. From here, a wall on these titles is worth checking against
that source first: the GLSL, the NIR, and what ACO emitted for it. Rediscovering the wall one run at a
time is slower. The clamp is one example: ACO sets `clamp` wherever GLSL `clamp(x, 0.0, 1.0)` or
`saturate` folds into an instruction, so every modifier the translator refuses by name can be listed
up front from what ACO emits.

## Gate state

`./bin/orbistoun check` green, worklog index regenerated, identity scan clean. No commit.
