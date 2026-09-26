# D129 - Two progress signals

**Status:** decided
**Date:** 2026-09-26

A run is compared with the previous run of the same title on two signals - distinct imports
reached and the fault position - and the verdict names which moved: `FURTHER`, `same`, `BACK`,
or `MIXED` with the reason. The shader corpus reports movement in the same vocabulary.

**Why:** a fault position compares two points on one path and says nothing across two paths, so
alone it calls real progress backwards. Distinct imports survives a change of path. Unattended
work steers by this verdict, and a misleading one is worse than none.

**Rejected:**
- Fault position alone: wrong the moment the path changes.
- Import count alone: inflatable, and blind to progress within a path.
- A separate vocabulary for shaders: two words for one loop.
