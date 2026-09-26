# D490 - A shared stub table needs call labels built per module

**Status:** decided
**Date:** 2026-09-03

The label a trace attaches to a call is built from each module's own slot offset into the
shared stub table, rather than built from one module's import count with later modules'
names simply appended.

**Why:** appending a later module's by-name labels past the first module's own count only
works when a single module is loaded. With several modules sharing one table, a later
module's calls are then labelled with whichever name happens to share their slot index, and a
wrong label reads as evidence rather than as missing information.

**Rejected:**
- Building the label vector from one module's own import count and appending every other
  module's labels afterward: correct only for a single loaded module, and produces plausible
  but wrong names once more than one shares the table.
