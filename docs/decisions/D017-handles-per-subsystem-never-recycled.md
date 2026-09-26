# D017 - Handles are per subsystem and never recycled

**Status:** decided
**Date:** 2026-08-19

Each subsystem issues its own handles, and an issued handle is never reused.

**Why:** a handle from one subsystem passed to another is caught rather than silently
plausible. Reuse makes a stale-handle bug look like a valid access to the wrong object, which
is far harder to diagnose than exhaustion.

**Rejected:**
- One shared handle space: an audio handle passed to a file call looks valid.
- Recycling freed handles: stale use becomes wrong-object use.
