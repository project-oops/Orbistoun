# 635. Compute dispatches are decoded, and what the translator's memory model means for the V# work

**2026-09-16** - the pipeline now surfaces compute dispatches as it does draws; and the finding that
reframes the frontend buffer decode: the translator has no per-buffer descriptor bindings, it has one
windowed guest-memory buffer

## The reframing (the important half)

The plan (worklog 634) was: decode a guest's V# descriptors, make each buffer resident, and bind it
to the SPIR-V binding the shader gives it. Reading the translator shows that is **not its model**.
`orbistoun-translate/src/buffer.rs` declares exactly two storage buffers for every module: an
**observation** window (binding 0, where the epilogue copies registers) and **guest memory** (binding
1). A guest buffer access is not a separate binding - it is an *address into the flat guest-memory
buffer*, checked against a **window** (`wavefront::Window`, a base and a length) the module is
compiled against. The pipeline carries that window (`Pipeline::with_window`) and it is in the shader
cache key, because two windows produce two different modules.

So the V# decoder built last time is still the right primitive, but its job is not per-buffer binding.
It is to **find the guest-memory region a dispatch touches** so the window can cover it: the base and
length come out of the V#, and the window is what the backend fills from guest memory and binds as
`GUEST_MEMORY`. The frontend buffer decode, correctly framed, is:

1. from the dispatch's V#s (and the window the shader was built against), know the guest-memory region;
2. read that region from guest memory into a `Buffer` resource;
3. bind it as `GUEST_MEMORY` (binding 1), dispatch, and **write the region back** - a guest compute
   shader's output is in guest memory, not in the observation window the translation harness reads.

That last point is the one the executor's compute path does not do yet: `dispatch_into` reads back
binding 0 (observation), which is right for verifying a *translated* shader but not for a *guest's*
dispatch, whose result is in binding 1. Naming it here so the next step aims at the guest-memory
read/bind/writeback rather than extending the observation path.

## What was built

The pipeline decoded draws but not compute dispatches. `dispatch_calls(walk, body)` now reads every
`DISPATCH_DIRECT` (opcode `0x15`) into a `DispatchCall` - the three workgroup counts from its measured
body `[x, y, z, initiator]`, the launch flag left unread - and `submit` emits a
`RenderCommand::Dispatch` for each, in the stream's order. A body too short to hold the three counts
is dropped rather than read past, the same rule the draw decode uses. This is the piece that surfaces
a compute dispatch to the backend that already runs one (worklog 632); the guest-memory binding it
needs is the window flow above.

## Made to fail

- `a_direct_dispatch_decodes_its_workgroup_counts` - a `DISPATCH_DIRECT` with body `[8, 4, 1, flag]`
  decodes to groups `[8, 4, 1]`; a two-word body yields no dispatch.

## Gate state

`cargo test -p orbistoun-gpu` green (1 new lib test; the pipeline integration tests unaffected -
they carry no `DISPATCH_DIRECT`); `cargo clippy --workspace --all-targets -D warnings` clean; fmt
clean. Full-workspace gates running.
