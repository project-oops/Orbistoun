# 541. Prior-art audit: four public PS5 AGC repositories against the GL cube's measurements

**2026-09-14** - read-only comparison; nothing merged, nothing copied. After worklog 539.

## What was read, and under what terms

Four repositories by the same author were cloned to a scratch directory outside every OOPS
tree and read at these commits. Every finding below cites repository, commit and path.

| repository | commit | what it is | licence |
|---|---|---|---|
| `mpereiraesaa/ps5-agc-gears` | `3b9b69f601939449faf20d32b80e680cf9ff441f` (2026-09-12) | lit, depth-tested gears; two frames in flight; soak-tested on FW 12.02 | GPL-3.0-or-later (`LICENSE`, `NOTICE.md`) |
| `mpereiraesaa/ps5-vulkan` | `43e287e09591a094bc6591bcb73e192466782ec1` (2026-09-14) | Vulkan-style API for gfx1013 with runtime shader compilation | GPL-3.0-or-later (`LICENSE`, `LICENSING.md`); several files are adaptations of `blackbearreloaded/ps5-opengl` at `7f9bfabd`, per-file SPDX headers |
| `mpereiraesaa/ps5-xash3d-halflife` | `be0731b4fcc4233349bba5e0d3ff33c79428f695` (2026-09-12) | GoldSrc port on the AGC path with real texture volume | GPL-3.0-or-later (`LICENSE`, `NOTICE.md`) |
| `mpereiraesaa/ps5-homebrew-lab` | `843c40511e0b79b7e50054f77e40b40512e08712` (2026-09-13) | umbrella and research notes (Spanish) | no top-level licence; "individual projects carry their own" (`README.md:96`) |

The task brief said ps5-vulkan carries no project-wide grant. At this commit it does:
`LICENSING.md` states GPL-3.0-or-later for everything not marked otherwise, and the
ps5-opengl-derived files carry their own SPDX headers. Either way the terms are incompatible
with this collection's MIT/Apache licences, so the rule stays the same as the brief: read to
confirm or refute, cite next to our own measurement, never copy or closely paraphrase. Nothing
from these trees is in ours.

**Their provenance, plainly, because it bounds what a citation to them proves.** The published
repositories state a "clean publication boundary" with no copied headers, binaries or captured
buffers (`ps5-agc-gears docs/BACKEND_PROVENANCE.md`). The lab notes behind them describe the
method: a private laboratory whose rules are to "keep firmware, module hash, offset and evidence
for every conclusion" and a documented `ps5debug-NG -> dump -> Ghidra` flow
(`ps5-homebrew-lab docs/GPU_RESEARCH.md:109,138`), with one finding attributed outright to
"the disassembly of 12.02's libkernel" (`docs/FINDINGS.md:425`). So these are public, authored
sources, and citing them is citing authored code; they are not independent of disassembly, and a
fact they confirm is confirmed by a second implementation, not by a second method. Our own
measurement stays the primary record in every row below.

## 1. Our measured facts, confirmed or refuted

