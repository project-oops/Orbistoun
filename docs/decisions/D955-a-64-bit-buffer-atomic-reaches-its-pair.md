# D955 - A 64-bit buffer atomic reaches its pair as one word

**Status:** decided
**Date:** 2026-10-24
**known_by:** published

A dispatch's `buffer_atomic_swap_x2` and `buffer_atomic_or_x2` act on the eight bytes at their
address as one 64-bit step, the data register pair low word first (AMD's published RDNA 2
instruction set, MUBUF). The module reaches the pair through a second view of the compute buffers'
binding, a runtime array of 64-bit words that aliases the 32-bit one, declared with the `Int64` and
`Int64Atomics` capabilities the first time a module needs it. The device enables `shaderInt64` and
`shaderBufferInt64Atomics` where it offers them; a device without them refuses the module.

An or folds zero in where the lane does not land, as the one-word atomics fold their identity
(D919). An exchange has no identity, so it runs inside a branch on whether the lane lands, its
answer left in a private word that holds zero where it did not run. A pair at an odd word is not
one the hardware's 64-bit atomics address, and it is not landed.

**Why:** run 305 of Grand Theft Auto V refused the compute dispatch at `0x400003e15400` as a buffer
access whose descriptor is not in scalar registers: its 64-bit atomics had no operand layout, so
nothing named their descriptor. The program is a single-pass scan. Each group publishes its flag and
sum as one pair with `buffer_atomic_swap_x2`, and later groups poll the pair with
`buffer_atomic_or_x2` of zero until the flag is set.

**Rejected:**
- Two one-word atomics, low word then high. A group polling the pair can see one group's new flag
  beside its old sum, whichever order the words are written in, and add the wrong sum.
- A lock around the two words. A wave spinning on a lock another wave of the same dispatch holds
  needs forward progress across workgroups that Vulkan does not promise.
