# D703 - The guest-memory window is frame state

**Status:** decided
**Date:** 2026-09-26

The frontend reads the pipeline window's exact span from guest memory, and the driver sets it on
the backend once per frame through `set_guest_memory`, before the commands. A span that is not
wholly mapped reads as empty.

**Why:** every module in a submission shares one window compiled into it, and the guest never
numbered it, so a guest-numbered buffer bind would misrepresent it. The span must equal the mask
the module applies, or an index folds to a different word. An empty window is what an unplaced
window holds; zero-filling or a partial read would invent memory.

**Rejected:**
- A `BindBuffer` command: its slot is the guest's numbering, and this buffer is the
  translation's.
- A partial or zero-filled window: contents the guest never wrote.
