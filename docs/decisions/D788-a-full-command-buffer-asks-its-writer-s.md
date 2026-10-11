# D788 - A full command buffer asks its writer's overflow callback

**Status:** decided
**Date:** 2026-10-11

When a packet does not fit between a command-buffer writer's cursor (`+0x10`) and its limit
(`+0x18`), orbistoun's builders call the function at the writer's `+0x20` with the writer, the
packet's length in dwords and the quadword at `+0x28`, as `(rdi, esi, rdx)`. If it answers non-zero
in `al`, the builder reads the cursor and limit again and writes the packet there; if it answers
zero, or still leaves too little room, the builder refuses as before (D670).

The contract is read from the guest, not measured on hardware (`known_by = guest-observed`).
PPSA25872 builds into a writer whose cursor and limit are both zero, with a callback at `+0x20`
and its argument at `+0x28`. The callback forwards `(rdi, rsi, rdx)` to a routine that treats `rdi`
as the writer, sizes the chunk it hands back from `esi * 4 + 0x70` bytes, reads its own state from
`rdx`, installs the chunk in the writer, and returns a bool in `al`. That fixes the argument order
and the meaning of the answer; the dword count asked for is the packet's own.

**Why:** refusing an empty writer lost every packet PPSA25872 built before its first chunk: a
compute shader's indirect register load, its user data and three dispatches, so its first
submission dispatched with no program (`COMPUTE_PGM_LO/HI were never written`).

**Rejected:**
- Refusing as before and waiting on a hardware request: the guest's own callback states the
  contract, and the refusal is what blocks the title.
- Answering a writer with no buffer from orbistoun's own memory: the title tracks its chunks
  through the callback's state, so a chunk it did not hand out would never be submitted.
