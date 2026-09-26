# D662 - Application category as a permission

**Status:** assumed
**Date:** 2026-09-26

The `applicationCategoryType` a title declares decides what it is granted: direct memory budget,
whether it may own the video scanout, and whether it excludes other titles. An unrecognised
category is its own variant and is granted neither scanout nor direct memory.

**Why:** granting every title everything is wrong in the direction that hides defects: a guest the
console would refuse proceeds here and fails somewhere unrelated. The per-category values are read
from SELFish's packaging notes and are `published`, not measured, which is why the status is
`assumed` until a probe confirms them.

**Rejected:**
- Treating every title as the most privileged kind: silent divergence from the console.
- Defaulting an unknown category to the default grant: grants everything on an unrecognised number.