| fact (worklog 539, oracle record) | verdict | where they say it |
|---|---|---|
| `SPI_SHADER_PGM_RSRC2_PS` `USER_SGPR` occupies bits 5:1 | **confirmed** | gears packs `ps_rsrc2 = (user_sgprs & 0x1F) << 1` and `gs_rsrc2` the same way with `ES_VGPR_COMP_CNT` at bit 16 (`ps5-agc-gears tools/generate_agc_metadata.py:129-132`); its test fixture uses `ps_rsrc2 = 2` for a one-SGPR pixel shader (`tests/test_ps5_shader_header.c:16`), which is exactly the value that bit us when we meant two |
| T# base address in 256-byte units, high byte in word 1 | **confirmed** | xash `out[0] = address >> 8; out[1] = (address >> 40) \| format << 20 \| ...` (`ps5-xash3d-halflife src/ps5_gfx1013_descriptor.c:81-83`); ps5-vulkan identically (`src/texture_descriptor.c:96-97`); gears does the same for the colour target (`src/ps5_color_target.c:50,59`) |
| `DST_SEL` X,Y,Z,W = 4,5,6,7 reads `FMT_8_8_8_8` channels as R,G,B,A | **confirmed** | ps5-vulkan's sampled-format table gives R8G8B8A8_UNORM selectors 4,5,6,7 (`src/texture_format.c:64`); xash builds word 3 as `SEL_X \| SEL_Y<<3 \| SEL_Z<<6 \| SEL_W<<9` (`src/ps5_gfx1013_descriptor.c:87-89`); ps5-vulkan's buffer path also states "GFX10 DST_SEL encodes X=4, constant-0=0 and constant-1=1" (`src/descriptor_encode.c:54`) |
| A linear image's pitch derives from its width; the `DEPTH` field does not carry it | **contradicted, unresolved** | Both xash and ps5-vulkan put `pitch - 1` into word 4's low 13 bits, with a `PITCH_MSB` at bit 13, for one-level 2D linear images whose row pitch differs from the width, and rely on the implicit 256-byte alignment only for mip chains (`ps5-xash3d-halflife src/ps5_gfx1013_descriptor.c:90-94`; `ps5-vulkan src/texture_descriptor.c:66-69`). Their encoding is word-for-word ours, and xash's HUD sprites and world textures have widths that are not multiples of 64. Our 12.40 run with a 64-wide image at a 128-texel pitch saw no effect from that field. See section 7. |
| Garlic direct memory is sampler-readable | **not exercised** | all three allocate through `sceKernelAllocateMainDirectMemory` and map with protection 0x33 (`ps5-vulkan native/memory_ps5.c:43,49`); none selects a garlic memory type |
| `CB_COLOR0_ATTRIB2`: width in bits 27:14, height in 13:0 | **confirmed** | gears `out[14].value = (height - 1u) \| ((width - 1u) << 14u)` for register 0x3b0 (`ps5-agc-gears src/ps5_color_target.c:64`) |
| `VGT_GS_OUT_PRIM_TYPE` = TRISTRIP (2) for triangles | **confirmed, with a naming question** | gears emits `GS_OUT_PRIM_TYPE 0x00000002` (`tools/generate_agc_metadata.py:162`), but its shader-header "special" carries it at register `0x2ce`, which in our numbering is `VGT_GS_MAX_VERT_OUT`; our context write of 2 goes to `0x29b` (`oops-sdk src/gl/gl_draw.c:309`). The library's linked-CX table (`include/ps5_agc_registers.h:9-13`) has a separate `vgt_gs_out_prim_type` slot. Worth one probe: which offset the library writes when it links. |
| `VGT_SHADER_STAGES_EN` and `GE_CNTL` field layout | **confirmed** | gears builds the stage word from `es_stage_en<<3`, `gs_stage_en<<5`, `primgen_en<<13`, `max_primgroup_in_wave<<15`, `gs_w32_en<<22`, `vs_w32_en<<23` and `GE_CNTL` as `prims \| verts << 9` (`tools/generate_agc_metadata.py:115-137`); our `0x00c12010` and `0x00020080` decode under the same fields |
| `DMA_DATA` fill: SRC_SEL DATA, DST_SEL TC_L2, CP_SYNC | **confirmed** | gears' inline `ps5_agc_dcb_fill_l2_sync` passes `dst_select 3`, `src_select 2`, `cp_sync 1` to the library builder (`ps5-agc-gears include/ps5_agc.h:39-45`); the lab's Stage C/D filled a whole back buffer this way before any shader existed (`ps5-homebrew-lab docs/GPU_RESEARCH.md:59-72`) |
| Depth: `Z_32_FLOAT`, `SW_MODE` 64KB_Z_X, no HTILE, 128 x 128 block extent | **confirmed** | gears' block: `DB_Z_INFO 0x183`, `DB_STENCIL_INFO 0x20000180`, `DB_DEPTH_SIZE_XY (h-1)<<16 \| (w-1)`, HTILE base 0 (`src/ps5_depth_target.c:15-24`); ps5-vulkan pins a 1920 x 1080 D32 layout at pitch 1920, padded height 1152, 0x870000 bytes (`tests/test_depth_layout.c:9`), our exact allocation |
| The submit descriptor: address at +0, size at +8 in dwords, 16 bytes | **confirmed** | gears' host tests "inspect the exact 0x10-byte descriptor" (`docs/BACKEND_PROVENANCE.md`, `ps5_agc_submit` row); the lab's runs submit 122-dword DCBs by count (`docs/GPU_RESEARCH.md:75`) |
| `sceAgcInit` takes a state buffer and the value 13 | **contradicted** | gears calls `sceAgcInit(&state, sizeof(state))` with an 8-byte state (`native/main.c:62,521`) and declares the second argument as a size (`include/ps5_agc.h:18`); obscene passes 13. Both return 0 on their firmware, so "version 13" is not what the argument means. Already on the bus as REQ-20260914T1206Z-b7e4's version sweep; add 8 to the values worth watching. |

