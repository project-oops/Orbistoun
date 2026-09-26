# D484 - One stub table spans every loaded module

**Status:** decided
**Date:** 2026-09-03

Every guest-facing stub table is a single, process-wide structure indexed by a slot range
assigned per module, rather than one table per module.

**Why:** the tables are process-global singletons that install once and refuse a second
installation. A second, per-module table would silently do nothing on its second
installation, leaving that module's calls answered from the wrong index with no indication
anything was wrong.

**Rejected:**
- One stub table per loaded module: the existing tables refuse a second installation, so a
  second module's table would install nothing and its calls would resolve against the first
  module's index space.
