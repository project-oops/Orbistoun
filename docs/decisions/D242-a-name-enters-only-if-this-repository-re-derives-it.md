# D242 - A name enters only if this repository re-derives it

**Status:** decided
**Date:** 2026-09-26

A name is admissible only when this repository's grammar, word lists and corpus can produce it
and the NID hash agrees. An external name list may not be imported, nor used as a candidate
source. Strings harvested from a module are admissible when the vendor built the module, never
from a module another emulator project built.

**Why:** the audit's claim that every name is re-derivable from this repository alone is the
whole product. Confirmation establishes that a name is correct, not where it came from, and a
database mixing the two stops the audit measuring anything. A module carrying a mined name list
is that list arriving through a file.

**Rejected:**
- Importing hash-confirmed external lists: correct names with foreign provenance.
- Filtering an external list through the hasher: every kept name arrived from outside.
- Harvesting strings from any module: launders mined lists.
