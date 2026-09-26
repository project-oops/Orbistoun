# D291 - A sweep finding becomes a record with its assumptions

**Status:** decided
**Date:** 2026-08-26

Promotion is a pure function in the turn dispatcher from a finding to a knowledge `Record`:
what the sweep established goes under `guest-observed` and edge cases, and everything it did
not establish goes under `assumes`, with the title it was seen in.

**Why:** a measurement that exists only as terminal output is lost. The judgement is which
column each claim belongs in; recording an unmeasured meaning as known would be a recalled
fact dressed as a measured one. A patch may then use only what the entry records as measured,
so a diff against the entry is a provenance audit.

**Rejected:**
- A second record format for measured findings: duplicates the knowledge vocabulary.
- Recording the function's apparent purpose: nothing measured it.
