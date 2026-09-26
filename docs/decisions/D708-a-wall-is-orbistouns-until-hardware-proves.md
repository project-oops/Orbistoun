# D708 - A wall is orbistoun's until hardware proves otherwise

**Status:** assumed
**Date:** 2026-09-19

Every guest fault is attributed to orbistoun by default; blaming the title's own code needs a
hardware observation of the same path failing the same way. The compatibility record carries a
`[hardware]` attestation of what the hardware does with a title, and the run report frames each
fault by it.

**Why:** orbistoun is the incomplete party, and the title shipped and ran. A guest log marker,
an import orbistoun cannot resolve, or a fault downstream of a stub is not evidence against the
title. Blaming the title removes the wall from scope and ends the work, the tooling equivalent
of a stub that returns success, so the ground truth is shown at the fault.

**Rejected:**
- Treating a guest TODO print or an unresolved import as proof of a broken title: a hypothesis
  presented as a finding.
- Attributing each fault on its merits with no default: the cheapest conclusion wins.
