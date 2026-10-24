# Issues

Open defects, gaps and unmeasured facts, one line each. Delete a line when it is fixed.

## Retail title walls

- PPSA04263: runs to its 20-million-call budget at `entered` with 213 imports, 93% of the calls `scePthreadYield`: a thread yields in a loop waiting on something nothing provides. The `int 0x41` assertion at image+0x2bf156c after `sceAgcDriverSetTFRing` no longer happens.
- PPSA04263: the IME update and user-service game-preset stubs need measured values.
- PPSA04263: the parent links the compute program at image+0x3d87800 walks hold a cycle, so its loop at +0x60 is cut (D949). The hierarchy builders at image+0x3ce2a00, +0x3ce3400 and +0x3ce3e00 translate to the reference build on keys sorted by their own compare (word one, word zero, word three masked, word two), duplicates included, and build cycles from keys out of that order; the pass that sorts the keys image+0x3cee500 writes, and which builder the title dispatches, are not identified.
- PPSA21564: runs its 120 s limit at `entered` with 263 imports and never flips. Its main thread loops at image+0xdd8e40: it calls image+0xdefa40 (library code linked into the executable; its errors are `0x8a84xxxx`) as `(0x11000000, 0, block, 0xa8)`, sleeps 1 ms, and leaves only when the block's quadword at +0x88 is non-zero. Every other thread is parked on a semaphore or condition. Before the wait it opens video output, registers its buffers, adds eight event-queue events and creates 1,276 shaders, but submits no command buffer and no flip; the loop itself calls no import (19,939 sleeps in 30 s, nothing else at that count), so it waits on memory something else should write. Forcing 0 from each unanswered boot-path import - `scePlayGoOpen`, `sceAcmContextCreate`, `sceRudpInit`, `sceNpEntitlementAccessInitialize`, and the unnamed libSceAgc `0xe486fe61994e8976` and `0x0a4e469ba741de7d` (33 calls each) - moves nothing (2026-10-12, 30 s each).
- `libSceAmpr` serves construction, binding, reset and the APR file read (`crates/orbistoun-gpu/src/ampr.rs`); its other imports have no measured answer (`orbistoun-cli census --imports`).
- NVRB00001: busy-waits on the process time counter (about 100 million reads in 40 s).

## Graphics

