# D915 - A strideless structured buffer bounds its offset as raw

**Status:** superseded by D917 (hardware bounds nothing; obSCEne gt22)
**Date:** 2026-10-20

A buffer descriptor with `OOB_SELECT` 1 (structured: index checked, offset not) and a stride of
zero bounds its accesses as the raw mode does. The offset plus the access's width must fall within
the record count, in bytes. The host binds that many bytes, and the translated check answers an
access past them as out of bounds: it reads zero and drops its write.

This is GCN's stride-zero rule carried over, and it is `assumed`. With no stride every index lands
on record zero, so the index check alone would bound nothing. An obSCEne request asks hardware to
confirm.

**Why:** Grand Theft Auto V's loading-screen program reads a 32-byte table of its own constant data
through word 3 `0x10005004` (mode 1) at register offsets. Without a bound, the read was refused.
