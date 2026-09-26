# Issues

Open defects, gaps and unmeasured facts, one line each. Delete a line when it is fixed.

## Retail title walls

- PPSA02664, PPSA03416: after one frame, a host `memcpy` reads `0xa8` through a null element buffer at image+0x3f8f0 during command-buffer construction; dump the shader slot tables to see how hardware avoids that path.
- PPSA25872: stops at image+0x3b383b past thread creation; the per-slot upload mask names a slot past the shader's binding count; find who builds the mask.
- PPSA04263: the static object at image+0x5b37e98 (wall at image+0x19676d7) is never constructed; reached by computed dispatch below image+0x4b7c77.
- PPSA04263: the IME update and user-service game-preset stubs need measured values.
- PPSA21564: stops in its own module code at +0x7af792 with no missing import named.
- PPSA28061: null read at image+0x43c4, then an abort after the mapper-parameter call; remaining hypothesis is a side effect of `sceSysmoduleLoadModule`.
- NVRB00001: the menu draws over black because depth, stencil and cull state are decoded but not applied per draw.
- NVRB00001: busy-waits on the process time counter (about 100 million reads in 40 s).

## Graphics

- Depth, stencil and cull state are decoded but not applied per draw.
- The clear-colour state is not decoded; the Vulkan backend refuses `ClearColour`, `Fence` and `present`.
- The Vulkan backend ignores the decoded colour-target state (`CB_COLOR0_BASE`, `ATTRIB2`, swizzle mode).
- gl1-probe fails most checks (scissor, stencil, clip plane, logic op, depth range, mipmaps, readbacks; many rows read `0xbf800000`).
- Detiling covers one 64 KiB block of 32-bpp `64KB_R_X` only; other modes, depths and the multi-block `pipeBankXor` case are refused.
- A second bound texture, a third texture per draw, or a descriptor from two table offsets is refused; no surface cache or format table exists.
- MTBUF 11/10-bit packed floats, `SRGB` and packed stores are refused.
- `v_rcp_f32` translates as an exact division; approximate instructions are not modelled.
- The host side of the resource model (descriptor base to host memory) is unbuilt.
- Submissions run synchronously; hardware queues the buffer and runs alongside the CPU.
- Per-frame host cost remains in prepare, flip write-back, and the first read and detile of each cleared target.
- Hardware's direct-dispatch packet is one dword longer than the documented one; the extra dword is unmodelled.
- `tests/oracle_gl_cube.rs` prints its two pixel hashes instead of asserting them.
- The GUI does not present the headless frame.
- The two-lane subgroup design (64-wide wavefronts on 32-wide host subgroups) needs a decision.
- The single-block dispatch loop is not collapsed and the shader cache is not persisted.

## Kernel, libraries and filesystem

- `libkernel::0x6abac2f3dc6f8cee` (a region allocator: size in arg1, alignment in arg3, header at region end) has no implementation.
- `libc::0x92f57c2dc704346f`, the `printf`-format libc function, `0x53bbd82b51d172db` and `0x7d86501b8094ef57` are unnamed; dispatch them by NID.
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

- Live input is not mapped onto pad bytes; the at-rest image is written on every read.
- `Source::Gamepad` is never read; every pad handle reads port 0.
- Vibration and light bar output are discarded.
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
- Public items with no consumer: `HandleAllocator`, `GuestResult`, `StubReturn::as_raw`, `SymbolDbFile`, `Registry::len`, `SymbolDb::len`, `Protection`, `Region`.
- Five crates cannot be cross-checked for Linux from a Windows host; only CI covers them.
- `agc_driver::tests::a_protected_target_is_trusted_until_a_write_to_it_faults` is flaky.
- The GUI's embedded-docs registry in `orbistoun-gui/src/app.rs` has stale page names and blurbs.
- Other oops-apps titles sync as images rather than staged directories.
