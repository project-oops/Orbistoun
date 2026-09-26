# D159 - Stack alignment is measured, never forced

**Status:** decided
**Date:** 2026-08-20

The trampoline records the stack pointer as the guest's call left it, and every run reports how
many calls arrived on a conforming stack, including when all did. It never realigns the stack.

**Why:** a misaligned stack does nothing until some callee spills a vector register, then faults
far from the cause. Measuring names the violator; forcing alignment makes the crash vanish while
the guest keeps running misaligned internally. A line that appears only on failure cannot be told
from a line nobody wired up.

**Rejected:**
- `and rsp, -16` in the trampoline: hides the fault permanently.
- Reporting only violations: silence is ambiguous.
