# D919 - Buffer and float atomics as integer atomics

**Status:** decided
**Date:** 2026-10-20

A dispatch's buffer atomics run as SPIR-V atomics on the word of its bound buffer (D746), with
device scope and acquire-release ordering. They take the address and bounds an untyped access has.
A lane that does not run, or whose access is out of bounds, folds in the atomic's identity, so its
word is left alone without a branch around the atomic. `GLC` (bit 14) returns what the word held,
or zero out of bounds. `buffer_atomic_add` is a sum. An atomic through the memory window rather
than a bound buffer, and a record-indexed one, are refused.

A float minimum or maximum (`ds_min_f32`, `ds_max_f32`, `buffer_atomic_fmin`, `buffer_atomic_fmax`)
is two integer atomics, split by the value's sign:

- A value whose sign bit is clear orders against any float as its bits do as a signed integer, so
  a signed minimum (maximum) keeps the smaller (larger).
- A value whose sign bit is set orders against any float reversed as an unsigned integer, so an
  unsigned maximum (minimum) keeps the smaller (larger).

Each lane gives the atomic its sign does not choose that atomic's identity. This needs no float
atomics extension. NaNs and the order of the two zeros are not modelled. Where one invocation runs
a wave's local share, the float is folded lane by lane, as the integer atomics are.

**Why:** Grand Theft Auto V's loading-screen reduction keeps a float minimum and maximum, in its
local share and then in a buffer.
