# D084 - A tech-debt pass, and the vocabulary grows from our own observations

**decided** · 2026-08-19

Two real defects, one improvement. The tree was otherwise clean - no TODOs, no dead
code, one justified `allow` - so these are worth naming individually.

**The sweep hid failures.** Its output filter matched `reached`, `outcome`, `halted` and
diagnostic lines, but **not `failed`**. Two of six local titles are previous-generation
containers the parser refuses, and both produced *no output whatsoever* - indistinguishable
from a title that ran fine. That is precisely the confusion D010 exists to prevent,
committed in the tooling built to enforce it. The filter is the bug: an allowlist of what
to show quietly becomes a denylist of what to hide.

**The trace file name was computed in two places.** The worker wrote it; the shim
recomputed the same path-mangling to read it back for the run-to-run comparison. Had they
ever diverged the comparison would have found nothing, reported "first run of this
module" forever, and never once looked wrong. Now declared once in the crate that owns
the format, and the shim asks.

That failure mode is the argument for the rule rather than an instance of it: duplicated
logic whose divergence is *silent* is worse than duplicated logic that breaks loudly.

**The module vocabulary now comes from real import tables.** Guests publish the libraries
they need, and orbistoun's own parser reads that list - so the subsystems that exist are
an observation rather than a guess. Merging what four executables declare took the module
list from 44 entries to 75, and the names found from **264 to 352** with no code change
at all.

Worth noting what that is and is not: it is orbistoun reading a `needed` list a module
publishes about itself, which is the same act a linker performs. No database was
consulted, and every one of the 352 still re-derives from this repository alone.

