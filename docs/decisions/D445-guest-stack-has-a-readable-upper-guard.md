# D445 - The guest stack carries a mapped, readable guard above its top

**Status:** decided
**Date:** 2026-09-01

A guest stack reserves one extra readable page above the initial stack pointer, in addition to
the unmapped guard below it.

**Why:** The argument block orbistoun builds at the top of a fresh guest stack sits close enough
to the reserved top that a guest reading a fixed number of words past its start can run past the
mapped region; on real hardware the stack extends further and the same read lands on mapped
memory. A guard that is mapped and reads as zero, rather than unmapped, lets a modest over-read
behave the way it does on the platform instead of faulting for an unrelated reason.

**Rejected:** leaving the region above the argument block unmapped, matching the stack's lower
guard - an over-read there faults with no relation to its actual cause.
