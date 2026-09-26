# D340 - Both pad spellings are declared with one implementation

**Status:** decided
**Date:** 2026-08-27

The pad library declares both the base and extended spelling of each pair, and one
implementation serves both halves of a pair.

**Why:** a real module imports both halves of every pair, so the library exports both. Deciding
which one a title really uses is a guess with no upside. Absence from the symbol database means
only that nobody here has named a symbol yet; the import table of a real module is the evidence.

**Rejected:**
- Declaring only the extended spelling: imports of the base names land on stubs.
- Separate implementations per spelling: two copies of one behaviour to drift.