- The host depth attachment is never read from or written to the guest's depth surface, whose tiling is unmeasured: a guest that reads depth back sees its own memory, and a depth clear other than a whole-surface fill or a `DB_RENDER_CONTROL` clear draw is not seen.
- A `DB_RENDER_CONTROL` clear draw clears the whole host depth target, not only the pixels it covers.
- Polygon offset (`PA_SU_SC_MODE_CNTL`'s offset enables, `PA_SU_POLY_OFFSET_*`), the depth-bounds test and the depth range are not decoded; a draw asking for depth bounds is refused.
- A `Z_24` depth surface's fill is not recognised as a clear, and a `Z_16` surface is held at 32-bit float precision.
- The clear-colour state is not decoded; the Vulkan backend refuses `ClearColour`, `Fence` and `present`.
- The Vulkan backend ignores the decoded colour-target state (`CB_COLOR0_BASE`, `ATTRIB2`, swizzle mode).
- gl1-probe fails most checks (scissor, stencil, clip plane, logic op, depth range, mipmaps, readbacks; many rows read `0xbf800000`).
- Detiling models `64KB_R_X` (1, 4 bytes), `4KB_D_X` (4, 16), `64KB_D_X` (16), `64KB_S` (4, 16), `4KB_S` (1, 4, 8, 16) and `LINEAR` (1, 4) (`tiling.rs`, `SurfaceLayout::of`); every other mode and depth, 3D and partially-resident layouts, and depth surfaces are refused.
- A pixel shader samples at most sixteen images (`MAX_TEXTURES`); no surface cache or full texture format table exists.
- MTBUF 11/10-bit packed floats, `SRGB` and packed stores are refused.
- `v_rcp_f32` translates as an exact division; approximate instructions are not modelled.
- A pixel shader enabling `SAMPLE_COVERAGE` or `POS_FIXED_PT` is refused: their register content is unmeasured. The ancillary word carries only the layer; its sample and shading-rate bits read zero.
- The host side of the resource model (descriptor base to host memory) is unbuilt.
- An asynchronous submission is still prepared on the guest's thread (D821): shader translation
  waits there, where hardware's driver would have it ready.
- Per-frame host cost remains in prepare, flip write-back, and the first read and detile of each cleared target.
- Hardware's direct-dispatch packet is one dword longer than the documented one; the extra dword is unmodelled.
- `tests/oracle_gl_cube.rs` prints its two pixel hashes instead of asserting them.
- The GUI does not present the headless frame.
- The two-lane subgroup design (64-wide wavefronts on 32-wide host subgroups) needs a decision.
- The single-block dispatch loop is not collapsed.

## Linking

- An orbistoun-aot build has been run only on the machine that built it (D724).
- `extrq` and `insertq` are not rewritten, so a title using them faults on a host CPU without SSE4a (D725); PPSA25872 also carries one `monitorx`/`mwaitx` pair, undefined on any non-AMD host.

## Kernel, libraries and filesystem

- `libkernel::0x6abac2f3dc6f8cee` (a region allocator: size in arg1, alignment in arg3, header at region end) has no implementation.
- `libc::0x92f57c2dc704346f` and the `printf`-format libc function are unnamed; dispatch them by NID, as `0x53bbd82b51d172db` and `0x7d86501b8094ef57` already are.
- `sceKernelGetModuleInfo` is refused because its layout is unmeasured; `110-modules/names` fails with a placeholder.
- The `015-sync/machine-kind` conformance check fails.
- About 37 argument counts across the subsystem crates are provisional.
- Whether `sceKernelGetProcessTime` counts from process start or an epoch is assumed.
- The copy-up overlay has no whiteouts, so a name a lower layer holds cannot be removed.
- Ordered base-plus-patch mounts are unsupported; title identity must hash the winning executable.
- Previous-generation containers are not loadable as test material.
- A multi-threaded guest does not yet get past initialisation with interleaved per-thread call sites.
- The watchpoint thread-start hook installs only for titles with TLS.
- `sceKernelDirectMemoryQuery`: titles keep walking the memory map and refusing it; the accepted map shape is unknown.

## Input and interface

- A host gamepad is read on Windows only; on Linux and macOS a gamepad port reports a neutral
  pad, since the gamepad library links the system's libudev there. Every pad handle reads
  port 0.
- Vibration and light bar output are discarded.
- Audio plays on a host device on Windows only (D819). `sceAudioOutOutputs`, `sceAudioOut2`
  context pushes and format selector 2, whose sample layout is unmeasured, are not heard.
- No layout ships for a second keyboard player.
- Replay determinism holds only to within one input change when a guest polls the pad several times per flip.
- The video and input preference panes are empty; the record button is disabled.
- Stop works only on Windows; suspend is not built.
- No live call inspector or memory/register viewer exists.
- Language and confirm-button settings have no measured encoding.
- The button-offset mapping is inferred.

## Tracing and reporting

- An execution and branch tracer from guest entry is needed to attribute the computed-dispatch walls; validate it on gl1-cube, gl2-cube and mesa-cube, then diff their frames against hardware output.
- The shader census is not wired into the run report's FURTHER/same/BACK verdict.
- The call ring keeps ordering only for 8192 entries; drain it periodically.
- No CLI queries a persisted trace.
- The worker's stderr is not captured into the run report.
- The run report does not record the host machine; the host probe lives in `orbistoun-llm` and sees NVIDIA GPUs only.
- Dispatch throughput is measured in wall time, not process CPU time.
- `orbistoun serve` withholds `call` and `read` after a guest loads.

## Names and provenance

- Most declared functions have no name-derivation record; sweep the set instead of hand-writing `found_by`.
- A `module-strings` record stores its path as invoked, so an absolute host path can reach `symbols/generated.json`.
- Mapped regions are not dumped for string harvesting after relocation.
- Call-position profiles are not used as sub-grammars for the name search.
- `argument-dump` contributes no names; `probe-transcript` is never fed.
- The `/system_ex` directory is unverified.

## Tooling

- A logging service with levels does not exist; diagnostic lines are gated by ad hoc environment variables.
- String literals in the gpu, shader, spirv and translate crates carry baked-in space runs from lost line continuations.
- No automated stub-semantics search exists.
- No error-code corpus with per-entry provenance exists.
- Miri does not run over the host-side crates, though `rust-toolchain.toml` mentions it.
- No synthetic malformed-container fixtures exist.
- Public items with no consumer: `StubReturn::as_raw`, `SymbolDb::len`, `Protection`, `Region`.
- Five crates cannot be cross-checked for Linux from a Windows host; only CI covers them.
- `agc_driver::tests::a_protected_target_is_trusted_until_a_write_to_it_faults` is flaky.
- The GUI's embedded-docs registry in `orbistoun-gui/src/app.rs` has stale page names and blurbs.
- Other oops-apps titles sync as images rather than staged directories.
- `orbistoun-gen measurements` over obSCEne's current logs writes 7,288 measurements (11 MB) against the 638 committed, and repeats ids such as `166-agc/tiling-swizzle:variant-256x256:format`, so `hardware.toml` has not been regenerated since obSCEne's later sweeps.
