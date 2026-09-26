# D389 - The kernel log device is fed from this project's own reporting

**Status:** assumed
**Date:** 2026-08-30

A kernel log device is a bounded ring of lines drawn from this project's own
reporting layer - a call it could not serve, a name it could not resolve, a
path it does not hold - served read-only, where an empty ring means not yet
ready rather than end of file.

**Why:** Reimplementing a kernel log with invented content would be a
fabrication a guest could read as authoritative; reporting facts this project
already produces about the run gives a guest something true to read, on the
understanding that its content describes this project's own decisions, not the
platform's messages.

**Rejected:**
- Leaving the device absent: sends any guest that opens it down an error path
  over something this project can answer honestly.
- Reporting end of file when the ring is empty: a kernel log has no end while
  the kernel runs, and a guest told otherwise stops reading too soon.
