# 2026-09-03 - (/loop) 24 claims, 24 non-claims, and a restore that broke a mutex

```
tests             1991  ->  1992
hardware claims     45  ->    69
outstanding        143  ->    95
```

Changed subject after four ticks on one `.bss` global. The encoder path probes were 48
outstanding measurements; they are now 24 claims and 24 things that were never claims.

## Reading the probe before asserting against it

```c
int res = 0;
int h = sceKernelLoadStartModule(search_paths[i], 0, (void *)0, 0, (void *)0, &res);
```

`res` is **the probe's own initialiser**, reported after the call. All 24 read `0x0` - which is
what a console that never touches the out-parameter produces, and equally what one that writes
zero produces. The reading separates nothing, so it is `OPAQUE` with that reason rather than
sitting in the work queue looking like 24 jobs.

Same family as D485 (a per-boot calibration) and D486 (a sticky status bit): three different
ways a measured value fails to be a property, one question that catches all three - *could this
reading have arisen without the platform doing anything?*

## The other 24 are real and already answered

Every path gives `0x80020002`. Asserted at **32 bits**, the width the probe took it at (D480).

Two of the four directories are right for the wrong reason: `/system/common/lib/` and
`/system/priv/lib/` are in `FIRMWARE_MODULE_DIRECTORIES`; `/system/sys/lib/` and `/system/lib/`
fall out of the bottom of the function into the unrecognised-path refusal. The constant's own
comment already records that hazard biting once, across 234 modules, so the test says so.

Watched failing by flipping the fallback to `EINVAL` - twelve of twenty-four, which is exactly
the half that reaches it, so the failure is attributable and not just present.

## And then I broke a mutex restoring it

The break was surgical. **The restore was not**: I replaced the first
`GuestError::vendor(orbistoun_core::errno::INVALID)` in the file rather than the one I had
changed, and hit the `Deadlock` arm of `scePthreadMutexTrylock` - a **measured** behaviour whose
own comment says it answers `0x8002_0016`.

Two tests then failed in opposite directions, which is what a swap looks like:

```text
a_firmware_encoder_path_is_refused    answered 0x80020016, console 0x80020002
a_second_acquisition_...              left 0x80020002, right 0x80020016
```

Nothing but the whole-workspace run would have caught it - the crate under edit was
`orbistoun-kernel` and the failing claim lives in `orbistoun-service`. The loop prompt has said
*run the WHOLE workspace suite* for weeks; this is the tick that shows why, and the lesson is
sharper than the rule: **an unanchored replace is a different edit from the one you made.** The
break used a two-line anchor and was fine; the restore used one line and was not.

## State

`cargo test --workspace` green - **117 suites, 1992 tests**, 0 failures. clippy `--tests`
clean, fmt clean, identity scan clean. Hardware: **69 claimed**, 95 outstanding, 39 opaque of
200 constants.

Nothing committed. The day holds worklogs 292-347 and D466-D497.

**Next**: `/dev/random` and `/dev/urandom` - the guest asks for both, `device.rs` deliberately
refuses what it cannot serve truthfully, and a seeded stream would serve the guest's contract
and reproducibility at once. Needs its own argument before any code.
