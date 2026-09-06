# 2026-09-02 - (/loop) Packed typed-buffer UNORM load: the first converting kind, GPU-verified

Increment 3 of the narrow-format work (D466), and the first that does not just move bits: a **UNORM**
component is its unsigned field divided by the field's maximum, landing in 0.0..=1.0.

- `orbistoun-spirv` gained `op::CONVERT_S_TO_F` (111) and `op::CONVERT_U_TO_F` (112) - integer-to-float
  conversions, distinct from the bitcast the crate already had - as constants and unary rows in the
  operand-layout table beside `BITCAST`. (`CONVERT_S_TO_F` is unused this tick; it is the SNORM
  increment's, added now because the pair belongs together.)
- Added `Model::unsigned_to_float_bits`, the exact counterpart to `f32_binary`'s bitcast: it takes a
  register holding an *integer* and returns the bits of the equal *float* (255 -> 255.0, not the float
  whose bits are 255). Its doc points at `f32_binary`'s warning, since confusing the two is the classic
  silent-wrong-render bug this project keeps flagging.
- Split the per-component work into `packed_component(kind)`, dispatching: `Uint`/`Sint` to the integer
  extractor as before; `Unorm` to `field / maximum` in the float domain. The maximum `2^width - 1` is
  *converted from the integer domain the same way the field is*, rather than written as a float
  constant - so both operands reach the `FDIV` having travelled the identical path, and there is no
  second place to get the constant wrong. The refusal widened from integer-only to admit single-word
  UNORM.
- Test `a_packed_unorm_load_normalises_each_component`: bytes 0, 255, 255, 0 as `BUF_FMT_8_8_8_8_UNORM`
  (code 56) must read back as the float bits of 0.0, 1.0, 1.0, 0.0, asserted with **no epsilon**. Only
  0 and 255 are exactly representable for an 8-bit field (255 is odd, so k/255 is dyadic only at the
  ends), but that still catches the whole mechanism - a bitcast where the convert should be, or a
  missing divide, lands nowhere near 1.0. **Executed on a real device** (no `!! SKIPPED`).

Two housekeeping fixes fell out:

- The existing gate test `a_typed_buffer_format_needing_conversion_is_refused_by_name` used 8-bit UNORM
  as its refused example - now implemented, so it would have gone green for the wrong reason. Repointed
  it to `BUF_FMT_16_16_16_16_UNORM` (code 65): four channels across two words, which the packed path
  refuses on width until the last increment, so the gate keeps guarding after the narrow kinds land.
- `cargo clippy --tests` surfaced six `items_after_statements` warnings - the `const`s in the three
  packed tests sat after the `device_or_skip` guard. Earlier ticks missed them by not running clippy
  with `--tests`; moved the consts above the guard in all three. Nothing was committed, so CI never saw
  them.

clippy (lib and `--tests`) and fmt clean; spirv (15) and translate (106 execute + 13 + 2) suites green.

Next: **SNORM** (increment 3b) - signed field, `CONVERT_S_TO_F` (already added), divide by
`2^(width-1) - 1`, then clamp to `>= -1.0` because the most-negative code divided by that maximum is
just past -1.0 and the reference pins it. The clamp needs a float max - either `op::FORD_LESS_THAN` +
`OpSelect` (check what exists) or a GLSL `FMax` ext-inst; pick whichever the crate can already reach.
Then FLOAT16, SRGB, packed stores, multi-word.
