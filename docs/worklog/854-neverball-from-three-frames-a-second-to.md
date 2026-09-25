# 854. Neverball from three frames a second to ten

**2026-09-24**. The translated shaders shrank, and the host's per-draw work mostly went away.
(First filed under 848, a number another session had already spent. Moved here, and the code
comments that cited it moved with it.)

**GPU (D716), busy ~820 to ~100 ms/s:**

- A fragment module simulates one lane. Neverball's pixel shaders went from 16-58k SPIR-V
  instructions to 0.4-1.8k.
- Each stage is translated at the width the stream declares (`GS_W32_EN`, `PS_W32_EN`). The GL
  context's primitive shader is wave32 and was translated as 64 lanes.
- A known exec mask emits only its lanes. The GL primitive shader runs 1 lane, then 3, not 32.

**Host prepare, ~41 to ~13 ms per 12k-draw submission:**

- `RegisterSweep` is a flat table read a register at a time. It was a `BTreeMap` gathered, sorted
  and rescanned for ~70 registers per draw.
- A draw whose shader registers hold the previous draw's values reuses its shaders.
- Packet bodies are read in place, not copied into a `Vec` per packet, in both the pipeline and the
  command processor. `draw_calls` runs once per submission, not twice.

**Measured** (`ORBISTOUN_LIMIT=40 ./bin/orbistoun run NVRB00001`): 8-10 flips/s, from 3-4. Frames
render correctly: textured planet, sky, blended level.

**What is left, per frame (~100 ms):**

| Where | Time |
|---|---|
| Guest's own paint (GL command building) | ~30 ms |
| Prepare | ~17 ms |
| Command-processor memory work | ~8 ms |
| Draw execution | ~14 ms |
| Target read, write-back and present | ~15 ms |

- The command processor's work is real: each frame fills and copies ~38 MB (colour and depth
  clears, and a target-sized copy).
- Temporary per-phase timers found all of this and are removed. The perf overlay's phases remain.
