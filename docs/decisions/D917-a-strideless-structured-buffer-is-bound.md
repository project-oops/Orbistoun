# D917 - A strideless structured buffer is bound by its records, not by hardware

**Status:** decided
**Date:** 2026-10-20

Supersedes D915.

A buffer descriptor with `OOB_SELECT` 1 and a stride of zero bounds nothing on hardware. The mode
checks the index alone, and with no stride every access is record zero, so the check never fails.
Any offset reads memory, and any store lands. obSCEne measured this (gt22, check
`000-boot/gt22-oob-select1-stride0`, report SHA-256 `ffb96397…`): a 32-byte descriptor read data at
bytes 32, 40 and 256, and a store at byte 40 landed. The translated check matches it: such an
access is never out of bounds.

The host still has to bind some range, and a register offset says nothing about which. It binds
the record count in bytes: the table the descriptor names, which the program is taken to read
within. That part is `assumed`. A read past it reads zero on the host where the console would read
memory.

**Why:** Grand Theft Auto V's loading-screen program reads a 32-byte table of its own constant data
through word 3 `0x10005004` at register offsets 8 to 24. D915 assumed GCN's stride-zero rule, which
hardware does not follow.

**Rejected:**
- Binding everything readable from the base: the program's own memory is a large mapping, and
  binding it on every dispatch would copy megabytes for a 32-byte table.
