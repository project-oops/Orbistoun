# 2026-09-02 - (/loop) Packed typed-buffer FLOAT16 load: driver-widened halves, GPU-verified

Increment 4 of the narrow-format work (D466), and the first that is a float in memory rather than an
integer normalised into one: a 16-bit IEEE half widened to a 32-bit float. Also the first that needed
new module-level infrastructure, so the approach got its own decision - **D467**.

**The three-way choice (D467).** Half->float can go through a hand-rolled bit-unpack, a GLSL ext-inst
(`UnpackHalf2x16`), or a real 16-bit float type widened by `OpFConvert`. Chose `OpFConvert`: the
driver's own IEEE conversion, correct for subnormals/inf/NaN, three instructions, and no shared-header
surgery. The manual unpack was rejected because its subnormal path silently reads zero without
`DenormPreserve` (not declared here); the ext-inst route was rejected as more shared infrastructure for
no better an answer (it will come back for SRGB's `pow`, and FLOAT16 can move onto it then if there is
ever a reason).

- `orbistoun-spirv`: added capabilities `FLOAT16` (9) and `INT16` (22), and ops `UConvert` (113) and
  `FConvert` (115) with unary `SHAPES` rows beside the other conversions.
- Both models (Wavefront and Predicated) now declare the two capabilities and two extra types -
  `OpTypeFloat 16` and `OpTypeInt 16` - and expose them through new `Model::f16_type` / `u16_type`
  accessors, mirroring `f32_type`/`u32_type`. This makes every emitted module require a device offering
  Float16/Int16 - accepted as an assumed cost (D467); the dev GPU offers both.
- `Model::half_to_float_bits`: narrow the masked field to `u16` (`UConvert`), read it as a half
  (`Bitcast` - needs equal widths, hence the narrow first), widen to `f32` (`FConvert`), bitcast back
  to the register's bits. The `packed_component` dispatch gained a `Float` arm calling it.
- The refusal admits `Float` **only when every component is 16 bits** - the narrower packed floats
  (11-/10-bit R11G11B10 channels) are not IEEE halves and stay refused, with a detail that says so.
- Test `a_packed_float16_load_widens_each_half`: halves 0x3C00 (1.0) and 0xC000 (-2.0), both exact in
  both widths, packed low half first into 0xC0003C00 as `BUF_FMT_16_16_FLOAT` (code 29, loaded xy),
  must read back the float bits of 1.0 and -2.0, no epsilon. **Executed on a real device** - which
  means the driver accepted the new capabilities and 16-bit types and the widening carried the value
  through exactly.

clippy (lib and `--tests`) and fmt clean; spirv (15) and translate (108 execute + 13 + 2) suites green;
all five earlier packed tests and the refusal gate unchanged.

Five kinds now translate (UINT, SINT, UNORM, SNORM, FLOAT16). Next: **SRGB** (increment 5) - the sRGB
transfer curve, `x < 0.04045 ? x/12.92 : pow((x+0.055)/1.055, 2.4)` applied to the UNORM value. `pow`
has no core SPIR-V op, so this is where the GLSL ext-inst plumbing D467 deferred finally has to land:
`OpExtInstImport "GLSL.std.450"` (a header slot between OpExtension and OpMemoryModel - the one shared
change avoided so far) + an `OpExtInst` emit path and its `SHAPES` row, then GLSL `Pow` (26). The linear
segment and the select are already-available ops. Then packed stores, then multi-word widths.
