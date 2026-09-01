# Phase 0e - Observability substrate *(DONE)*


The plumbing for D046 and D047, before there is much to report - so that every later
phase adds to it naturally instead of retrofitting.

- **Run identity.** One id shared by the log, the trace, and the report.
- **Developer log** to hardware plus a rolling file, structured fields rather than
  formatted prose, with the level contract from D047 documented and honoured.
- **Run report**: versioned schema, emitter, and the diff-against-previous-run
  machinery. Thin at first - it has little to describe until phase 1 - but the shape
  is what later phases fill in.
- **Retention**: 72-hour purge on startup, a manual purge command, and a size budget.

`tracing` returns to each core crate here as that crate gains real log calls, not
speculatively - it was pruned during the dependency cleanup because it was genuinely
unused.

**Observable result:** every run produces a correlated log, an optional trace, and a
bounded machine-readable report that says what it was given and what it did.

