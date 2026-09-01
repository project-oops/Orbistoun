# D004 - Fourteen crates arranged as a dependency spine

**decided** · 2026-08-19

`core → trace → elf → nid → mem → hle → loader`, then six subsystem shims and the
CLI. The order is load-bearing, not stylistic: a subsystem shim is never *reached*
until a guest has loaded, allocated, and spawned threads, so writing one early
produces code that cannot be exercised and therefore cannot be trusted.

