# D896 - A refused dispatch or draw can be passed over to see every wall

**Status:** decided
**Date:** 2026-10-19

`ORBISTOUN_PASS_REFUSED`, a diagnostic that intervenes, has the command processor pass over a compute
dispatch or a segment of draws that the host refuses, rather than stop its queue there. The refused
work writes nothing and the stream goes on to the packet after it. Each distinct refusal is kept with
its kind and how many times it was met, and the run report lists them all, first met first.

Unset, a refusal stops its queue as before (D705, D710), and nothing after it retires.

**Why:** A refusal stops the graphics queue, so a run shows only the first wall the host meets, and
which wall comes first changes from run to run as the title's submissions interleave. One Grand Theft
Auto V scene held forty distinct refusals: unnamed opcodes, SDWA on float instructions, layered images,
runtime-indexed descriptors, and texture formats. They were met one per five-minute run, and a run
that stopped at a different one first read as going back. Listing every refusal at once lets them be
fixed by the mechanism they share, not one per run.

**Rejected:**
- Passing over refused work by default: a frame or a result after it is not the title's, and a run
  under the default measures the emulator (D708).
- Counting refusals without passing over them: the work after the first refusal never reaches the
  host, so its refusals are never met.
