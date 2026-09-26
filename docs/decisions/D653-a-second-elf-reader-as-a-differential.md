# D653 - A second ELF reader as a differential

**Status:** decided
**Date:** 2026-09-09

`selfish-elf` is a dev-dependency used only by a differential test that runs both readers over the
installed corpus and reports every field on which they disagree. Orbistoun's own reader remains the
one that loads guests.

**Why:** a parser has no oracle, and hostile bytes produce a plausible answer whatever it does. Two
readers built from the same knowledge and drifted independently catch defects neither project sees
alone. The test asserts nothing about which side is right, and skips loudly when the corpus is
absent so a silent pass cannot read as agreement.

**Rejected:**
- Migrating the loader to `selfish-elf`: replaces the reader that runs guests rather than checking it.
- No cross-check: leaves only fixtures somebody already thought of.
