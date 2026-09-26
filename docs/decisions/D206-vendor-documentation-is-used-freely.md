# D206 - Vendor documentation is used freely

**Status:** decided
**Date:** 2026-09-26

Public GPU vendor documentation and vendor-contributed open source are used freely. The reference
assembler and disassembler may check a table and cross-check a fact; they are never what a table
is generated from, and a wrong row is corrected from the published document. A hand-written
fixture is weaker evidence than a compiled one and is marked so.

**Why:** a silicon interface documented for anyone to program against is published for this.
Generating a table from the reference implementation's own tables collapses two sources into
one, and the differential test then confirms only that it agrees with itself.

**Rejected:**
- Treating vendor documentation like another implementation's source: refuses what is published to be used.
- Generating tables from the assembler's target description files: the check becomes circular.
