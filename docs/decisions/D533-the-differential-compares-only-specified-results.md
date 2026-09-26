# D533 - The differential compares only specified results

**Status:** decided
**Date:** 2026-09-04

The libc differential compares a function against the reference C library only where a
published specification fixes the answer, and compares floating-point results as raw bit
patterns. Transcendental math, random generators, error message text, NaN payloads and returned
addresses are out of it.

**Why:** a bit-for-bit comparison states the contract only when the contract has one answer.
Where the specification permits a range, agreeing cases would pin orbistoun to one
implementation's choices while looking like verification. Bit patterns keep last-place
differences and the sign of zero visible, which a decimal rendering hides.

**Rejected:**
- Comparing every implemented function: forty agreeing transcendental cases would assert an implementation detail nobody promised.
- Comparing decimal renderings: hides `-0.0` against `+0.0` and last-place errors.
