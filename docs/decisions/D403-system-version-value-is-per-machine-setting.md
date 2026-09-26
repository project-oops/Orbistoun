# D403 - The system-version value a guest reads is a per-machine setting

**Status:** decided
**Date:** 2026-08-30

The system-version value a guest reads through the kernel's direct version
call is a per-machine setting, packed the way a guest compares it; zero means
unset, and an unset value refuses the call rather than answering the lowest
possible version.

**Why:** A guest tests this value against version-band boundaries and
branches on it, so answering an untested default would silently select
whichever code path serves the oldest system there is, in a way that reads as
ordinary success. Refusing an unset value applies the same rule already used
for the kernel's release string (D397), for the same reason.

**Rejected:**
- Defaulting the setting to zero and answering it: zero sits inside the
  lowest tested band, so it would not fail visibly, it would silently pick
  the oldest system's branch.
