# D545 - Measured relations are asserted as properties

**Status:** decided
**Date:** 2026-09-04

Where a conformance check's name states a relation (handles distinct, identity stable, a held
mutex excludes another thread, a file position advances), orbistoun asserts that property and not
the number the capture recorded. A code the check measured is asserted only when it is itself the
property, and it is read from the knowledge base at run time.

**Why:** the recorded number is often a count of what the probe allocated, or a handle value from
the console's own kernel, and pinning it would tie orbistoun to one probe run. The relation is
something orbistoun either has or lacks. Reading expected codes from the knowledge base keeps a
re-absorbed capture authoritative without editing tests.

**Rejected:**
- Asserting recorded values directly: pins orbistoun to how many objects a probe happened to create.
- Leaving every value whose meaning is in the check's source unasserted: loses properties that need no number.
- Copying expected codes into tests: drifts from the capture and passes for the wrong reason.
