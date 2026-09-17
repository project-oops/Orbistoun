# 618. The a70f builder cluster wired, and the proof that PPSA02664's wall is not the placeholders

**2026-09-16** - nine more builders answer real cursors from their measured headers; the wall did not
move, and that is the finding

## What was wired

REQ-...a70f delivered the packet bytes for the builders it had only given extents for before (sweep
`20260915-203058`). Nine single-packet builders, each with its header **and** extent verified in the
raw log, are now reservation skeletons - `implementations()` 32 -> 41:

| builder | opcode | header | extent |
|---|---|---|---|
| `sceAgcCbDispatch` | 0x15 | `0xc0031500` | 20 |
| `sceAgcDcbDispatchIndirect` | 0x16 | `0xc0011600` | 12 |
| `sceAgcAcbDispatchIndirect` | 0x16 | `0xc0021600` | 16 |
| `sceAgcDcbDrawIndirect` | 0x24 | `0xc0032400` | 20 |
| `sceAgcDcbDrawIndexIndirect` | 0x25 | `0xc0032500` | 20 |
| `sceAgcDcbSetShRegistersIndirect` | 0x63 | `0xc0036300` | 20 |
| `sceAgcDcbSetUcRegistersIndirect` | 0x64 | `0xc0036400` | 20 |
| `sceAgcDcbStallCommandBufferParser` | 0x42 | `0xc0004200` | 8 |
| `sceAgcAcbAcquireMem` | 0x58 | `0xc0065800` | 32 |

Each is a `packet::build::reservation(opcode, body_dwords)` - the header rebuilt from the measured
opcode through `command_header`, body zeroed, one generic helper rather than nine near-identical
skeleton functions. `sceAgcAcbAcquireMem`'s header was dumped for the Acb form this time, so the twin
is measured rather than the assumption I refused to make in worklog 612.

## What it did, and the finding it is worth

Stubs answered fell **29 -> 26** and two builders left the placeholder list (`sceAgcCbDispatch` and
`sceAgcDcbSetShRegistersIndirect`, both on PPSA02664's command-buffer path). But the wall did **not**
move: PPSA02664 stays at 222 imports, still faulting in the host `memcpy` at `VCRUNTIME140.dll+0x1dc8d`
(`read of 0xa8`).

That is the load-bearing result, not a disappointment. Two batches now - the patch family (worklog
616) and this builder cluster - have each answered more of the cluster's placeholders and neither has
moved the wall. So **the memcpy fault is not the placeholder cursors.** It is a null base plus `0xa8`
(`r13`/`r14 = 0`) in the Cx-indirect patch loop, where the guest `memcpy`s its eight-byte register
entries (worklog 614): the *source* of one copy is null, and that null does not come from a builder
answering `0xf7ff0001` - clearing those would have moved it, and across two batches it did not. The
next investigation is that null: which structure the guest expected populated and reads at `+0xa8`,
not another builder to wire.

## What is left returning a placeholder, and why it is not chased here

Three AGC calls on PPSA02664's path still answer the placeholder: `sceAgcDcbPushMarker`,
`sceAgcDcbPopMarker` (12-byte `SET_UCONFIG_REG` to the CP marker register `0x342`, a reserved low bit
set in the measured header), and `sceAgcDcbWaitRegMem` (a 56-byte compound of three packets). They are
correctness worth doing - a builder should hand a real cursor - but the evidence above says they are
not this wall, so they wait behind the null investigation rather than being rushed in with their
raw-header and compound shapes. The unnamed `0x7d86501b8094ef57` also answers the placeholder and
obSCEne reports it does not resolve on retail, so it is a probe that fails on hardware too.

## Made to fail

- `the_a70f_cluster_reserves_its_measured_headers_and_extents` (gpu) - each of the nine writes its
  measured header (asserted little-endian, so a wrong opcode or count cannot pass), reserves its
  measured extent, and leaves the body zero with a non-zero argument that must not leak in.
- `the_wired_set_is_the_size_the_module_documentation_claims` - 32 -> 41.
- `every_implemented_function_is_written_down` (service) - three new knowledge entries (the six others
  already had one); each carries its measured header and extent.
- `the_frontier_matches_what_is_committed` - PPSA02664's record improved (unanswered fell as the
  builders answered); regenerated, diff read.

## Gate state

`cargo fmt --all --check` clean, `cargo clippy --workspace --all-targets -D warnings` clean,
`cargo test --workspace` 2,359 pass / 0 fail, worklogs unique (renumbered off a collision with a
concurrent 617), identity scan clean. Frontier regenerated.
