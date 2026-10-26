# D993 - A fused multiply-add rounds once

**Status:** decided
**Date:** 2026-10-25
**known_by:** published

Every multiply-add the instruction set defines as fused is translated as one rounding of
`a * b + c`. Every one it defines as unfused rounds the product first. The host evaluator computes
both the same way, so a trace stays an exact oracle of either (D991).

**Which is which** (AMD's published RDNA 2 instruction set, the `VOP2`, `VOP3` and `VOP3P`
opcodes):

- Fused, in single precision: `V_FMA_F32`, `V_FMAC_F32`, `V_FMAAK_F32`, `V_FMAMK_F32`,
  `V_DIV_FMAS_F32` and `V_FMA_MIX_F32`. They are emitted as `GLSL.std.450` `Fma`
  (`Model::f32_fma`). Before this, they were a multiply and an add the driver could fuse or not.
- Fused, in half precision: `V_FMA_F16`, `V_FMAC_F16`, `V_PK_FMA_F16`, `V_PK_FMAC_F16`, and
  `V_FMA_MIXLO_F16` and `V_FMA_MIXHI_F16` where every source is a half. These round once to the
  half they write (`fused_to_half`). The product of two halves is exact in single precision, so
  the sum is the only rounding. Its error is found exactly (`TwoSum`), and an inexact sum whose
  last bit is even is moved one step toward the exact value: it is rounded to odd. Rounding to odd
  at 24 bits and then to nearest at 11 is the one rounding to nearest at 11 (Boldo and Melquiond,
  IEEE Trans. Computers 57(4), 2008). Before this, the sum was rounded to nearest twice, which can
  round a tie the wrong way.
- A mix narrowing to a half with a single-precision source rounds once to single precision, then
  to the half.
- Unfused: `V_MAD_F32`, `V_MAC_F32`, `V_MADAK_F32` and `V_MADMK_F32`. The product and the sum each
  round, denormals flushed (`legacy_multiply_add`, unchanged). ACO selects these for an `fmul` and
  an `fadd` kept apart, and the fused opcodes for `ffma`.
- `V_FMA_F64` is not translated.

**The host evaluator.** It computes `Fma` with the host's own fused multiply-add in every run, not
only in a trace: the operation is exact on every device, so a run that forms addresses may rely on
it.

**The test.** `fused_multiply_add.rs` runs each form on the device and on the host, on operands
where fused and unfused differ, against values worked out by hand:

- In single precision, `a = b = 1 + 2^-12` and `c = -(1 + 2^-11)`. The product
  `1 + 2^-11 + 2^-24` rounds to `1 + 2^-11`, a tie to even. Fused, the result is `2^-24`; unfused,
  zero.
- In half precision, `a = 2^-11 (1 + 2^-10)`, `b = 1 - 2^-10` and `c = 1 + 2^-10`. The exact
  result `1 + 3 * 2^-11 - 2^-31` lies just below the tie between `0x3c01` and `0x3c02`. Rounded
  once it is `0x3c01`; rounded first to single precision it lands on the tie, which rounds to
  `0x3c02`.

**Rejected:**
- Decorating the multiply and the add `NoContraction` for the fused forms: that rounds twice, as
  the unfused forms do.
- Computing the half forms in single precision and narrowing: the double rounding above.
