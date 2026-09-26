# D398 - Guest-visible error codes use the platform's own encoding

**Status:** decided
**Date:** 2026-08-30

A guest-facing error status is the platform's error encoding - the POSIX error
number packed into the low bits of a fixed high word - built only from values
this project has confirmed the hardware itself returns, rather than from
placeholder codes invented for cases the encoding does not obviously cover.

**Why:** A single case observed on other tooling was weak evidence, since that
tooling could itself have been inferring the same rule; multiple independent
failures observed directly on the target hardware confirm the encoding is real
rather than assumed. A placeholder code invented to distinguish a case the
real encoding already covers is now unnecessary and is retired.

**Rejected:**
- Keeping placeholder-only codes for cases not directly observed:
  indistinguishable from a value nothing has ever confirmed, once the real
  encoding is known.
