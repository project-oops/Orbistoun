# 668. `sceAgcDcbWaitUntilSafeForRendering` is wired as a measured no-op, reversing worklog 665's refusal

**2026-09-17** — worklog 665 refused `sceAgcDcbWaitUntilSafeForRendering` because its only measurement
was the 35-builder sweep's `fail (wrote 0)` - a probe failure, not an empty encoding - and filed a
re-probe (`REQ-...4e91`). The re-probe came back conclusive, so the builder is now wired as the
library-level no-op it turned out to be.

## What the re-probe settled

obSCEne 4e91 (sweeps `20260917-090300`/`101310`) probed it across every state the earlier failure
could have hidden something behind:

- `sceAgcDcbWaitUntilSafeForRenderingGetSize` resolves to `0x0` - the size symbol is **absent** from
  `libSceAgc`.
- bare writer, `arg1 = 0` and `arg1 = 1`: `rc = 0x0`, `bytes = 0`.
- writer prepared through `sceAgcDcbResetQueue(0)` and `ResetQueue(0x400)`: `rc = 0x0`, `bytes = 0`.
- `empty-encoding: 0x1` (true) under every condition.

So it is a genuine library no-op: it returns `0x0` and emits nothing, not "the probe could not reach
it". That is obSCEne's own acceptance option (b) from the re-probe - a library-confirmed empty
encoding - which is exactly the evidence 665 said would let orbistoun wire it rather than refuse it.

## The wiring

`agc_no_op_returns_ok` answers the measured `0x0` and writes nothing, and
`("sceAgcDcbWaitUntilSafeForRendering", agc_no_op_returns_ok)` is in `implementations()`. It
dereferences no argument, so a null handle needs no guard - the same terms as the patch family
(`agc_patch_returns_ok`), though it is a wait/sync no-op rather than a patch, so it gets its own named
handler rather than sharing the patch one. The doc block above `implementations()` now records the
wiring and the 665 → 4e91 history in place of the refusal.

`tests/dcb_wiring.rs::wait_until_safe_for_rendering_is_a_no_op_that_answers_zero` pins that it answers
`0x0` (not the placeholder) and leaves the writer's cursor untouched, on a real handle and a null one;
the wired-set count test moves to 46.

## Why this matters for b7e4

`-b7e4`'s cross-reference (reached AGC builders not in `implementations()`) still printed
`sceAgcDcbWaitUntilSafeForRendering` while it was refused-not-wired. It is now wired, so that name
drops out too - the reached-builder gap b7e4 named is fully closed, both its builders implemented
(`sceAgcDcbResetQueue` as a skeleton in 665, this one as a no-op).

## Gate state

`cargo clippy -p orbistoun-gpu --all-targets -- -D warnings` clean; gpu lib 75 passed; `dcb_wiring`
17 passed; fmt clean; `./bin/orbistoun prose` exit 0; identity scan clean. No commit.