The three quarantined NGG facts (D005) are not hand-written anywhere in these trees: gears
compiles its GLSL with the public `amdllpc` for `gfxip 10.1.3` (`tools/build_shader.py:21-22,111`)
and ps5-vulkan compiles at run time through its Mesa/ACO-based pipeline
(`native/runtime_shader.c:78,123`). The primitive-shader sequence therefore comes from LLPC's and
ACO's NGG lowering, which is the same open-source citation D005 already gives, now with two
public consumers proving it runs on this GPU. That is the clean re-derivation path: cite LLPC
`NggPrimShader` and Mesa `ac_nir_lower_ngg`, and note these two trees as consumers.

## 2. Tiling and swizzle

- **Sampled textures are linear in all three**, exactly as ours: `SW_MODE` 0 in word 3, rows at a
  256-byte-aligned pitch, mip levels packed in descending order with each level's pitch aligned
  to 256 bytes (`ps5-xash3d-halflife src/ref_agc_gpu_texture_cache.c:135-152`; `ps5-vulkan
  src/texture_layout.c:37-53`). Nobody samples a 64KB_S or 64KB_R image. Our "biggest gap" is
  theirs too, so there is no prior art to check the tiled T# against.
- **Tiled surfaces exist only as render targets**, and the swizzle is `SW_64K_R_X` (mode 27).
  ps5-vulkan carries CPU de-tiling equations for it, adapted from ps5-opengl's
  "coordinate-ramp receipts", with 128 x 128 texel blocks at 4 bytes per texel (`src/color_detile.c:1-33`).
  That is the compositor's `CB_COLOR0_ATTRIB3` swizzle we measured (`0x08c6c000` decodes to 27).
- **The addrlib question is answered only for linear:** xash names "AddrLib's implicit 256-byte
  pitch alignment" for mip chains and the "GFX10.3 custom PITCH encoding" for single levels
  (`src/ps5_gfx1013_descriptor.c:90-91`); ps5-vulkan names "Mesa gfx10-rsrc.json" for the field
  layout (`src/texture_descriptor.c:91-92`). No Sony-specific delta is claimed by either.
- **What xash does that a demo would not:** a bump allocator and a two-slot transient ring
  for per-frame tables, a generation-tagged resource pool with deferred retirement, a lightmap
  atlas, and an explicit cache contract: after any CPU write into GPU-visible memory it emits the
  library's `sceAgcDcbAcquireMem` over the written range rounded to 256 bytes with engine 1, GCR
  control `0x9000` and 0x190 poll cycles (`src/ps5_cache_contract.c:19-36`, `src/ps5_cache_contract.h:8-10`).
  We rely on the end-of-pipe event's L2 writeback and never invalidate before a read; that
  difference is worth a probe before textures are updated mid-run.
- **Extended user data:** none of the three touches it. Gears passes 24 parameter dwords plus one
  32-bit table pointer as direct user data (`src/gears_scene.c:116`, `src/gears_draw_compose.c:6`),
  xash and ps5-vulkan point user-data dwords at tables. EUD spill stays unmeasured everywhere.
- One convention worth having: gears requires its descriptor table to live where the address's
  high 32 bits equal 2 (`native/main.c:659,673`), because its LLPC shader forms the 64-bit table
  address from a 32-bit user SGPR. Every GPU mapping we have seen sits at `0x2_xxxx_xxxx`, which
  is why it works for them and would for us.

## 3. Presentation, the answer to our 508 ms swap

- **They render straight into the scanout buffer.** Gears takes the runtime's own MRT0 default
  register block from `sceAgcGetRegisterDefaults()` (keyed `0x38e92c91`, 16 registers 0x318..0x3b8)
  and patches base, view, info, extent and `ATTRIB3` to `0x4506c000` under mask `0x4707dfff`, which
  sets `COLOR_SW_MODE` 27 (`src/ps5_color_target.c:14,50-66`). No CPU touches the pixels.
