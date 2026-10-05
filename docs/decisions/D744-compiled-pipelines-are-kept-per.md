# D744 - Compiled pipelines are kept per title

**Status:** decided
**Date:** 2026-10-05

The host's compiled pipelines are kept per title in the title library, beside its translations
(D113), as one Vulkan pipeline cache: loaded when the device opens, used for every pipeline the
backend creates, and written back after each one it had to compile. Data whose header names
another vendor, device or pipeline-cache UUID is not handed to the driver; the cache starts
empty instead.

**Why:** a translated module simulates every lane of a wave, so one is megabytes of SPIR-V and
the driver takes seconds to compile it. CRFT00001 spent 68 to 128 s of a 120 s run compiling 29
pipelines whose modules were byte-identical to the previous run's, while the guest waited on its
fence and the run read as stalled. The modules are already the same across runs (D113); this
keeps what the driver made of them too.

**Rejected:**
- Relying on the driver's own shader cache: measured not to hit for these modules.
- Writing the cache only when the run ends: a run stopped by its time limit never ends cleanly,
  and the compiles it paid for would be lost.
