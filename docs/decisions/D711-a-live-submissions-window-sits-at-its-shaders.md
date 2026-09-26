# D711 - A live submission's window sits at its shader's constant base

**Status:** assumed
**Date:** 2026-09-24

The live submit path places each submission's memory window at the constant 64-bit base its
vertex-stage shader forms: a scalar register pair, each written once by a constant move, feeding
a carry-chained vector add. The window is the largest wholly readable power-of-two span from
that base, at most 2^16 words and never crossing a 4 GiB boundary; with no such base it stays
where it was.

**Why:** the open-toolchain GL context patches its vertex buffer address into the shader as two
literal moves, so the guest's own program says where it reads. This reads a constant the guest
wrote rather than a guessed register vocabulary, and a shader that forms addresses otherwise
gets no window, the old failure rather than a frame drawn from the wrong memory. One window
serves the submission, so the first such pair is taken.

**Rejected:**
- Deriving the base from a command-stream register: nothing measured says which register carries
  buffer addresses.
- The default window at address zero: every vertex fetch is refused.
