# D087 - Decoding reports rather than fails

**Status:** assumed
**Date:** 2026-08-19

Neither shader decoding nor packet walking returns an error. A binary that cannot be walked is
a finding carried on the result - desynchronised, overran, or trailing bytes - and
`is_trustworthy` tells a measurement from a lower bound.

**Why:** a sweep over a corpus has to say how many binaries were strange, and one that stops at
the first answers nothing useful. A tool that cannot say when to distrust it has its numbers
quoted anyway.

**Rejected:**
- Returning `Result`: a sweep halts on the first odd binary.
