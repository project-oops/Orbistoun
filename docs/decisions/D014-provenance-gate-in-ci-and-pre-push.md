# D014 - Provenance gate in CI and the pre-push hook

**Status:** decided
**Date:** 2026-09-26

No firmware, key, decrypted title, disassembly or code written while reading a vendor binary
enters the repository. `bin/orbistoun provenance` enforces it in CI and in the pre-push hook,
on every push, over every file `git add -A` would pick up.

**Why:** reimplementation from disassembly converges on the original's structure, and that
convergence is evidence. The cost of contamination is that the work can never be shared. The
gate is the control; `.gitignore` is a convenience that a force-add bypasses.

**Rejected:**
- Relying on `.gitignore`: a single `git add -f` defeats it.
- Checking only the index: untracked files about to be added go unexamined.
- Skipping the hook for documentation-only pushes: the check costs seconds.
