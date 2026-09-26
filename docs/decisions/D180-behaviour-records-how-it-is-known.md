# D180 - Behaviour records how it is known

**Status:** decided
**Date:** 2026-08-21

Every behavioural fact in the knowledge file carries `known_by` - `published`, `measured`,
`guest-observed` or `assumed` - and anything it does not cover is listed in `assumptions`.
`published` and `measured` must cite. `learn` refuses an entry that does not account for itself.

**Why:** facts can arrive through a model that has read other projects, with no reading step to
point at, so abstinence is unenforceable and accounting is not. Every value is falsifiable, and
none means "already known". Per-claim assumptions stop a mixed entry being rounded up into a
fact.

**Rejected:**
- A value for "by analogy": becomes the comfortable default.
- One provenance per function: rounds a mixed entry up.
- A default when the source is missing: every default is a lie.
