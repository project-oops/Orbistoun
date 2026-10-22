# D943 - A relative register move selects among the registers past its source

**Status:** decided
**Date:** 2026-10-21
**known_by:** published

`v_movrels_b32 d, s` writes vector register `s + m0` into `d` (AMD's published RDNA instruction
set). It translates as a selection, per lane, among the 32 registers from `s`, keyed on `m0`. Every
model reads a register it names this way, so none needs a register file it can index. An `m0` of 32
or more reads `s` itself, a value no program here has been seen to form.

**Why:** Grand Theft Auto V's shadow-cascade lookup reads three registers at `m0` of 0, 4, 8 or 12
past `v0` (`m0` four times the cascade, `v_readfirstlane_b32` of it), and the translator had no
translation for the instruction, so the draws that use it were refused.

**Rejected:**
- An access chain into the register array with `m0` as its index: only the wavefront model keeps
  one, and each model would need its own form.
