# 535. The GL cube renders on the GPU: five measured fixes in oops-gl's AGC path

**2026-09-14** - oops-gl hardware path, GLCube (GLCB00001) on the PS5, after worklog 534

## What changed

oops-gl's RDNA2 path now draws the demo cube on the GPU: NGG vertex shader plus interpolating
pixel shader, Gouraud colour, back-face culling and the depth test, all checked on the HDMI
capture rather than in the log. At the start of the day the vertex and pixel canaries were never
touched; by the end every fix below is pinned to a measurement taken on the console.

1. **`CB_COLOR0_ATTRIB2` has width in bits 27:14 and height in 13:0.** The macro in `oops/agc.h`
   had them the other way round, on the strength of an earlier check that only proved the register
   mattered, not which field was which. A 96 x 54 px triangle placed at the NDC origin came back in
   the linear framebuffer as runs 0x440 apart - 1088 pixels, which is 1080 rounded up to 64. The
   colour block takes its row pitch from bits 27:14; with width there the runs are 1920 apart and the
   triangle sits where it should. Macro and its unit test corrected; `agc_draw.c` picks the fix up
   through the macro.
2. **`PA_CL_VPORT_YSCALE` is negative.** GL's NDC +y is up and framebuffer rows grow downward. With a
   positive scale every front face was culled, because a triangle wound counter-clockwise in NDC is
   clockwise on screen. The scale is now -h/2 with offset +h/2, and GL culling works unchanged.
3. **`CB_COLOR0_INFO` gets `COMP_SWAP=ALT`.** Bytes B, G, R, A in memory match the 0xAARRGGBB
   framebuffer the CPU path and the flip expect.
4. **The pixel shader must load m0 with the primitive-mask SGPR before `v_interp`.** The SPI hands it
   over in the SGPR after the user data: s0 for the untextured shader, s2 for the textured one. Left
   unset, m0 carried whatever the last wave left, and the interpolator walked the LDS parameter slots
   wrongly: the first quad of every 8 x 8 tile (a wave64 is exactly one tile) interpolated attribute 0
   and the other fifteen read attribute 1, the texcoord. On screen that was a regular 2 x 2 dot grid on
   every face, and faces coloured (u, v, 0) with alpha 0 - the alpha was the tell, because the
   texcoord attribute is {u, v, 0, 0}.
5. **The depth buffer is allocated and cleared over the 64KB_Z_X tiled extent.** `DB_Z_INFO` names
   that swizzle, whose blocks are 128 x 128 pixels, so a 1920 x 1080 surface needs 1920 x 1152 floats;
   the old allocation was exactly w x h and the clear only covered that. With the depth test on and
   culling off the cube occludes itself correctly, so the Z path is real.

Also settled: `PA_CL_CLIP_CNTL` is back to 0 (standard clipping) with no visible change; the
per-frame register block and the shaders lost their experiment scaffolding; `oops_pm4_validate_stream`
in the PM4 unit test now accepts `SET_UCONFIG_REG_INDEX` (0x7a), which the frame uses for
`VGT_PRIMITIVE_TYPE`. `make test` in oops-sdk: 119 pass.

GLCube's HUD now says **GPU** or **CPU** from a live query, `glIsHardwareAccelerated()`, instead of a
static "Pipeline: ... HW" string that would have kept saying HW after a fallback. It also watches for
`/app0/stop.<pid>` every thirty frames, which is how the app is closed from the desk: the title
directory is mounted at `/app0` inside the sandbox and `/data` is not visible there, so the file is
pushed as `/data/homebrew/GLCB00001/stop.<pid>` and seen as `/app0/stop.<pid>`.

## Surprises

- **Read the pixels, not the picture.** Two things settled the dot grid that no screenshot could:
  a one-character-per-pixel dump of a 96 x 48 window of the linear framebuffer, and the raw hex of
  one tile. The dots were at x mod 8 in {0, 1} and y mod 8 in {0, 1}, which is lanes 0..3 of a
  wave64, which is a shader-input problem and not a colour-block one.
- **The primitive-mask SGPR does not look like a mask.** Values seen at pixel-shader entry were
  0x80000000, 0x20000 and 0. Whatever the encoding, copying the word into m0 is what the interpolator
  needs; decoding it is not.
- **obSCEne's 166-agc/primitive-draw recipe is right for its probe and wrong as a template.** Its
  `VGT_GS_OUT_PRIM_TYPE` is POINTLIST, which for a triangle list turns every triangle into a point at
  vertex 0, and the leaked geometry-engine slots starved the compositor until the system killed
  SceShellUI on an HP3D timeout. TRISTRIP (2) is the value for triangles. The `s_waitcnt expcnt(0)`
  between the primitive export and the exec change, without which no pixel wave ever launched, came
  from reading AgcCompositor's own NGG shader; and user data register 0 arrives in s8, not s0, for the
  NGG stage on this hardware. obSCEne should carry those three as measured facts next to the recipe.
- **Two exits, both crashes.** Returning from the eboot entry faults at rip 0; a raw `SYS_exit`
  through the syscall helper draws SIGSYS. A clean exit needs libkernel's own exit, which the SDK does
  not import yet. The stop file therefore ends the app with a coredump, as the controller did before.
- **oops-apps has no `make check`** although the top-level CLAUDE.md names it; `make title` in the
  app directory is the gate that ran.

## Next

- Take the vertex index from the hardware (v5) instead of the lane index, and batch more than one
  triangle per draw; the per-draw packet count is what caps the frame today.
- The textured pixel shader's m0 comes from s2 by inference from the untextured measurement; run the
  textured path and confirm.
- Feed obSCEne the three facts above as knowledge-base entries.
