# D725 - Instructions the host lacks are rewritten at link

**Status:** assumed
**Date:** 2026-09-26

When the host CPU lacks an instruction set extension the guest CPU has, the link plan
(D724) replaces each such instruction with an equivalent sequence the host executes. SSE4a
`extrq` and `insertq` are the case in force. A host that has the extension gets no rewrite.
The plan lists every rewrite by address, and the run report says a rewritten title ran.

**Why:** an instruction the host lacks faults with an illegal-instruction exception on first
execution, so the title stops at the host's gap rather than at orbistoun's. A rewrite at
link costs nothing per execution and is visible in the plan. This is instruction set
translation, not interception: calls still reach orbistoun only through linkage slots
(D005).

**Rejected:**
- Emulating the instruction in the illegal-instruction handler: a fault per execution, and
  these instructions sit in inner loops.
- Refusing to run on such a host: the gap is the host's, and the fix is mechanical.
