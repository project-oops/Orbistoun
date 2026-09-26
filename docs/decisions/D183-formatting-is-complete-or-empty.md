# D183 - Formatting is complete or empty

**Status:** decided
**Date:** 2026-08-21

A formatted-output implementation that cannot honour the whole format writes an empty, terminated
result and reports zero. Floating-point conversions and arguments beyond the captured registers
are refused as their own causes. Formatted writes are counted in every run.

**Why:** a partially rendered string is data the guest cannot detect as wrong; it opens the wrong
file and faults somewhere unrelated. A floating-point argument arrives in a vector register the
trampoline does not capture, a different fix from an unsupported conversion.

**Rejected:**
- Rendering what is understood: invented data.
- One "unsupported" cause: sends the reader to the wrong place.
