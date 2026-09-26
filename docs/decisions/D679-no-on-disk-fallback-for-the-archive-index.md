# D679 - No on-disk fallback for the archive index

**Status:** decided
**Date:** 2026-09-26

The archive path-resolve call resolves only the paths the title's own index names; every other
path gets the unresolved answer measured on the hardware, written at each slot's full width.
orbistoun synthesises no id and no size from files on disk.

**Why:** an id for a path outside the index is a guessed output that no implemented read path
consumes, and a size beside it asserts a resolve the hardware does not perform. The measured
answer is exact: id `0xffffffff`, size 0, status 0, return -1. A size slot is eight bytes, and a
title reads an unwritten upper half as a real length.

**Rejected:**
- An on-disk fallback answering the real file size and a synthesised stable id: a guess that
  moves no title.
- Four-byte writes into each slot: they leave the upper half of a 64-bit size slot as the
  caller's stack held it.
