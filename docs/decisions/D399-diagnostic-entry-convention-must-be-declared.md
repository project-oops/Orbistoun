# D399 - A diagnostic that varies an entry condition must declare which one

**Status:** decided
**Date:** 2026-08-30

A tool that changes what a guest is entered with (for example, which structure
or argument convention its entry point receives) must select that convention
explicitly rather than assuming a single fixed one, and every setting that can
change a run's behavior is registered wherever this project checks whether a
run was ordinary.

**Why:** An instrument that poisons one input and assumes every other input
is some fixed shape draws a false "nothing reached" conclusion when the guest
was never given that shape to begin with. A setting missing from the
ordinary-run registry lets an intervened run silently report itself as
unremarkable, which has happened before.

**Rejected:**
- One fixed entry convention for every diagnostic: correct for one guest shape
  and silently wrong for every other.
