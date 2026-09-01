# D427 - The GPU subsystem starts at the command builders: a PM4 writer, and the two dispatch calls


**measured** - 2026-09-01 (user-directed)

The remaining conformance gaps were subsystem-scale, and the GPU is the project's core and biggest
lever, so this starts it - at the smallest honest place. `165-gnm` probes libSceGnmDriver's command
*builders*: `sceGnmDispatchInitDefaultHardwareState` and `sceGnmDispatchDirect`, which write PM4
packets into a caller's buffer and *touch no GPU*. orbistoun already had the read side (`packet::walk`
decodes a submitted buffer) but no write side and no `implementations()` for the module - the whole
of libSceGnmDriver was declared and stubbed.

Added `packet::build`, the write side of the walker, sharing one header format so a packet built
walks back to the packet it stood for (a test pins exactly that). `sceGnmDispatchDirect` writes the
documented five-dword `DISPATCH_DIRECT` packet (mesa's `PKT3_DISPATCH_DIRECT` = 0x15, the dims, and
the dispatch initiator) and answers 0; `sceGnmDispatchInitDefaultHardwareState` reserves `0x100`
dwords and returns that, filling them with valid **no-op** packets - honest placeholder for a default
state no lawful source here documents, so a guest's submission walks cleanly and the count it trusts
is backed by real dwords rather than the D125 nothing. The submit and flip calls stay declared-only:
they are where Vulkan translation begins, and a stub claiming a frame was submitted is the exact
plausible output this crate exists first to refuse.

Both checks pass, where they were partial (a stubbed placeholder). `dispatch-init` matches hardware
exactly (`0x100`). `dispatch-direct` passes with measure `0x5` where hardware writes `0x6`: the
console's builder emits one dword more than the documented packet, and matching the count by guessing
what it is would be the invention principle 3 forbids - so orbistoun writes the packet it can cite and
the one-dword difference stays unmodelled, inside a pass. gpu tests (26) + clippy clean; tally 516
pass / 8 partial / 4 fail / 15 skip. The subsystem is started: a builder to write PM4, a walker to
read it, and the first two calls that hand a guest a command buffer - the submit→walk→translate path
is the work from here.

