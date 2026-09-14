# 539. The GPU badge is a measurement, the frame hashes are stable, and textures sample

**2026-09-14** - oops-gl hardware path, GLCube (GLCB00001) on the PS5, after worklog 535

Six questions were put, in order; each answer below is data from the console, and the oracle
record they produced is `oops-sdk/docs/hardware/agc-gl-cube-oracle-fw1240.md`.

## 1. The GPU path is proved, not selected

- **The badge was a code-path flag.** `glIsHardwareAccelerated()` returned `use_hardware`, a
  boolean that the fence miss cleared after the fact. It now returns a measurement: the
  GPU-only clear test at context creation passed, and the last submission's end-of-pipe fence
  *and* GPU clock both came back. Every stream now ends in two `RELEASE_MEM` events, one writing
  the fence word and one writing the 64-bit GPU clock counter (`DATA_SEL` 3); a frame counts
  only when the fence reads `0xbeefcafe` and the clock is non-zero and higher than the previous
  frame's. The demo's HUD shows the clear-test count, the fence and the clock beside the badge.
  The clock counter runs at 100 MHz: 99.75 ticks per microsecond of CPU time over 17.9 s.
- **The CPU rasterizer is out of the console binary.** It compiles only under `OOPS_HOST_BUILD`,
  where the unit tests and the host self-test have no GPU. On the console a refused submit, a
  missing fence or a stalled clock stops drawing for good, logs `HARDWARE FAILURE` with the
  reason, and the HUD turns red with the same reason. Nothing draws in that state.
- **The discriminating test.** At context creation the CPU fills the target with a sentinel,
  one stream clears it with a `DMA_DATA` fill (no draw call), the fence and clock close the
  stream, and the CPU reads it back: 2,073,600 of 2,073,600 pixels came back in the clear colour,
  fence `0xbeefcafe`, clock `0x46f22612f33` on the first run. `glClear` itself now goes through
  the same fill for colour and depth, so on the console no CPU write ever touches a render
  target; the HUD is drawn into the linear buffer after the frame has been measured.

## 2. Determinism

- The pixel hash (FNV-1a 32-bit over all 1920 x 1080 words) is computed console-side from the
  render target after the fence and before the HUD or the flip: the CP copies the finished
  target into CPU-cached memory (`WAIT_REG_MEM` on the fence, then `DMA_DATA` L2 to L2) and the
  hash reads that copy. It never sees the HDMI capture.
- Paused at rotation (25, 35, 10) degrees the hash is `0x9dbfe189` for 116 consecutive frames,
  and `0xc51cec32` textured for 60; both reproduce across separate launches. **Stable.**

## 3. The oracle record

Frame 30 of a paused run, twice: the whole 1052-dword command stream, the vertex and both pixel
shaders, the T# and S#, every buffer address, the fence, the GPU clock and the pixel hash, keyed
by firmware and build. `glRequestHardwareDump()` arms it; the stream is logged as it sits in
memory just before submission.

## 4. Depth

Depth on is the demo's default now. `DB_Z_INFO` names 64KB_Z_X, no HTILE (`TILE_SURFACE_ENABLE`
0, `DB_HTILE_DATA_BASE` 0), and the surface is allocated and cleared over the 128 x 128 block
extent. **The washed-out face is not overdraw.** With the animation paused the demo toggled the
depth test at frames 60 and 90 and compared the frames either side pixel for pixel: 0 pixels
differ in either direction. A convex cube with back-face culling has no overlapping front faces,
so the depth test cannot change a pixel; what washes the face out is the per-vertex Blinn-Phong
specular term where the highlight falls.

## 5. Textures

- **The fault.** `SPI_SHADER_PGM_RSRC2_PS` was `0x2` for the textured shader, meant as "two user
  SGPRs". `USER_SGPR` occupies bits 5:1, so `0x2` is *one*: the descriptor table's low half
  arrived in s0 and the primitive mask in s1 (captured: `s0 = 0x008f0900`, the table's low word;
  `s1 = 0x80000000`, the mask). `s_load` from that pair drew `GPU_FAULT_WAVEFRONT_ERROR_ASYNC`,
  and each one cost SceShellUI an HP3D timeout and a restart. `0x4` is two SGPRs.
- **The T# base address is in 256-byte units** (word 0 = address bits 39:8, word 1 bits 7:0 =
  bits 47:40); the unshifted address was the other fault waiting behind the first.
- **`DST_SEL` X, Y, Z, W = 4, 5, 6, 7** reads channels 0..3 of `FMT_8_8_8_8` as R, G, B, A: the
  gold (255, 215, 0) grid lines of the checker texture come out gold, not cyan.
- **Linear pitch comes from the width, not `DEPTH`.** Rows stored twice as far apart as the width
  sample identically wrong (`0x57a27135`) whether `DEPTH` holds pitch - 1 or 0, against
  `0xc51cec32` for rows at the width. The GFX10.3 convention of carrying a linear pitch in `DEPTH`
  is not this part's. Widths that are not a multiple of 64 pixels are unmeasured.
- `WC_GARLIC` direct memory is readable by the sampler through the queue's page tables; every
  other GPU buffer here is `WB_ONION`, so this is the first garlic read that worked.
