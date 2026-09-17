# 612. Three more AGC builders wired from the a70f sweep, and why only three

**2026-09-15** - the `20260915-174357` sweep answers part of REQ-...a70f; the honest yield is three
builders, and the reason the rest are not is worth as much as the three

## What was wired

Three `libSceAgc` command builders, each with its header **and** extent measured in the new sweep,
each a single packet, wired as reservation skeletons on the established terms (measured header +
measured extent + zero body, D696) - so `implementations()` goes 19 -> 22:

- `sceAgcCbReleaseMem` - `RELEASE_MEM`, header `0xc0064900`, 32 bytes (`166-agc/cb-release-mem`).
- `sceAgcDcbDmaData` - `DMA_DATA`, header `0xc0055000`, 28 bytes (`166-agc/dcb-dma-data`).
- `sceAgcDcbSetBaseIndirectArgs` - `SET_BASE`, header `0xc0021100`, 16 bytes
  (`166-agc/dcb-set-base-indirect-args`).

Each header reproduces exactly through `command_header(opcode, body_dwords)`, so the write side and
the walker agree. Each hands the guest a **real cursor** where an unwired builder handed it the loud
placeholder to `memcpy` through - which is the wall these clear for the Alex Kidd in Miracle World / Summer Sports
cluster (worklog 600), regardless of the body.

## Why skeletons and not full encoders

Because a single argument pass cannot pin the body, and where a second pass exists it *disagrees*:

- `DMA_DATA` and `RELEASE_MEM` already carried a knowledge entry from an earlier sweep whose
  "zeroed arguments" body was **non-zero** (`b0 bb ff ee ...`), while this sweep's zero-argument pass
  came back all zero. Two captures, two bodies - which is the disagreement that says "do not encode
  the body from one pass". The extent and header are stable across both; the body is the argument
  permutation, so it is zeroed and said to be.
- `SET_BASE`'s one pass carried `1` in the first body dword. The public PM4 layout calls that the
  base-index selector and `1` is the draw-indirect base this builder's name sets - but one pass
  cannot separate a constant selector from an argument, and the address beside it plainly is one, so
  the whole body is zeroed rather than half-encoded.

This is the same line `sceAgcDcbAcquireMem` and the Cx producer already sit on.

## Why only three, when eighteen builders passed in the sweep

The sweep measured far more than three, but most are **not wireable from what it recorded**, and the
honest thing is to say which and why rather than reserve on a guess:

- **Extent only, no byte dump** (so no header to reserve with): `cb-dispatch` (20),
  `dcb-dispatch-indirect` (12), `acb-dispatch-indirect` (16), `dcb-draw-indirect` (20),
  `dcb-draw-index-indirect` (20), `dcb-set-sh-registers-indirect` (20),
  `dcb-set-uc-registers-indirect` (20). Their extents cross-verify against REQ-...72d7, but a
  reservation needs the opcode, and this sweep did not dump their bytes.
- **Acb twins whose header this sweep did not dump**: `acb-acquire-mem` (32), `acb-dma-data` (28),
  `acb-event-write` (8). Tempting to reuse the Dcb twin's header - but the markers prove twins can
  differ: `dcb-push-marker` is `0xc0017904` and `acb-push-marker` is `0xc0017900`, the low byte
  distinguishing them. So a twin's header is not assumed; it is measured or left.
- **Compound or constant-at-one-pass**: `dcb/acb-reset-queue`, `dcb/acb-push/pop-marker`,
  `dcb/acb-wait-reg-mem` were dumped, but as multi-packet streams carrying addresses (`0xeeffb290`)
  or a single constant pass. A single-header skeleton would mis-frame them; encoding the body would
  bake one pass in. They want the second argument pass a70f asked for.
- **Refused by hardware - closed, not wired**: `acb-write-data` faulted (SIGSEGV), `dcb-write-data`
  faulted, `cb-set-sh-registers-direct` wrote nothing, `dcb-set-flip` returned success but wrote
  nothing. These are `not-measurable-this-way` answers, recorded so they are not re-queued.

So a70f is **partly delivered**: the sweep gave extents broadly but packet bytes for only a handful,
and the ask that unblocks the rest is narrow and named (dump the bytes for the extent-only builders,
and a second argument pass for the ones dumped once). Filed back on the bus.

## Made to fail

- `the_wired_set_is_the_size_the_module_documentation_claims` - 19 -> 22, the guard against the
  module prose drifting from the built slice (which it once did, claiming "nothing here is
  implemented").
- `the_measured_skeletons_reserve_their_extent_with_a_zero_body` - each of the three writes its
  measured header, reserves its measured extent, and leaves the body zero with a non-zero argument in
  `arg1`/`arg2` that must not leak in - the line between a reservation and a guess.
- `a_skeleton_refuses_a_null_handle` - a null `arg0` is the measured bad-argument code, not a
  dereference.
- `every_implemented_function_is_written_down` (service) caught that `sceAgcDcbSetBaseIndirectArgs`
  had no knowledge entry; added, with its measured header, extent and one-pass body.

## Gate state

`cargo fmt --all --check` clean, `cargo clippy --workspace --all-targets -D warnings` clean,
`cargo test --workspace` 2,355 pass / 0 fail, worklogs unique, identity scan clean.
