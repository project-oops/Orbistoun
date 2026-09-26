# D716 - A fragment module simulates one lane

**Status:** decided
**Date:** 2026-09-24

A fragment-stage module simulates one lane. At every stage, a lane that a constant
execution-mask write is known to exclude emits nothing, and a lane known set writes without the
select; that knowledge resets at each block. Each stage runs at the wave width its command
stream declares.

**Why:** a fragment invocation is one pixel whose coverage the host rasteriser decides, and no
translated instruction reads another lane, so lane zero's result is unchanged. Skipping
known-inactive lanes is the hardware's own behaviour. The instruction stream cannot say which
width it was compiled for, and translating a 32-lane wave as 64 simulates lanes that do not
exist.

**Rejected:**
- Simulating every lane: most of the work is for lanes that write nothing.
- The subgroup model everywhere: exact only where the host subgroup matches the wave, and much
  larger; it stays for primitive shaders with unknown masks.
