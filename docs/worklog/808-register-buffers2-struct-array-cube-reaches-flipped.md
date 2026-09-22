# 808. `sceVideoOutRegisterBuffers2` gets its own handler — it reads a `SceVideoOutBuffer` array, not v1's raw addresses — and the cube's display goes ready: it enters its 3D render loop and reaches `flipped`, 32,988 calls up from 28

**2026-09-22** — the downstream wall worklog 807 left: the cube's display was *not ready* with no
unimplemented call behind it. The cause is a conflation orbistoun had carried since the two functions
were declared: **`sceVideoOutRegisterBuffers` and `sceVideoOutRegisterBuffers2` were bound to one
handler, and they do not take the same arguments.** Splitting them takes the cube from dying before it
renders to running its full render loop and reaching the `flipped` rung.

## Two functions, two argument layouts, one handler

`sceVideoOutRegisterBuffers(handle, startIndex, addresses, count, attribute)` hands a raw `void*[]`
of buffer addresses at **arg2**. `sceVideoOutRegisterBuffers2(handle, _, _, buffers, count,
attribute, ...)` hands an array of `SceVideoOutBuffer` structs — `{ data, metadata, reserved[2] }`,
32 bytes each, the buffer's address in `data` at offset 0 — at **arg3**, with the count at arg4 and
the attribute at arg5, and **arg2 zero**.

orbistoun bound both names to `video_out_register_buffers`, which reads the address array from arg2.
For a v2 call that is zero, so the handler hit its one refusal — *a null address array* — and answered
`INVALID_HANDLE`. The open-toolchain display registers with v2 (`sceVideoOutRegisterBuffers2(handle,
0, 0, buffers, registered, attr, 0, 0)`, agc_display.c), so `agc_display_init` got a non-zero `rrc`,
set `disp->last_error`, and returned before `disp->ready = 1`. `agc_display_is_ready` is `disp &&
disp->ready`, so the cube saw *display not ready* — one call before it would have gone ready. The call
was **answered, not stubbed**, which is why worklog 807's run showed no unimplemented call behind the
wall: a handler that reports the wrong result is exactly the failure principle 3 forbids, one level up
from a stub.

## The fix

`video_out_register_buffers2` is now its own handler in `crates/orbistoun-video/src/lib.rs`, reading
the addresses from the struct array's `data` fields (stride 32, offset 0). The shared body of the two
— clamp the count, decode the attribute block, store the addresses and shape on the port — is
factored into `read_buffer_addresses` (which takes the stride) and `register_buffer_set`, so v1 keeps
its raw-`void*[]` read and v2 gets the struct read, and neither duplicates the store. Rebound in the
implementations table; the declaration was already arity six, which is right — every value v2 needs
is in the first six registers, the seventh and eighth arguments (both zero) are on the stack.

A test (`register_buffers2_reads_addresses_from_the_struct_array`) registers through a two-struct
`SceVideoOutBuffer` array with arg2 zero, exactly as the SDK calls it, and checks the two `data`
addresses read back in order — the case the old binding refused. The a6b3 attribute test still passes
on v1's path. Video tests 10/10.

## What the cube does now: a live render loop, reach `flipped`

The run is transformed. The display goes ready, and the cube prints *entering 3D rendering loop at 60
FPS* and runs it — flipping frames continuously (`[AGC] flip 1 … flip 10+`, `tile=1 submit=1`), with
per-frame timings (`t-draw-finish-us`, `t-swap-us`) each iteration, to the time limit. **Reach is
`flipped`**, up from `entered`; **32,988 import calls, up from 28** (+32,960); standing 32,986 of
32,988 (100%). Verdict **FURTHER**.

This is the baseline becoming what backlog 037 argued it would: a guest whose whole expected behaviour
is knowable, running its render loop against buffers orbistoun registered, submitting flips orbistoun
completes. The `presented` rung and framebuffer diffing — the correctness signal the operator named,
unreachable while every guest stopped before a surface existed — are now one wall away.

## Where it stops: the draw pipeline, and two input stubs

- **`no hardware pipeline: nothing is drawn`** — the cube flips, but the GPU *draw* (the AGC command
  stream reaching the Vulkan backend and rendering the cube) produces no pixels. This shows as **no
  unimplemented call** — the AGC submission calls are answered — so it is a deeper backend gap, and it
  is the gap between `flipped` and `presented`. That is the next target, and it is where the inbox's
  rendering-cluster requests (the decoded-frame seam, the backend clearing to black, a handover that
  renders at most one frame) live — now reachable because the cube flips.
- Two input calls are stubbed once each, `scePadSetProcessPrivilege` and `scePadGetHandle`. They do
  not block the loop; a small work item, noted not chased.

## Gate state

`crates/orbistoun-video/src/lib.rs` gains `video_out_register_buffers2`, the shared
`read_buffer_addresses` / `register_buffer_set` / `clamp_count` helpers, and the v2 test; the
implementations table rebinds `sceVideoOutRegisterBuffers2`. v1's read path is unchanged in behaviour
(the retail titles that use it are unaffected). The compat record advanced itself (`flipped - 18
imports, 32988 calls, 100% standing`), so the generated compat and status docs are regenerated with
it. `orbistoun-video` tests 10/10, `./bin/orbistoun check` green, worklog index regenerated, identity
scan clean. No commit.
