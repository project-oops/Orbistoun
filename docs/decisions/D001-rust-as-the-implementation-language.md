# D001 - Rust as the implementation language

**Status:** decided
**Date:** 2026-08-19

orbistoun is written in Rust, one crate per subsystem.

**Why:** guest threads call into host code from threads no runtime created, so the host needs
no garbage collector and no managed runtime. Foreign-function boundaries are explicit rather
than ambient, which suits implementing someone else's ABI. Host-side bookkeeping is most of
the code, and that is where Rust removes whole classes of defect.

**Rejected:**
- C++: the same control, without the memory-safety guarantees on the bookkeeping.
- C#: a managed runtime contends with guest threads it never created.
