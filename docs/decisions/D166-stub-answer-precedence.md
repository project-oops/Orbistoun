# D166 - Stub answer precedence

**Status:** decided
**Date:** 2026-09-26

A call's answer comes from, in order: a forced return set for the run as a diagnostic; a stub
policy override keyed by name or by hash; the knowledge file's declared return kind; the policy
default. The policy reaches every import, declared or not. A forced answer on an implemented
function runs the implementation and replaces only its answer.

**Why:** an explicit override is a deliberate question and wins. A declared pointer, handle or
count must answer zero regardless, so a blanket default cannot reintroduce wild pointers (D125).
Undeclared imports are the ones most worth asking about, and unnamed ones can be addressed only
by hash. Skipping an implementation would change its side effects as well as its answer.

**Rejected:**
- Policy applied to declared symbols only: the commonest case exempt from the experiment.
- Default before declared return kind: "answer ok" reintroduces wild pointers.
- Skipping the implementation under a forced answer: two changes read as one.
