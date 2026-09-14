# 530. The shader-capture pipeline: from a command stream to the census corpus

**2026-09-13** - making shader-translation coverage measurable while the retail walls wait on Phase-6

The plan was to run the shader census (`orbistoun-cli shaders`) on real shaders and rank what
blocks translation. Tracing it showed the census had no corpus and no way to get one: the
connective step between the packet walker and the shader decoder did not exist. This builds it.

## What was found first (why the census had nothing to rank)

- **No title reaches shader submission.** PPSA02664 and our own obSCEne probe both wall in the
  AGC command-buffer builders (`sceAgcDcbEventWrite`, `sceAgcDcbDmaData`, `sceAgcDcbWaitRegMem`,
  `sceAgcSetCxRegIndirectPatchAddRegisters`, ...), all unimplemented. PPSA02664 dies inside its
  own `GfxDevicePS5SharedData::CreateWorkload()` there.
- **Shaders are not created; they are pointed at.** `registers.rs` had it right: a submission
  carries shader *addresses* in register writes, not shaders. So a real corpus needs a command
  stream, which needs the AGC command-buffer layer - Phase-6, gated on obSCEne captures. The
  census bottoms out at the same wall as the retail titles.
- The packet walker, the register/shader-address extraction, `decode_program`, and
  `ShaderCorpus::capture` all existed and none of them touched. `registers.rs` names them "two
  tools that never meet".

## What landed (orbistoun-gpu `pipeline`)

`capture_shaders(stream, memory, vocabulary, encodings, operands, corpus)` is the step that
makes them meet: walk the packets, `register_writes` -> `shader_candidates`, and for each
candidate read a window at the address (through the existing `GuestMemory` seam and
`read_window`), find the extent with `decode_program`, and offer exactly those bytes to the
corpus. It returns a `CaptureReport` of what was stored and what each unresolved address was.

The acceptance bar is a **terminator, not a clean decode** - the opposite of
`Pipeline::submit` - because a shader the translator cannot handle yet is precisely what the
census ranks. Filtering to translatable shaders would produce a coverage report that can never
show a gap. Reasoned in **D682**.

## Proven end to end, offline

`tests/shader_capture.rs` drives the whole chain on a machine with no title and no GPU: a
hand-built `SET_SH_REG` stream naming one shader address, a fake address space (`GuestMemory`)
holding a known `v_mov` + `s_endpgm` shader there, capture into a tempdir `ShaderCorpus`, then
the census (`CorpusCoverage`) reading only the corpus and seeing the one shader. Plus the two
edges that matter: the same shader captured twice is stored once (content addressing dedupes a
rebind), and an address memory cannot honour is a reported miss, never a capture of whatever
was there.

## State and what it unblocks

`capture_shaders` is not yet called from a run - nothing produces a command stream until the
AGC builders exist - so it waits, tested, for the first real stream (a title past the builder
wall, or an obSCEne command-stream capture). The moment one arrives, `shaders <dir>` ranks a
real corpus with no further wiring. Verified: `orbistoun-gpu` suite green (31 unit + 17
pipeline + 3 capture + others); `pipeline.rs`/`shader_capture.rs` fmt- and clippy-clean.

Note: `agc.rs`/`agc_driver.rs` also show as modified in the tree - that is a **parallel
session's** AGC render-state work (worklogs 528, 529), not part of this change; a
whole-package `cargo fmt` here incidentally reformatted their in-progress content, which is
harmless (fmt preserves semantics) but is why those files appear touched.
