# D002 - Edition 2024, MSRV 1.85, toolchain pinned to 1.97.1

**decided** · 2026-08-19

Edition 2024 makes `unsafe_op_in_unsafe_fn` warn by default and requires
`unsafe extern` blocks - both align with D013 rather than fighting it. Build
toolchain is pinned separately from the MSRV floor; they are different things and
conflating them causes confusing CI failures.

