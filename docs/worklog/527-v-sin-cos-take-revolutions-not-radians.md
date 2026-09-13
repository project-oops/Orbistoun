# 527. v_sin_f32/v_cos_f32 take revolutions, not radians - correcting worklog 526

**2026-09-12** - reviewing the transcendental expansion before it is trusted

Worklog 526 wired the six unary VOP1 float ops through `GLSL.std.450`. Five of them are right.
`v_sin_f32`/`v_cos_f32` were not, and the test that "verified" them could not have noticed.

## The defect

The hardware's `v_sin_f32`/`v_cos_f32` take their angle in **revolutions**: the instruction
computes `sin`/`cos` of `2*pi * x`, which is why a shader divides a radian angle by `2*pi`
before calling one. GLSL.std.450 `Sin`/`Cos` take **radians**. Worklog 526 mapped
`v_sin_f32 -> Sin(x)` directly, with no scale - so the emulated instruction computed the sine
of `x` radians where the hardware computes the sine of `2*pi * x`. Wrong by a factor of `2*pi`
in the argument, for every angle except the ones where it happens not to matter.

The verification in 526 was exactly one of those: `sin(0.0) == 0.0`, `cos(0.0) == 1.0`. Zero is
the fixed point of both conventions (`2*pi * 0 == 0`), so the assertion passed whether the
translation was right or wrong. It counted a success without checking for the failure - the
principle-3 trap the conventions call out by name.

## The fix (orbistoun-translate)

- `float_unary` now carries a per-instruction `revolutions` flag. For `v_sin_f32`/`v_cos_f32` it
  multiplies the argument by `TWO_PI_F32` (`0x40C9_0FDB`, the f32 bits of `2*pi`) before the
  `OpExtInst`, via the existing `f32_binary(op::FMUL, ...)`. `v_sqrt`/`v_rsq`/`v_exp`/`v_log` are
  unchanged - they take their argument directly.
- `float_unary_trig_produces_the_right_bits` now uses a **quarter turn**: `sin(2*pi*0.25) ==
  sin(pi/2) == 1.0`, `cos(pi/2) == 0.0`. Compared approximately (`2*pi` is not exactly
  representable and the extended trig is not required to be correctly rounded). This angle
  separates the conventions: the un-scaled bug computes `sin(0.25 rad) == 0.247` and
  `cos(0.25 rad) == 0.969`, nowhere near the truth.

## Made to fail before it was trusted

Disabling the scale (setting `revolutions = false`, i.e. reintroducing the 526 behaviour) makes
the new test fail with `sin(2*pi*0.25) should be 1.0; got 0.24740396` - the sine of a quarter
*radian*. Restoring the scale makes it pass. The test detects the exact defect it exists for,
which the `sin(0)` version could not.

## Verified

- `orbistoun-translate`: 112 compute-oracle tests pass, including the three transcendental tests;
  13 unit + 2 agreement pass. `orbistoun-shader` (60 + 7 differential + 10 hostile + 2 measured),
  `orbistoun-spirv` (17), `orbistoun-gpu-vulkan` (all) green.
- `model.rs`/`execute.rs` fmt-clean and clippy-clean (the remaining `orbistoun-gpu` clippy/fmt
  findings are pre-existing in the committed AGC code, untouched here).

## Note on scope

sqrt/rsq/exp2/log2 from worklog 526 are correct and were left as they are. The `abs` case the
plan grouped with these is *not* a standalone instruction: `llvm.fabs.f32` folds into a source
modifier (`v_add_f32_e64 v0, v1, |v2|`), which `apply_modifiers` already implements by clearing
the sign bit. There is no float-abs VOP1 to translate, so `FAbs` has no caller and none was
added - wiring one would be unreachable code.
