# The division sequence, two thirds of it


The document fetched for the encoding families turned out to unblock the oldest refusal in
the translator. `v_div_fixup_f32` and `v_div_fmas_f32` are translated and executed on
hardware; `v_div_scale_f32` is still refused but for a reason that names an action (D143).

215 tests green across the five shader-side crates, clippy clean.

### Surprises

- **The blocked list had the retarget bug too.** Keyed by `(family, opcode)` with the
  previous generation's numbers - so a paragraph explaining why the division pre-scale is
  refused was, after the retarget, being offered as the reason for whatever instruction
  now sits at 480. Keyed by name now, with a test that every blocked name exists on this
  target: a reason nobody can reach is not a worklist entry.
- **Literal source operands were never translated.** They decode - there is a whole
  fixture for them - but `read_source` had no arm, so any instruction taking a literal was
  refused with "source operand kind is not translated yet". Found only because a test
  needed to name a NaN bit pattern and no inline constant can. Two lines per model.
- **`vcc` is not a scalar register.** `s_mov_b32 vcc, 1` is refused, correctly: the mask
  is named, and `s_mov_b64` is the instruction that writes one. The refusal was right and
  my test was wrong.
- **The reference leaves three terms undefined** - `Quiet`, `underflow`, `overflow` - and
  they are IEEE-754 terms, so they were read from IEEE-754. The check that this is
  right rather than convenient: the reference's own underflow threshold of -150 is exactly
  where IEEE rounding sends a quotient to zero. Two independent routes to one number.
- **One branch of the fixup is unreachable** - `exponent(denominator) == 255` means an
  infinity or a NaN, and both are handled above it. Translated anyway. Deciding a branch
  the reference states is dead is not this translator's call to make.

### Outstanding

`v_div_scale_f32` needs `SPV_KHR_float_controls` - request subnormal preservation for
fp32, then confirm the device reports `shaderDenormPreserveFloat32`. That is the whole
remaining obstacle, and it is a capability question rather than a numerical one.

