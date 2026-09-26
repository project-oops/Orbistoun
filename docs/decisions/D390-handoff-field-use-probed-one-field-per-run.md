# D390 - An undocumented handoff field's use is probed one field per run

**Status:** assumed
**Date:** 2026-08-30

A field in a structure a guest's runtime is handed, whose use nothing here
documents, is probed by poisoning one field per run with an address nothing
maps and observing whether and how the guest faults on it.

**Why:** Poisoning several fields at once only answers whether any of them is
touched, and the first fault taken hides every field after it; one field per
run separates "used" from "never reached" cleanly, and needs no symbols or
documentation to work - which matters because a commercial guest carries
neither.

**Rejected:**
- Poisoning several fields together: the first fault taken masks every other
  field's answer.
