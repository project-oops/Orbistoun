# D456 - A fault report distinguishes the emulator's own bugs from the guest's, and names a trap instruction

**Status:** decided
**Date:** 2026-09-01

When a guest fault's instruction pointer lies outside the guest image, the report states plainly
that the fault is in this project's own code, not the guest's, and that the import named in the
header is only context. When the faulting instruction is a privileged or trap instruction, the
report names it instead of describing the fault as an ordinary bad memory access.

**Why:** Without this, a fault raised by this project's own code reads as if the guest's last
imported call was where it happened, sending an investigation to the wrong function; and a trap
instruction raises the same host-level fault shape as a wild pointer dereference, reading as a
guest bug where none exists.

**Rejected:** leaving the reader to infer these from the raw register dump alone - costly and
error-prone, as a real investigation demonstrated.
