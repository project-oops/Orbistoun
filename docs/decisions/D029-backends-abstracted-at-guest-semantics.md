# D029 - Backends are abstracted at guest semantics

**Status:** decided
**Date:** 2026-08-19

A backend contract describes what the guest asks for, not what one host API provides: render
commands, samples at a rate, pad state, guest path semantics. Any string crossing a boundary is
a named constant declared once.

**Why:** a contract designed from one host API carries that API's model, and a second backend
then fits badly. A seam is structural when it pays for itself in testability; the render contract lets
command-stream translation be tested with no device through a recording backend.

**Rejected:**
- A contract shaped by the first host API: a second backend fits badly.
- Seams that pay off only hypothetically: speculation.
