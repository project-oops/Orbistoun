# D002 - Edition 2024, with a pinned build toolchain

**Status:** decided
**Date:** 2026-08-19

The workspace uses edition 2024 with an MSRV floor in `Cargo.toml`, and the build toolchain is
pinned separately in `rust-toolchain.toml`.

**Why:** edition 2024 warns on `unsafe_op_in_unsafe_fn` and requires `unsafe extern` blocks,
which match the unsafe discipline the lints enforce. The toolchain a build uses and the oldest
toolchain the code supports are different facts, and conflating them produces CI failures
nobody can explain.

**Rejected:**
- An unpinned toolchain: host and CI compile with different compilers.
- One number for both: a toolchain bump silently raises the supported floor.