- What the *runtime* selects for its own images (swizzle mode, EUD spill) is not measurable
  from here without reading Sony's shaders, which section 6 rules out. The linear mode is what
  this project's own path uses and it is now measured.

## 6. Provenance, plainly

- **Symbol names and NIDs.** oops-gl calls `sceAgcDriverSubmitCommandBuffer`,
  `sceAgcDriverCreateQueue`, `sceAgcDriverSubmitDcb` and friends as weak symbols by name; the
  SELF's import table is built by `selfish` from those names. The names arrived through the open
  homebrew toolchains' published interface lists (OpenOrbis, the ps5-payload-dev SDK), which is
  `supplied` evidence in `docs/PROVENANCE.md`'s vocabulary: outside material, credited in
  oops-sdk's `ACKNOWLEDGEMENTS.md`. That upstream generated its lists from decrypted modules is
  their provenance, not a step this tree performs. Nothing in oops-sdk, oops-apps or selfish
  decrypts a module, and no decrypted module is checked in.
- **One hole, this desk, this day: the compositor's shader.** Worklog 535 says the
  `s_waitcnt expcnt(0)` between the primitive export and the exec change "came from reading
  AgcCompositor's own NGG shader". That sentence undersells it. On 2026-09-14
  `/system/sys/AgcCompositor.elf` was pulled from the console with `pros pull`, its NGG vertex
  shader located, and the words run through `llvm-mc --disassemble` in the toolchain container.
  A module read from a jailbroken console's filesystem is decrypted material, and disassembling
  it is exactly what CONVENTIONS section 1 forbids. Three facts in oops-gl's vertex shader trace
  to that reading: the `expcnt(0)` wait, the primitive-export word `0x20280600` (indices at
  bits 0, 10, 20 with the edge flags set) and the `m0 = vertices | primitives << 12` form of
  `GS_ALLOC_REQ`. All three are also documented by the open-source AMD driver stack (Mesa's NGG
  lowering emits the same sequence) and the ISA reference, which is where a clean re-derivation
  would cite them. The pulled file lives only in a session scratch directory, outside every
  repository, and should be deleted. **This is the owner's call**: keep the three facts with a
  citation to the open sources, or strip and re-derive them; the code does not say which yet.
  **Decided the same day by the owner: keep the three facts, cited to the open sources.** The
  shader comment and oops-sdk's `ACKNOWLEDGEMENTS.md` now cite Mesa's NGG lowering and the ISA
  reference; oops-sdk decision D005 records the choice and its reasoning.
- Everything else measured today (register field positions, descriptor layout, packet formats)
  was derived from the public RDNA ISA reference and the open-source driver headers, then
  confirmed by the console. The T# base-address shift, the `RSRC2` field position and the
  `DATA_SEL` values are cited to those sources in `ACKNOWLEDGEMENTS.md` now.

## Surprises

- **The demo never ran at 60 frames a second.** The HUD said "(60 FPS)" as a constant string.
  Measured phase times for one frame: draw and fence 7-12 ms, hash 11 ms, HUD 94 ms, swap 508
  ms. The swap is the CPU tiling of the uncached linear target into the compositor's surface;
  the HUD is CPU text into the same uncached buffer. The GPU work is a fiftieth of the frame.
  The HUD now prints the measured milliseconds. Rendering straight into the compositor's tiled
  surface (the `64KB_R_X` swizzle its `CB_COLOR0_ATTRIB3` names) would remove the swap and is
  the next RDNA2 fact worth measuring.
- **Reading the target from the CPU costs two frames a second.** The first hash read the linear
  target word by word: 500 ms. The CP copy into cached memory costs the GPU nothing visible and
  the CPU 11 ms.
- **`pros logs` replays the tail of the previous run.** The first lines of every capture belong
  to the instance that was just stopped; four "reference" hashes at the top of a padded-pitch
  run were the old binary's, not a stale cache.
- **A klog line has a length limit** near 128 bytes; longer lines vanish without an error.

## Next

- Render into the compositor's tiled scanout surface and drop the 508 ms CPU swap.
- Measure linear textures whose width is not a multiple of 64 pixels (is the pitch rounded to
  256 bytes, as the address library would predict?).
- Decide the compositor-shader provenance question above and clean the three facts either way.
- Two obSCEne requests filed 2026-09-14, REQ-20260914T1206Z-a1c3 (the submit descriptor layout) and
- Two obSCEne requests filed 2026-09-14, REQ-20260914T1206Z-a1c3 (the submit descriptor layout) and
  REQ-20260914T1206Z-b7e4 (the sceAgcInit gate), each carrying the exact values the probe is
  expected to reproduce, so the tree can cite measurements instead of the 2026-09-10 disassembly.
  A third, REQ-20260914T1221Z-c5d9, asks for the CreateShader checks to run on a container obSCEne
  captures at run time or builds itself, with the retail arrays deleted from its source, and
  lists every record value the replacement must reproduce.
  A fourth, REQ-20260914T1224Z-d2f7, orders the tip clean-up so nothing regresses: ten checks read
  those arrays, not two, so the deletion lands with the generated container, after today's records
  are archived, against a pinned outcome table.
- A clean exit for the title (libkernel's own exit); both current exits crash.
