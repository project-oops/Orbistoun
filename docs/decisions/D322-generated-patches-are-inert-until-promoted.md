# D322 - Generated patches are inert until promoted

**Status:** decided
**Date:** 2026-09-26

A generated change is a patch file applied by nothing until a person promotes it. It carries
its oracle and `proposed_by`, and `submit check` lists patches apart from measurements as
unchecked. A knowledge patch generated from a measurement holds only fields the measurement
established, plus the library it belongs to.

**Why:** a proposal that a person reads, gates and merges has a verification step; the real
risk in generated code is provenance, a recalled behaviour presented as reasoning. Listing
patches separately stops a diff inheriting the trust measurements earn. A person's patch and a
model's need different reading. Filling purpose, arity or `found_by` from a measurement that
never established them is exactly the recall the provenance rules refuse.

**Rejected:**
- Refusing to generate implementations: a labelling requirement suffices.
- One list of claims: a patch borrows the measurements' standing.
- Completing an entry from the function's name: invents fields.
