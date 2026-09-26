# D393 - Machine-identity queries are tested as one family

**Status:** decided
**Date:** 2026-08-30

Every function answering a yes/no question about which machine is presented
(devkit, development mode, faster revision, and similar) is covered by one
test asserting the whole family answers a boolean-shaped value, rather than by
a per-function test asserting a specific answer.

**Why:** A single misspelled or dropped entry in this family answers this
project's own non-zero placeholder, which a boolean caller reads as true; a
test written from the same assumption as the implementation caught nothing.
Testing the shared property catches a function dropped from, or misspelled
out of, the table regardless of which one it is.

**Rejected:**
- One test per function asserting its expected value: passes even when the
  function under test is bound to the wrong name entirely.
