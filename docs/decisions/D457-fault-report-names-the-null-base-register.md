# D457 - A fault report names the register most likely holding the null base of the access

**Status:** decided
**Date:** 2026-09-01

On a fault whose address is at or near zero, the report names the specific register whose value,
plus a small field offset, produced the fault address, preferring a register that is exactly zero
over one that only matches by coincidence.

**Why:** The full register dump already exists, but which register was the null pointer was left
for a reader to work out by matching values against the fault address by hand. An exactly-zero
register is a genuine null dereference; a near-zero coincidental match is excluded so the report
does not mislead. A fault far from address zero names nothing.

**Rejected:** naming any register whose arithmetic happens to match the fault address - produces
false leads from coincidental matches.
