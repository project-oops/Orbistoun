# D184 - Instrumentation is tested by golden output

**Status:** decided
**Date:** 2026-08-21

The ranking and rendering of results is tested by a committed golden frontier generated from
`compat/`, which is tracked and holds no guest material. Its diff is the artefact and is expected
to change. Ties break on the title, so the order is total.

**Why:** a unit test written by whoever chose an ordering asserts that ordering and cannot see
it is wrong; rendering real data shows it. obSCEne tests the emulation, and nothing guest-side
can see the code that ranks and renders what the emulation observed.

**Rejected:**
- More unit tests on ordering: faithful to the mistake.
- A snapshot over the titles library: untracked and machine-specific.
