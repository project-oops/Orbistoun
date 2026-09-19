# 692. The handover resolves shader addresses against the guest's regions, and bounds-checks the descriptor

**2026-09-19** — inbox `-5bff`: worklog 687's `submit_dcb` served the pipeline from a `SubmittedBuffer`
that answered only for the command buffer's own bytes, so every shader **GPU** address the stream
named read `None` - the unresolved count fixed at "all unresolved" whatever the truth, no module
prepared, no `BindShader`. And the descriptor's `gpu_addr` was dereferenced guarded only against null,
zero and a 16 MiB ceiling, so an address in no region faulted the host. 3861 stopped at that boundary;
this is the next step.

## Served from the regions the guest was given

`SubmittedBuffer` is gone. In its place, `MappedRegions` (`crates/orbistoun-gpu/src/agc_driver.rs`) is
a `GuestMemory` over the guest's allocated regions: a read whose whole range lies inside one is
answered from host memory (identity mapping, D014), one outside is `None`. The regions are a
process-global set by `set_guest_regions`, which `orbistoun-worker` calls before entering the guest
from the same allocated map it records as `memory_map` in the run conditions - so orbistoun-gpu still
depends on no address-space crate; the worker pushes the map down.

`submit_dcb` now:

- **Refuses a descriptor whose command buffer is outside every region** - `memory.read(gpu_addr,
  length)` returns `None`, and the submit records an empty report and returns `0x0` without a raw
  dereference (was a fault waiting to happen).
- **Serves the shaders from the same `MappedRegions`**, so a shader address inside a region resolves
  (and, the bytes being a real shader, translates - which is what pushes a `BindShader`) and one
  outside is counted unresolved. That count is **D101's first route**: a GPU address the stream
  carries matching an allocation orbistoun handed out.

`SubmissionReport`'s `addresses_resolved` / `addresses_unresolved` now reach the run report:
`SubmissionSummary` (`orbistoun-report`) carries both, `submission_summary()` (`orbistoun-worker`)
fills them, and the `Gap::Submitted` finding's evidence names them ("N of the addresses named resolved
to a region the guest was given, M did not (D101)").

## The four tests (5bff acceptance)

In `agc_driver.rs`: (1) a command buffer naming a shader at a 256-byte-aligned address **inside** a
registered region counts `addresses_resolved == 1` and translates it (a `BindShader`); (2) the **same**
address with only the command buffer's region registered counts `addresses_unresolved == 1`,
`resolved == 0`, nothing translated; (3) a descriptor whose `gpu_addr` is in no region returns `0x0`
and records an empty report without faulting. In `orbistoun-report`: (4) a `SubmissionSummary`'s two
counts survive a serde round-trip. The three that touch the process-global stores hold a shared lock,
so they run one at a time; the 256-byte alignment is because a shader-address register stores the
address in 256-byte units.

## Gate state

Workspace builds with tests; `orbistoun-gpu`/`-report`/`-worker` tests pass (agc_driver 3, the serde
round-trip, the finding test with the new counts); `clippy --all-targets -D warnings` clean on the
three; `status --check` exit 0; `./bin/orbistoun prose` exit 0; `cargo fmt --check` clean; identity
scan clean. No commit.
