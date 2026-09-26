# D557 - A prop is decided by provenance

**Status:** assumed
**Date:** 2026-09-04

A run is propped up when any symbol it decided for, an answer or a region, rests on a fact graded
`guest-observed` or `assumed`; facts graded published, differential or measured count as knowledge.
Provenance is kept beside the answers in `StubPolicy::known`, and a name with no provenance reads
as assumed.

**Why:** the grade already exists on every learned fact, and discarding it makes a measured answer
indistinguishable from a typed one. A guest proceeding on an answer shows consistency, not
correctness, so guest-observed stays on the prop side. Defaulting missing provenance to assumed
means drift between the two maps can only make a run look less honest, never more.

**Rejected:**
- Any override at all props a run: a hardware-measured answer could never reach the honest slot.
- Counting answers only: a region writes guest memory and would read as an unassisted run.
- Provenance inside each answer: puts a field in the guest's path that only a report reads.
- Guest-observed as knowledge: rewards trying answers until the guest proceeds.
