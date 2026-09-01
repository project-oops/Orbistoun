# D332 - A record with no honest baseline, and the parse test that failed for it


**decided** · 2026-08-27 · a consequence of the slot routing, surfaced by an unrelated test

The corpus sweep recorded the obSCEne probe module for the first time. The policy carried one
measured override, so its result was routed to `[experiment]` (D312) - and the file therefore
has **no `[status]` at all**. Nobody has ever measured that module unassisted.

That is correct and it is worth being able to say. What broke was a test:

```
assertion `left == right` failed: every .toml carries a readable status
  left: 6, right: 7
```

`every_record_in_the_tree_parses` compared the number of `.toml` files with the number of
records carrying `[status]`. The two were the same number for as long as every first run was
an honest one, so the count worked as a proxy for "parsed" - and stopped the moment a first
run was not. **A test about parsing failed for a reason that had nothing to do with parsing.**

It counts slots now: every file parses, and every file carries at least one measurement.
A file with neither is still a failure, because that is a record nobody measured.

**The general shape, which this log has hit twice today already.** A proxy that has agreed
with the thing it stands for since the day it was written is indistinguishable from a real
check, right up until the first case where they differ - and then it fails somewhere else,
naming something that is not the cause. The count was never checking parsing; it was checking
a coincidence.

**And the underlying fact deserves surfacing rather than only accommodating.** A module whose
only record is an experiment has no unassisted number, and the generated title table quietly
omits it - correctly, since it belongs in neither the honest table nor a comparison with it.
Whether that absence should be *reported* rather than merely handled is not settled here.

