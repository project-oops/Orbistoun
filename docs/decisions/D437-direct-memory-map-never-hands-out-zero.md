# D437 - The direct-memory map never hands a guest physical address zero

**Status:** decided
**Date:** 2026-09-01

The direct-memory map's default reserves a floor above physical address zero before handing out
the first byte of the range.

**Why:** A conformance probe run against real hardware shows the platform reserves the start of
its direct range and never answers a first allocation at zero; a guest that treats a returned
zero base as a sentinel needs this to hold.

**Rejected:** a floor invented to be "large enough" - the measured platform floor is the only
value with a source.
