# 661. The packed float store lands, and its bits come back from the device

**2026-09-17** - `tbuffer_store_format` for the 10/11-bit packed floats is translated and runs: three
measured inputs pack to the exact words obSCEne read on hardware, on this machine's own device. It is
the inverse of the load (worklog 650), unblocked by the rounding measurement obSCEne `-9f1c` returned

## The rounding, and the whole rule

The store was refused because a store must choose a rounding, a saturation and an uncovered-bits rule,
and 4c2e held that guessing any of them renders a plausible colour and fails nowhere. All three are
now measured:

- **Rounding is truncate (round-toward-zero).** obSCEne `-9f1c`: `1.009375` - 0.6 of a mantissa step
  above 1.0 - stores as mantissa 0, where round-to-nearest would carry to 1. (`-2f7a`'s `1.0001`
  could not tell the two apart, which is why `-9f1c` was filed with a value 0.6 of a step up.)
- **Saturation is to the largest *finite* value**, not infinity: `100000` stores as exponent 30,
  mantissa all ones (`-2f7a`).
- **Uncovered bytes are left alone** (`-b3d4`, worklog 650), and the admitted formats cover the whole
  word anyway.

So the rule is **clamp to `[0, max finite]`, then truncate**, verified by hand against all five
measured samples before a line was written.

## What landed

- **`Model::float_to_narrow_float_bits`** (`crates/orbistoun-translate/src/model.rs`) - the SPIR-V
  pack, the inverse of `narrow_float_to_float_bits`: decompose the single, truncate the mantissa to
  the field, rebias 127 to 15, and clamp with three selects (negative or underflow to zero, over-range
  including Inf/NaN to max finite, else the truncated normal).
- **`packed_format_admitted`** now admits a store, but only for a packed float that is exactly one
  word (`kind == Float && total_bits == 32 && widths in {11, 10}`). Every other store - the integers,
  the normalised and scaled formats, the 16-bit halves - stays refused: its rounding and saturation
  are not measured.
- **The store path** in `packed_buffer_memory`, extracted to `packed_store` so the function stays
  under the line ceiling: pack each channel, shift into place, OR into the word, write it keeping the
  previous contents out of bounds.

## Two edges, noted not hidden

Subnormals flush to zero and infinity clamps to the largest finite rather than to the packed infinity.
Both are the choice a clamp already makes, both are rare for colour data, and neither is separately
measured - so they are documented on the method rather than presented as measured fact.

## Tests, including one on the device

- `a_packed_float_store_clamps_and_truncates_to_its_measured_bits` - stores `(1,2,4)`, `(1e5)x3` and
  `(1.009375)x3` through `10_11_11`, reads the packed word straight back with an untyped load, and
  asserts the bits. **It ran on this machine's Vulkan device** (not skipped) and the three words came
  back `0x882003c0`, `0xf7fdffbf`, `0x781e03c0` - exact, saturated, truncated - the bits obSCEne
  measured.
- `a_packed_float_store_translates_to_well_formed_spirv` - the no-device half: the store now translates
  rather than being refused, and the SPIR-V it emits validates.
- `a_typed_buffer_format_needing_conversion_is_refused_by_name` moved onto an `8_8_8_8_UNORM` store,
  which stays refused - the guard now holds the *remaining* refusals.

## Gate state

`cargo test -p orbistoun-translate` **143 passed, 0 failed** (+2, one device-verified); clippy clean (a
`too_many_lines` on the grown function cleared by the `packed_store` split); fmt clean; identity scan
exit 0. No commit.
