# D752 - An unwritten compute start is the queue's zero

**Status:** decided
**Date:** 2026-10-06

A compute dispatch whose initiator does not force its start to zero, on a queue where no
submission wrote `COMPUTE_START_X/Y/Z`, starts its grid at group zero. A start the stream did
write is still read as written, and a non-zero one is still refused as not modelled. This is the
one register reset value the pipeline models. It is `assumed`, so a hardware row that contradicts
it retires it.

**Why:** the AGC library's own dispatch never writes a start. `sceAgcCbDispatch` emits
`DISPATCH_DIRECT` with initiator `0x41`, which does not set `FORCE_START_AT_000`, and nothing else
(`166-agc`, measured). obSCEne's REQ-cs02 found that the shader header's register table names no
`COMPUTE_START_*` either. PPSA03416 and PPSA02664 dispatch this way, and so does every title built
on that library. They work on hardware, so the start their queue holds is one under which the
dispatch covers its whole grid. Mesa's drivers say the same of the queue: radv's queue preamble and
radeonsi's set `COMPUTE_START_X/Y/Z` to zero once (`radv_queue.c:647`, `si_mesh_shader.c:522`),
and a dispatch writes them only when it has a base (`radv_cmd_buffer.c:14832`). The CP's shadowed
compute range begins at `COMPUTE_START_X` (`ac_shadowed_regs.c:458`), so the value is queue state
and not per-dispatch.

Hardware could not settle it. REQ-cs01 and REQ-cs02 built the dispatch twice, bare and through
oops-sdk's compute queue, and neither retired its fence.

**Rejected:**
- Refusing the dispatch until hardware answers. Two requests could not make a standalone dispatch
  retire, and the refusal holds two commercial titles at their first submission.
- Modelling `CLEAR_STATE`'s defaults as values in general (D737 keeps them unmodelled). Only this
  register has the evidence above.
