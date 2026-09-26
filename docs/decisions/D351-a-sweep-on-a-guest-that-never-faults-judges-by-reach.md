# D351 - A sweep on a guest that never faults judges by reach

**Status:** decided
**Date:** 2026-08-27

When the baseline run does not fault, the sweep reports `Finding::Escaped` if an intervention
makes the guest reach imports it never reached, ahead of `Unmoved`. `Derailed` requires a fault
before asking where it was.

**Why:** a spinning guest never faults, so a fault comparison answers `Unmoved` however well an
experiment worked and fires only when it breaks the guest. A guest escaping a loop calls new
imports, which reach already measures. A run without a fault has no fault address, so it cannot
have derailed.

**Rejected:**
- Declining spinning titles: return answers are exactly what such a loop waits on.
- The fault comparison alone: blind, and inverted when it fires.
