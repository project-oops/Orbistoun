# 650. The packed floats decode, and finding out cost a latent width-order bug

**2026-09-16** - orbistoun-translate, orbistoun-shader, part (1) of `REQ-20260915T0929Z-4c2e`

`BUF_FMT_10_11_11_FLOAT` and `BUF_FMT_11_11_10_FLOAT` were refused because nothing measured said
how they decode, and a wrong conversion here renders a plausible colour and fails nowhere. obSCEne
measured them (`REQ-20260916T1250Z-b3d4`, sweep `20260916-223136`, `rc-submit 0x0`,
`fence-hit 0x1`): six known words per format through `tbuffer_load_format_xyz`, all three output
floats recorded as raw bit patterns. Thirty-six channel values.

They are not IEEE types: **no sign bit at all**, a five-bit exponent biased by 15, and `width - 5`
bits of mantissa. The decode now reproduces all thirty-six.

## The two edges are what the measurement actually bought

The normal case is arithmetic anyone would write. The edges are not:

- **Exponent 31 is Inf/NaN and the mantissa lands at the top of the single's field**, not the
  bottom. The all-ones word gives `0x7ffe0000` for an eleven-bit channel and `0x7ffc0000` for a
  ten-bit one - the payload shifted up. A zero-extension or a canonical NaN would both have been
  reasonable guesses and both wrong.
- **Exponent 0 is subnormal**, and multiplying the mantissa as an integer by `2^-(14 + mantissa)`
  gives the measured bits exactly. The usual way is to normalise with a leading-zero count, which
  needs an instruction this does not have; the multiply needs none and is rounding-free because
  the scale is exact.

`half_to_float_bits` hands a half to `FConvert` and lets the hardware do it. There is no such
instruction for these, so `narrow_float_to_float_bits` builds all three cases and selects between
them.

## The bug this turned up, which is the part worth remembering

The first run failed on `0x00200401`: component `x` came back as `2^-19` where the device
returned `2.03125`. Thirteen orders of magnitude, from a format whose name looked honoured.

`BufferFormat::widths` is listed **highest bits first**, the order the name lists them - so
`10_11_11` is `[10, 11, 11]` and `x`, the component at bit 0, is the **last** entry. The
translator walked it front-to-back and called the first entry `x`, mirroring the packing.

**It had always done that, and nothing caught it**, because every packed format exercised until
now has equal widths: `8_8_8_8` reads the same in both directions. The order was undocumented on
the field, so neither side was wrong to read it the way it did - which is the actual defect, and
the field now says which order it is.

The two formats pin it from opposite directions and that is why both are in the test: `x` is the
eleven-bit channel in `10_11_11` and the ten-bit one in `11_11_10`. No walk order satisfies both
except the right one. Reverting the reversal fails the test on the third word.

## NaN payloads are compared structurally, and only NaN payloads

The device test asserts bits, not floats within an epsilon - NaN compares equal to nothing, so a
float comparison would silently skip the row that pins the most surprising behaviour.

The one exception is the NaN payload itself. Vulkan does not require a payload to survive
arithmetic and this host does not preserve it: `0x7fde0000` where the target gave `0x7ffe0000`,
one bit apart. Asserting it would fail on a difference that is permitted and is not a translation
bug, so the test asserts the value **is** a NaN - not an infinity, not a zero, not a finite number
- and the exact payload stays pinned in the measured table for a target-side check to compare
against.

## The bug reached twenty-six formats, so the other twenty-four are covered too

The float pair is not where the damage was. Counting the table by width, **twenty-six formats
have unequal widths** and every one of them was decoding mirrored: seven each of `[11, 11, 10]`
and `[10, 11, 11]`, and six each of `[2, 10, 10, 10]` and `[10, 10, 10, 2]`. The `2_10_10_10`
family is the common packed vertex-normal format, and its integer and normalised variants were
admitted the whole time - they translated, and translated wrongly.

`the_two_ten_bit_families_unpack_from_opposite_ends` covers that half.
`BUF_FMT_2_10_10_10_UINT` and `BUF_FMT_10_10_10_2_UINT` are the same four widths in opposite
orders, so **one word decodes differently through each**: `x` is a ten-bit channel in one and the
two-bit channel in the other. A mirrored walk returns the other format's answer, which is what the
failure says when the reversal is removed - `left: 3, right: 291`.

`UINT` is deliberate. It is exact bit extraction with no conversion rule, so the expected values
follow from the field positions and are not a claim about hardware behaviour that would need
measuring first. The floats needed a device to say what they meant; this needed only the order.

## Files

- `crates/orbistoun-translate/src/model.rs` - `narrow_float_to_float_bits`, the `select` helper,
  the admission of widths 11 and 10, and the reversed component walk.
- `crates/orbistoun-shader/src/formats.rs` - what order `widths` is in, said out loud.
- `crates/orbistoun-translate/tests/execute.rs` - both measured tables and the device test that
  replaces the refusal.

## Next

Part (2) of `4c2e` - packed **stores** - is still open and its measurement has also landed: the
same sweep dumped three `tbuffer_store_format_xyzw` elements into a buffer pre-filled with `0xcd`,
and the bytes outside the stored word came back `0xcd`, so **an uncovered bit is left alone rather
than zeroed**. That is the rule that was unguessable; the rounding and saturation rules are in the
same rows and are the remaining work.
