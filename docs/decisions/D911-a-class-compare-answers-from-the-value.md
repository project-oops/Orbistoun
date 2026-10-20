# D911 - A class compare answers from the value's bits

**Status:** decided
**Date:** 2026-10-20

`v_cmp_class_f32` tests whether its first source's class is one its second source's mask names.
The classes are: bit 0 a signalling NaN, 1 a quiet NaN, 2 to 5 negative infinity, normal,
denormal and zero, 6 to 9 positive zero, denormal, normal and infinity (AMD's published RDNA
instruction set).

No one SPIR-V comparison is this test. The class index is computed from the value's bits (sign,
magnitude against infinity and the smallest normal, the quiet bit) and the mask's bit at that
index is read.

The SDWA form takes the word's negate and absolute value on its float source, as ACO's `isfinite`
does with `-|x|` against negative normal, denormal and zero.

**Why:** Grand Theft Auto V's fragment shader tests `isfinite` with `v_cmp_class_f32_sdwa`.
