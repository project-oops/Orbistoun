# D401 - Direct syscalls are tracked separately, ranked by run count

**Status:** decided
**Date:** 2026-08-30

A guest's direct kernel calls - made without going through any resolved
import - are recorded in their own field, separate from imports, and are
ranked for implementation work by how many recorded runs asked for them
rather than by call volume.

**Why:** A direct syscall has no stub index to be counted alongside an
import, and folding the two together would silently change what "distinct"
means everywhere it is reported. This project only records that a syscall
number came up, not how many times, so ranking by call volume would mean
inventing a number; ranking by how many runs asked for it is the fact
actually available, and better captures a call that blocks every guest
outright.

**Rejected:**
- Counting direct syscalls among resolved imports: changes the meaning of an
  existing count everywhere it is used.
- Ranking by call volume: this project does not have that number and would
  have to invent it.
