# D937 - A buffer bound twice to one range is one buffer

**Status:** decided
**Date:** 2026-10-21
**known_by:** published

A dispatch may bind two descriptors to exactly the same guest range, reading through one and
writing through the other. On the hardware both address the same bytes. The device binds one
buffer at both slots: it is uploaded once, read where either slot reads it, and written back once
from what either slot stored. Each slot hands back that buffer's words.

Two ranges at one base are one buffer too, where they differ only in length, as two loads of
other widths through one descriptor reach: the longer is bound at both, and the shorter reads and
writes a prefix of it. Ranges that overlap otherwise are still refused (D746), since two buffers
would hold two copies of the shared bytes.

**Why:** Grand Theft Auto V's dispatch at 0x125a4 binds one 32-byte buffer at slots 0 and 2,
loading through slot 0 and storing through slot 2. D746 refused it as two overlapping buffers, one
written, and the segment drew nothing. Its vertex fetch dispatch at byte 0x185fc binds one buffer
at slots 11 and 12, `0x3588` and `0x3584` bytes, one written.

**Rejected:**
- Two buffers, the written one copied over the other afterwards: a load through slot 0 after a
  store through slot 2 in the same wave would read the stale copy.