- **Two 64KB_R_X buffers in one allocation**, 64 MiB apart in a 128 MiB reservation, registered
  with `sceVideoOutSetBufferAttribute2` / `sceVideoOutRegisterBuffers2` using format word
  `0x8000000022000000` (`src/ps5_surface.h:10-13`, `src/ps5_videoout.c:42-47`); ps5-vulkan's scanout
  word for B8G8R8A8 is `0x8000000000000000`, "RGB-channel order measured on FW12.02"
  (`native/presentation_format_ps5.h`). The tiled footprint is sized in 512 x 128 pixel bands
  (`src/ps5_surface.c:5-21`).
- **The flip is a packet.** The library's `sceAgcDcbSetFlip(writer, handle, index, mode 1, token)`
  goes into the DCB, then a terminal `RELEASE_MEM` `0xc0064900 0x06000528 0x42010000 <fence> 0 0 0`:
  event 0x28 (BOTTOM_OF_PIPE_TS), 64-bit data 0, interrupt on write, destination TC_L2
  (`src/ps5_present.c:47-51`). Ours is CACHE_FLUSH_AND_INV_TS with a 32-bit word; theirs skips the
  flush because the CB wrote the scanout surface directly.
- **Retirement:** a slot is reusable only when its fence reads zero and the VideoOut flip event
  carrying the exact 48-bit token has arrived (`src/ps5_frame_completion.c:36-50`,
  `docs/ARCHITECTURE.md:41-43,77-78`). Before rendering into a buffer again they emit the driver's
  own wait: `sceAgcDriverWaitUntilSafeForRendering`, sized by
  `sceAgcDriverGetWaitRenderingPacketSizeInDwords` (`native/ps5_agc_native.c:74-75`). Gears measures a
  17 ms frame budget (`docs/HARDWARE_VALIDATION.md:144`).
- ps5-vulkan additionally calls `sceAgcSuspendPoint` right after every submit and documents it as
  "a native GPU suspension opportunity; not a completion fence" (`native/submit_suspend_ps5.h:6-7`).
  We never call it. Given the HP3D timeouts our early faults caused the compositor, that call is
  worth measuring on its own.

## 4. Depth, and the scene that proves it

Their register block is ours plus three writes we leave at zero: `DB_RENDER_CONTROL 0x60`,
`DB_RENDER_OVERRIDE 0x2a` (HiZ and HiS forced off, which is what no-HTILE wants) and
`PA_SU_POLY_OFFSET_DB_FMT_CNTL 0x1e9`; depth control `0xb6` is Z enable, Z write, LESS_EQUAL
(`ps5-agc-gears src/ps5_depth_target.c:15-24`). HTILE is off in all three.

The litmus that turned their "depth on" into a measurement (`ps5-homebrew-lab docs/FINDINGS.md:305-315`):
two exactly overlapping quads, the nearer drawn first with a lit normal, the farther drawn second
with the opposite normal. With the test off the second draw darkens the centre; with LESS_EQUAL
on, the farther quad fails and the bright centre stays. Build inputs differ by one flag, and the
comparison was held for ten seconds each way. That is the scene our cube lacks: draw order
against depth order, on the same pixels, with a colour that tells them apart.

## 5. NGG

All three run the NGG primitive-shader path: gears' stage-enable word sets `primgen_en` and
`gs_w32_en` from LLPC's PAL metadata and the pre-raster stage is a GS object
(`tools/generate_agc_metadata.py:115-123`, `native/main.c:599`); ps5-vulkan's vertex stage is
`PSBC_HW_STAGE_NGG` (`native/runtime_shader.c:78`). No hand-written `GS_ALLOC_REQ`, primitive-export
word or `expcnt` wait exists in any of them; the compilers emit those. Section 1 says what that
buys D005.

## 6. Shutdown

Gears never returns from its entry: on Options it stops producing frames, drains both in-flight
slots by fence and exact flip token, checks its memory guards, closes pad and user service, closes
VideoOut (unregister then close), unmaps and releases direct memory, emits its closing log record,
and calls libkernel's `_exit(0)`. Its release notes state why: on FW 12.02 the title CRT's
`exit/atexit` path "can remove BigApp and still report a game/app failure"
(`docs/RELEASING.md:18-25`, `native/main.c:390-430,754-770`). That matches both of our crashes: a
return from the entry faults at address zero, and a raw exit syscall from application text is
refused with SIGSYS. The approach for us is the same shape: drain, tear down in reverse order,
then the imported `_exit`, never the syscall and never a return.

## 7. Firmware delta, 12.02 against 12.40

Nothing they build from constants disagrees with what we measured, with two exceptions that
need a run to settle:

