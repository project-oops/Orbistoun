# D377 - Syscall-gadget stubs capture the full register set

**Status:** decided
**Date:** 2026-08-29

A stub intercepting a raw syscall gadget saves `rax`, the six argument
registers and `r10` into its own per-stub buffer and reports all eight, instead
of shifting and printing arguments the way an ordinary call stub does.

**Why:** A syscall gadget is entered directly rather than called as a function:
the number to perform arrives in `rax`, and the fourth argument arrives in
`r10` rather than `rcx`, because that is the convention the syscall instruction
itself uses. An argument-shaped report cannot see either, and reporting the raw
registers is the only way to know which convention a caller used.

**Rejected:**
- Reusing the ordinary call-stub reporting shape: it assumes an argument layout
  a gadget does not have.
- A single shared buffer for all gadgets: two guest threads calling two gadgets
  concurrently would overwrite each other's report.
