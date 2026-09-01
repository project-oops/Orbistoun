# 2026-08-19 - Scaffold


**Done.** Fourteen-crate workspace created and green under the full gate
(`./orbistoun.sh check`): fmt, clippy at `-D warnings`, 22 tests, doctest, rustdoc
with broken-link denial, machete, audit, cargo-deny.

Working today: ELF64 header and program-table parsing, NID hashing and reverse
lookup, the module registry and `guest_module!` macro, stub policy, address-space
validation rules, the trace event model, and two of three CLI commands (`symbols`,
`policy`).

**Surprises.**
- `cargo-machete` flagged 40 aspirational dependencies. Pruning them per D019 left
  `orbistoun-core` with **zero** dependencies, which turned out to be exactly the
  right shape for the bottom of the graph rather than a loss.
- A `[workspace.dependencies.rustix]` sub-table header silently captured every key
  declared after it, moving the internal path dependencies inside it. TOML sub-table
  scoping - use inline tables in that block. Noted in a comment there.
- `cargo-machete` does **not** scan target-specific dependencies, so `rustix` and
  `windows-sys` are unused in `orbistoun-mem` and invisible to the gate.

**Next.** Roadmap phase 0 - synthetic container fixtures.

