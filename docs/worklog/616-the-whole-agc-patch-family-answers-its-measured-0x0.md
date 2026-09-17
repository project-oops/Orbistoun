# 616. The whole `sceAgc*Patch*` family answers its measured 0x0, and the wall is now the builders

**2026-09-15** - REQ-...3d1e came back with a return code for every patch; implementing the family is
a fidelity fix, and it named the next wall precisely rather than moving this one

## What the sweep delivered, verified

REQ-...3d1e (the patch family's return codes) resolved from sweep `20260915-203058`. The resolution's
line numbers were off by the usual handful, so I read the rows rather than the citation: **every one
of the nine `sceAgc*Patch*` functions returns `0x0`, each across two argument passes**
(`patch-rc-pass-a` and `-pass-b`, both `0x0`, at `166-agc/patch-cx-reg-set-address`,
`patch-sh-reg-add-registers`, `patch-sh-reg-set-address`, `patch-uc-reg-add-registers`,
`patch-uc-reg-set-address`, `patch-dma-data-dst`, `patch-dma-data-src`, `patch-wait-reg-mem-address`,
`patch-queue-eop-address`). With the Cx pair already measured (REQ-...4386), the whole family is
settled: `0x0` on success.

## What was implemented

All ten patch entry points now share **one handler** that answers the measured `0x0`
(`agc_patch_returns_ok`), replacing the per-function stub. Each patch amends a field of an
already-written packet in place - a `SetAddress` writes an address, an `AddRegisters` extends the
register run, the diffs show exactly which dword - but that is a GPU-submission detail the CPU flow
does not read, and where the guest cares about the bytes it writes them itself (the Cx case, one
eight-byte entry per call, worklog 614). So the handler returns the measured success and writes
nothing, the discipline the Cx patch already set. `implementations()` goes 23 -> 32.

## Why it is a fidelity fix and not a wall-mover, and how I know

It did not move PPSA02664's import wall (222, unchanged) - but it was not nothing: the patches were
answering the loud placeholder, a *wrong* value, and now answer the `0x0` hardware returns.
`unanswered` fell 34 -> 19 and stubs 42 -> 29. The Cx patch was the load-bearing one (worklog 614,
+2 imports); the rest are correctness.

And the run said, plainly, why the memcpy wall stayed: with every patch answering `0x0`, the loop
still faults at `VCRUNTIME140.dll+0x1dc8d`, and the calls just before it are now the **builders** the
guest reaches on the same command buffer, still unimplemented and still answering the placeholder as
a *cursor*:

```
sceAgcDcbWaitRegMem(...) -> 0xf7ff0001
sceAgcDcbPushMarker(...) -> 0xf7ff0001
sceAgcDcbPopMarker(...)  -> 0xf7ff0001
sceAgcDcbSetShRegistersIndirect(...) -> 0xf7ff0001
sceAgcCbDispatch(...) -> 0xf7ff0001
```

A builder returning a placeholder where a real cursor belongs is the exact leak D696 fixed for the Cx
producer and the reservation skeletons: the guest writes the placeholder into its command buffer and
`memcpy`s through it. So the wall is the builder cluster, not the patches - which is precisely the
finding worth having, because REQ-...a70f (resolved in the same sweep) delivered the measured headers
and extents for all of them. The next unit wires them as skeletons; the data is in hand.

## The unnamed one, left unnamed

`libSceAgc::0x7d86501b8094ef57` still answers the placeholder and is on the loop. obSCEne offered a
name (`$fYZQG4CU71c`) but reported the symbol does not resolve on retail userland, so it is a probe
that fails on hardware too - and a name is confirmed by its NID hash agreeing, never by a table, so it
is not added here on obSCEne's say-so. Recorded, not guessed.

## Made to fail

- `every_patch_answers_the_measured_success_not_a_placeholder` (gpu) - each of the ten patch names
  answers `0x0` and specifically not the placeholder the guest reads as a pointer. Closing the family
  as a set is the point: fixing one moves the leak one patch downstream, which is the shape worklog
  614 hit.
- `the_wired_set_is_the_size_the_module_documentation_claims` - 23 -> 32.
- `every_implemented_function_is_written_down` (service) - nine new knowledge entries, each carrying
  its check name, its two-pass `rc 0x0`, and the packet field it amends.
- `the_frontier_matches_what_is_committed` - PPSA02664's record improved (unanswered 34 -> 19);
  regenerated, diff read.

## Gate state

`cargo fmt --all --check` clean, `cargo clippy --workspace --all-targets -D warnings` clean,
`cargo test --workspace` 2,357 pass / 0 fail, worklogs unique, identity scan clean. Frontier
regenerated for the improved records.
