# Phase 1b - Corpus-wide survey report *(DONE)*


Survey every item in `titles/` and aggregate unresolved imports **ranked by frequency
across the whole corpus**: *implement these 20 functions and 8 more titles get
further.*

This is the highest-value thing the corpus unlocks, and it is not the running - it is
the surveying. A prioritised work list derived from evidence rather than guesswork,
strictly better than the per-title triage in BACKLOG, and it works before anything
executes.

**Observable result:** one command produces the work queue that drives every
subsequent phase.

**Done.** `./bin/orbistoun sweep` runs every module available locally and ends in the
ranked list; `orbistoun-cli worklist` produces the same totals from persisted traces
without re-running anything. It ranks by *calls*, not by count of modules - a function
called eighty-seven million times is a guest stuck in a loop, which is a wall rather than
a feature.

