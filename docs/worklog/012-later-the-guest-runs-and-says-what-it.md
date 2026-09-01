# 2026-08-19 (later) - The guest runs, and says what it wants


Phase 4 is genuinely done. Fault reporting, the execute-only protection fix, the time
limit, and named call traces. D065-D067; 235 tests.

**The result.** All four commercial executables execute real guest code. Three fault
deep inside their own text; the fourth does not fault at all - it spins, calling one
libkernel function **299 million times in ten seconds**, which the run now reports by
library and hash.

### Surprises

- **Execute-only text segments.** `p_flags == 0x1`, read bit clear. Nothing in the
  design anticipated it and the protection mapping tested `read` first, so guest text
  became `PAGE_NOACCESS`. Placement, 174,172 relocations and the entry-point check all
  reported success; only the page disagreed.
- **The fault reporter found it, not reasoning.** "Instruction fetch from image+0x70"
  named the operation and the address in one line. The same failure had been sitting
  there as an unqualified "access violation" through several rounds of guessing.
- **Fixing it moved the guest from byte 0x70 to 79 KB, 8 MB and 22 MB in.** One protection
  bit was the entire difference between "faults immediately" and "executes for minutes".
- **Not crashing is its own failure mode.** The 96 MB executable ran ten minutes without
  faulting, and would have run indefinitely. A hang looks like progress and produces
  nothing; the watchdog turns it into the most useful output the project has produced.
- **A `{:5.1}` format on a `String` truncates it.** Precision on a string is a maximum
  length, so `99.9` printed as `9`. It looked like arithmetic being wrong.
- **The test suite crashing its own harness read as a flake in another crate.** One
  unrelated test failed intermittently and passed in isolation every time. A different
  test binary was dying and the runner attributed the wreckage to whatever was in flight.

### Outstanding

The spinning import has no name, only `libkernel.prx::0x7dd1e10c2d2e7a04`. Naming it is
now the highest-value single piece of work in the project - which makes phase 2, the
symbol database, the thing to do next rather than a later nicety.

