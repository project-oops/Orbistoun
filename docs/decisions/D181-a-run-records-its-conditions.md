# D181 - A run records its conditions

**Status:** decided
**Date:** 2026-08-21

Each trace carries `Conditions` - the limits, the default stub answer, the override count, the
diagnostics applied and the build - and a comparison reports what changed underneath a verdict
rather than refusing to render one. The build is recorded but not compared. Every run also
reports how many calls reached an implementation.

**Why:** a verdict is evidence only if nothing else changed. `default_return = "ok"` improves
every number while implementing nothing, and an unattended loop finds it within a few
iterations; labelling it is what separates a technique from a reward hack. The build changes on
every release and would drown the differences that matter.

**Rejected:**
- Forbidding blanket success: a legitimate bisection technique.
- Refusing to compare across conditions: hides real numbers.
