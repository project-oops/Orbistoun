# 2026-09-02 - (/loop) Packed typed-buffer SINT load: sign extension, GPU-verified

Increment 2 of the narrow-format work (D466). Added **SINT** to `packed_buffer_memory`: a signed
component whose high bit is set is negative and must fill the register's high bits with ones.

- `orbistoun-spirv` gained `op::SHIFT_RIGHT_ARITHMETIC` (195, `OpShiftRightArithmetic`) - the signed
  right shift, absent until now - as a constant and a row in the operand-layout table beside the
  logical shifts.
- Extracted `packed_integer_component(model, packed, bit, width, signed)` from the loop, which kept
  `packed_buffer_memory` under the line limit and put the two integer kinds side by side. Unsigned:
  shift down, mask. Signed: shift the high bit to bit 31, then `SHIFT_RIGHT_ARITHMETIC` by `32 - width`
  - two shifts, no mask, exact.
- Test `a_packed_sint_load_sign_extends_each_component`: stores `0x817F0180` (bytes -128, 1, 127, -127),
  loads as `BUF_FMT_8_8_8_8_SINT` (code 61), asserts each reads back as its full sign-extended word. A
  logical shift would have left 0x80/0x81 unchanged and failed it. **Executed on a real device.**

clippy/fmt clean; spirv (15) and translate (105 execute + rest) suites green; the UINT test and the
three refusal tests unchanged.

Next: **UNORM/SNORM** (increment 3) - the first kinds that actually convert. UNORM: extract the
unsigned field, `CONVERT_U_TO_F`, divide by `2^width - 1` (the field's max) to land in 0.0..1.0, then
`BITCAST` the float's bits back into the u32 register. SNORM: signed field, `CONVERT_S_TO_F`, divide by
`2^(width-1) - 1`, and clamp to `>= -1.0` (the reference maps the most-negative code to -1.0, not
below). Check `CONVERT_U_TO_F`/`CONVERT_S_TO_F` and a float `FMAX`/GLSL `FClamp` exist in
`orbistoun-spirv::op`; `FDIV`/`FMUL` already do. Test with values whose float result is exact (e.g. 0,
255 -> 0.0, 1.0 for an 8-bit UNORM) so the assert needs no epsilon.
