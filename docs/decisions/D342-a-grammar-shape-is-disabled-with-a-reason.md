# D342 - A grammar shape is disabled with a reason

**Status:** decided
**Date:** 2026-08-27

`PatternSpec::disabled` is an `Option<String>`; its presence disables the shape and its text
states the cost and what would bring the shape back. A disabled shape is validated before it is
filtered out. The two shapes that repeat `learned` are disabled, and the names only they spell
are on the ceiling list with that reason.

**Why:** a shape using `learned` twice makes a round quadratic in that list and caps the
vocabulary far below what the unsplittable names need, while spelling only a few names. The
shape is early rather than wrong, so it stays in the file. A bare flag grows into a file of
unexplained exceptions. Validating first stops a stale part name surfacing only on re-enable.

**Rejected:**
- Deleting the shape: loses a shape that returns when the vocabulary grows.
- A boolean flag: no record of why.