1. **The linear pitch in `DEPTH`** (section 1). Their encoding is ours; their titles draw with it
   on 12.02; ours showed no effect on 12.40. A T# field is a GPU fact, not a firmware one, so the
   likelier explanation is a difference in our experiment or in the surrounding descriptor words
   (they set word 5 to `MAX_MIP` plus `PERF_MOD 4` where we write zero; they never set
   `RESOURCE_LEVEL` differently). It has to be re-measured with a width that is not a multiple of
   64, which is the case their titles depend on, before "no effect" stands.
2. **`sceAgcInit`'s second argument** (section 1): 8 for them, 13 for obscene.

Two more are firmware-keyed constants of theirs that we have never read on 12.40 and can:
`sceAgcGetRegisterDefaults()` reports 137 entries with 84 context defaults on 12.02
(`ps5-agc-gears src/ps5_color_target.h:9-10`), and the library's indirect register packets are
five dwords, not the four their first composer assumed (`ps5-homebrew-lab docs/FINDINGS.md:245-248`).

## What we believed that this audit shows to be wrong, or unproven

- **"The DEPTH field does not carry a linear pitch on this GPU"** is contradicted by two shipping
  implementations and stays only until re-measured. The comment at
  `oops-sdk src/gl/gl_state.c:545` should say "unresolved" until then.
- **"sceAgcInit takes version 13"** is a reading, not a measurement; a working implementation
  passes 8.
- **"Depth: ON" on the cube proved nothing**, as worklog 539 already said; the two-quad litmus is
  the proof.
- Our `VGT_GS_OUT_PRIM_TYPE` write at `0x29b` works, but the library's own special sits at `0x2ce`
  in gears' header; one of the two names is wrong and a link-time probe will say which.

## What to measure next, ranked by what they have already established

Items 2 to 4 are on the obSCEne request bus as of 2026-09-14: REQ-20260914T1245Z-e8a1 reads
`sceAgcGetRegisterDefaults()` on 12.40, REQ-20260914T1245Z-f3b2 decides the linear-pitch field with
a 100-pixel-wide image, and REQ-20260914T1206Z-b7e4 already sweeps `sceAgcInit`'s argument.

1. The two-quad depth litmus on our path, depth off then on, hash of each: the cheapest claim to
   retire and the one with a proven scene.
2. Render into a `SW_64K_R_X` scanout buffer registered with VideoOut and flip from the DCB with
   the library's SetFlip builder, retiring on fence plus flip token. Their numbers: 17 ms frame
   budget against our 508 ms swap. Read the MRT0 defaults from `sceAgcGetRegisterDefaults()` and
   count them on 12.40 on the way.
3. Re-measure the linear pitch with a 100-pixel-wide RGBA8 image (pitch 128) and `DEPTH`/`PITCH_MSB`
   set as they set it, with word 5 matched to theirs, before and after; whichever way it falls,
   record it.
4. `sceAgcInit` with 8 and with 13, and `sceAgcGetRegisterDefaults` entry counts: two firmware-delta
   probes that cost minutes and belong on the obSCEne bus next to b7e4.
5. `sceAgcSuspendPoint` after a submit, and `sceAgcDriverWaitUntilSafeForRendering` before reuse:
   whether either changes the compositor's tolerance of our queue.
6. The cache contract: an `ACQUIRE_MEM` with GCR `0x9000` after a CPU texture update, against our
   current no-invalidate path, measured by hash.
7. The library's own shader container builder path as a second witness for REQ-20260914T1221Z-c5d9:
   gears and ps5-vulkan both build the `1234`/version-24 self-relative header themselves with six
   SH pairs, and the library accepts it (`ps5-agc-gears src/ps5_shader_header.c:32-72`,
   `ps5-vulkan native/runtime_shader.c:162-181`). Reading is enough; obscene's generated container
   must still be written from our own differential.

## Surprises

- **`sceAgcGetRegisterDefaults()` exists and is the runtime's own answer to "what is the default
  pipeline state".** Every "matching AgcCompositor.elf" comment in obscene and oops-sdk was a
  disassembly stand-in for a call the library exports. That is the clean source for the register
  recipe and belongs on the bus.
- **Nobody samples a tiled texture.** The three most complete public AGC renderers are all linear
  for sampled images, so the tiled-texture gap is the field's, not ours.
- **Both references treat gfx1013's T# as GFX10.3-shaped for the linear pitch**, and their games
  draw. If our re-measurement agrees, the note in worklog 539 was wrong; if it does not, the
  difference is a real finding either way.
