# 2026-09-02 - (/loop) Packed typed-buffer SNORM load: the clamp, GPU-verified

Increment 3b of the narrow-format work (D466): **SNORM**, UNORM's signed twin and the first kind that
clamps. A SNORM component is its signed field over the signed maximum `2^(width-1) - 1` (127 for a
byte), spanning -1.0..=1.0.

- No new SPIR-V ops were needed - `CONVERT_S_TO_F` went in with UNORM last tick, and `FORD_LESS_THAN`
  (184) and `SELECT` (169) were already present with layout-table rows. (`FORD_LESS_THAN` was there all
  along; a single-line grep had missed its multi-line table entry.)
- Added `Model::signed_to_float_bits`, the exact twin of `unsigned_to_float_bits` differing only in the
  convert opcode - the pair exists precisely because choosing the wrong one is silent.
- **The clamp, without a float-typed select.** The most-negative code (`-2^(width-1)`) over the maximum
  lands a hair past -1.0 - `-128/127 = -1.0079` - and the reference pins it at -1.0. So after the
  divide the low end is clamped: compare `value < -1.0` (a float compare on the bitcast values), then
  `pick` between -1.0's bits and the value's bits. `pick` selects between two u32 bit patterns, which
  is bit-identical to selecting between the two floats, so no `f32`-typed `OpSelect` is needed - the
  existing u32 `pick` does it.
- Test `a_packed_snorm_load_normalises_each_component`: bytes 127, 0, -128, -127 as
  `BUF_FMT_8_8_8_8_SNORM` (code 57) must read back as the float bits of 1.0, 0.0, -1.0, -1.0, no
  epsilon. The third byte is the clamp case (-128/127 is past -1.0, pinned to -1.0); the fourth is the
  boundary that is already exactly -1.0 (-127/127) and so does *not* trip the strict `<`. A missing
  clamp reads the third as -1.0079 and fails. **Executed on a real device.**

clippy (lib and `--tests`) and fmt clean; spirv (15) and translate (107 execute + 13 + 2) suites green;
the four earlier packed tests and the refusal gate unchanged.

All four integer/normalised kinds now translate. Next: **FLOAT16** (increment 4) - a 16-bit half
unpacked to f32. The clean route is GLSL `UnpackHalf2x16` (ext-inst 62), which needs the
`OpExtInstImport "GLSL.std.450"` id - check whether the builder already imports the GLSL ext set (the
memory model names GLSL450 but that is not the same as an ext-inst import); if not, add the import once
and thread its id through, or unpack the half manually (sign/exponent/mantissa) if the import plumbing
is more than the increment warrants. Then SRGB, packed stores, multi-word widths.
