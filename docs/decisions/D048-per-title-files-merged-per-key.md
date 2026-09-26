# D048 - Per-title files, merged per key

**Status:** decided
**Date:** 2026-09-26

Title-specific behaviour lives in one file per title, identified by a hash of its executable,
never in code. Layers merge per key, user over repository over global. A compatibility key
names a deviation, not a title, prefers a typed value to a boolean, and carries a reason and a
label: `quirk`, `workaround` or `unsupported`. Every applied override is reported in the run.

**Why:** a key named for the behaviour lets a second title reuse it with no code change.
Whole-file replacement silently drops the repository's entries under a user's. The labels keep
a workaround visibly temporary instead of turning into a permanent, respectable name.

**Rejected:**
- `if title == ...` in the core: title knowledge in code.
- Hashing the title directory: tens of gigabytes per run.
- Whole-file override: user settings erase shipped compatibility entries.
- Negative booleans such as `disable_x`: double negatives once forced on.
