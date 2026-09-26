# D385 - A constant harvester reports every name it declines to keep

**Status:** decided
**Date:** 2026-08-30

A harvester that extracts named constants from a header reports, by name and
by section, every candidate it declines to keep, alongside the ones it keeps.

**Why:** A spelling rule that silently narrows what counts as a constant has
taken the wrong set from a header more than once, each time discovered only
because a total count looked wrong. A harvest that names what it skipped turns
a silent narrowing decision into one that can be checked.

**Rejected:**
- Reporting only the constants successfully harvested: a header whose naming
  convention the rule does not anticipate then loses entries with no visible
  sign that anything was dropped.
