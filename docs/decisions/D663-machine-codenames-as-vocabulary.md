# D663 - Machine codenames as vocabulary

**Status:** decided
**Date:** 2026-09-26

Orbistoun names machines by codename: `orbis` and `prospero` are the base machines, `neo` and
`trinity` their mid-generation refreshes. `Platform` is derived from `Generation` and `Revision`,
and an artifact for any previous-generation machine is `orbis` unless it is genuinely specific to
the refresh.

**Why:** codenames are not trademarks, so they keep vendor marks out of orbistoun's own output while
matching how the rest of the collection names the same machines. Deriving the platform from the
two existing axes means a machine cannot claim two contradictory identities.

**Rejected:**
- Product names in output: puts a trademark on every report line.
- `Platform` as a third stored field: allows a machine to be a base model and a refresh at once.
- `neo` for any previous-generation artifact: names a refresh for a build that is not one.