- **Their exit is our missing import.** `_exit` from libkernel, after a drain, is the whole answer.

## Addendum, same day: `blackbearreloaded/ps5-opengl`

Cloned at `7f9bfabdddb187a11e4401058eba8c9e55194d0a` (2026-09-10), GPL-3.0-or-later, 442 files,
7.4 MB. The history is a publication, not a development record: 331 commits between 2026-09-06
and 2026-09-10 under one handle, the first commit a 674-line file, then 80 to 90 commits a day
averaging 404 added lines each. The pitch code discussed above was present in the publication
commit (`6c2e928`).

**What it is.** OpenGL 3.3 Core with GLSL 3.30 on a pinned Mesa 26.2.0 (GL state tracker, GLSL,
NIR) plus an 11,332-line gallium driver (`src/gallium/ps5/ps5_screen.c`), a native backend of
about 5,500 lines (`src/platform/`), a 1,349-line EGL facade, and a shader compiler that is
OpenGNM's PSBC (MIT) with a 4,605-line PS5 patch driving NIR through ACO
(`dependencies.json`, `docs/architecture.md`, `THIRD_PARTY_NOTICES.md:21-22`). Shader packages
are ELF containers with the `1234` magic and AMDGPU machine 224 (`src/platform/ps5_agc_package.c:12-16`).
Presentation is fullscreen EGL at fixed 1080p60, 1440p120 or 4K120 into 64 KB-block tiled
display buffers (`src/platform/ps5_scanout.h:39-44`), flipped through `sceVideoOutSubmitFlip`
with `sceAgcDcbSetFlip` and `sceAgcSuspendPoint` also bound (`src/platform/ps5_agc_native_runtime.c:1766-1852`).
Claimed evidence: a frozen CTS campaign of 39,544 results, 37,404 passes and 2,140 reviewed
exclusions; Dear ImGui at 119.88 FPS in 4K; 128 textured cubes at 58 FPS ordinary and 117 FPS
instanced (`README.md`, `docs/performance.md`). Official CTS qualification is stated as blocked.

**Firmware.** Every hardware result is from "one recorded firmware-6.02 console"
(`docs/limitations.md`, `docs/validation.md:114`). So the three public families now sit at three
firmware points: 6.02 here, 12.02 for the other author, 12.40 for us.

**Facts that bear on ours.**
- It writes `descriptor[4] = pitch - 1` for single-level, non-tiled 2D textures whose pitch
  exceeds the width, labelled "GFX10.3 custom linear pitch" (`ps5_screen.c:2633-2640`), and its
  CTS texture cases ran on hardware with it. Third witness for section 7; same encoding.
- It samples tiled targets: render-to-texture colour and depth targets carry a swizzle in T#
  word 3, `0x01800000` (mode 24) for depth textures, with MSAA and array variants
  (`ps5_screen.c:2580-2596`). This is the only public code sampling a tiled image, and it is the
  render-to-texture case, not an uploaded tiled texture.
- It reads `sceAgcGetRegisterDefaults()` and derives its target state from the result
  (`ps5_agc_native_runtime.c:2171`); it also probes for a `sceAgcGetRegisterDefaults2` and logs
  when absent (`:1541-1543`). Second consumer for REQ-20260914T1245Z-e8a1.
- Direct memory type 12 with protection 0x33 (`ps5_agc_native_runtime.c:588-589,2600-2604`): a
  memory-type value neither we nor the 12.02 trees use.
- Render-to-texture barriers are `RELEASE_MEM` events 0x2d, 0x2b and 0x14 with GCR bits
  `0x0070f5xx` and TC_L2 destination, no data (`src/platform/ps5_agc_runtime_backend.c:213-224`).
  Our end-of-pipe event is the 0x14 form; theirs adds the colour-only and depth-only variants.
- It never touches HTILE, garlic memory or extended user data either (zero hits across `src/`).

**Verdict for the strategy question.** As a renderer this is the finished article the others
build on: a real GL, a real compiler, CTS numbers, 4K120. Nothing in oops-gl competes with it and
nothing should try. What it does not have is what oops-gl is for: no hand-assembled shaders and
so no ISA-level ABI facts, no measurement records with fence, clock and hash, no 12.40 data, and
a provenance that rests on a private lab and an MIT shader compiler whose PS5 delta is a
4,605-line patch. It is the best public cross-check available and it is on a different firmware.
