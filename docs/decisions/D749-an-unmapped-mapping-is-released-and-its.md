# D749 - An unmapped mapping is released and its base reused after a quarantine

**Status:** decided
**Date:** 2026-10-06

`munmap` of the whole of a mapping `mmap` placed releases its host reservation, and its base is
held in a quarantine: the bump allocator hands a released base out again only once 65,536 newer
releases have followed it and the slot is wide enough. Any other unmap - a piece of a mapping, or a
direct-memory view, whose memory another view may still map - keeps its reservation as before.

**Why:** an unmap released nothing, and every no-preference map took a fresh base, so a guest that
maps and unmaps as it runs grew the host's committed memory without bound. TSHP00001 maps and
unmaps 16 KiB about 22,000 times a second at its prompt: 350 MB of commit a second, and the worker
aborted with `0xc0000409` after eleven minutes at 18,000 flips; ETR00001 grows the same way at a
third of the rate. Releasing alone would not do, since a bump allocator that never reuses walks the
twelve terabytes above `MAPPING_BASE` in about an hour at that rate.

**Rejected:**
- Reusing a base at once: a stale pointer into an unmapped buffer would land in the next one and
  corrupt it silently, where it faults today. The quarantine keeps that fault for all but a pointer
  held across tens of thousands of later unmaps.
- Decommitting and keeping the reservation: returns the memory, but still walks the range.
