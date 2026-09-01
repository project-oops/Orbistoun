# D409 - Layout as a testable plan, console profiles, port reporting, and vaddr provenance


**assumed** - 2026-08-31

Four changes fell out of the payload work, each closing a gap it exposed.

### The libkernel layout is a pure plan, not inline placement

The stub-overruns-its-neighbour collision that corrupted `getpid` (D407) was invisible until it
broke something, because the layout was decided inline in the worker where nothing could test it.
`orbistoun_firmware::plan_layout` now decides the layout as a pure function - each export's slot
kind and whether it overruns the next - and the worker only *places* what the plan says. That
makes the collision class a unit-test failure (there is a test that the compact anchor fits the
measured `0x20` packing and a 64-byte one would not), and `orbistoun-cli firmware` prints the
plan. The first run of the verb surfaced **89 collisions** in the tightly-packed unimplemented
region - real, and previously silent.

Aliases are excluded: two names at one vaddr share a stub, and reporting them against each other
was the false positive that made the old collision output unreadable.

### Named console profiles

Every payload run meant hand-setting firmware and the release string in `shell.toml`.
`--profile ps5-cex-12.40` now presents the measured reference machine (D403, D405) for one run,
from `orbistoun-shell/data/machine-profiles.toml`. Validated in the CLI before the worker spawns,
so an unknown name fails fast with the alternatives; applied in the worker over the loaded
settings via an env the parent sets. The default machine still refuses firmware, so a profile is
the thing that presents one, chosen by name rather than retyped.

### A listening guest says so

`pros check` waits for a guest with a listening socket on its port, and that moment used to pass
silently. `listen()` now names the host address it bound, to stderr and the kernel log - the
service announcing itself, which is what makes orbistoun gradable as a target the moment a
payload reaches `listen()`.

### vaddr provenance: confirmed vs candidate

The vaddr table began as numbers scanned off a firmware file - a candidate, not a measurement,
until obSCEne calls `base + vaddr` on a console and confirms the function behaved (its
`139-exports`). `libkernel-vaddrs.txt` gains an optional third column, `confirmed`, defaulting to
candidate; `getpid` and `sceKernelWrite` carry it (behaviourally confirmed already), and the
layout verb shows each slot's provenance. So "which vaddrs are actually confirmed" is a
machine-readable fact rather than a comment, and it grows as obSCEne confirms more.

Recorded `assumed` because these are tooling and structure choices, not measurements of a guest.

