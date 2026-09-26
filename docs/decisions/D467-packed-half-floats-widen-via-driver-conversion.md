# D467 - A packed half-float widens through the driver's own conversion

**Status:** assumed
**Date:** 2026-09-02

A 16-bit packed float component widens to 32 bits by narrowing the field to an unsigned
16-bit value, bitcasting it to a 16-bit float type, and converting with the driver's own
float-conversion instruction.

**Why:** a hand-rolled bit unpack gets subnormals, infinities and NaNs wrong silently unless
denormals are also preserved through the pipeline, which they are not here. Routing the
conversion through the driver's own IEEE path makes those cases the driver's problem instead
of a constant this project would have to get right unobserved.

**Rejected:**
- An extended-instruction unpack: equally correct, but needs new header and type-emission
  infrastructure this decision does not otherwise require.
- A manual bit-unpack ("magic multiply"): needs no new infrastructure, but its subnormal
  path silently reads back as zero without a preserved-denormals declaration.
