# D047 - Files first; OTLP export is an opt-in feature, not the substrate

**decided** · 2026-08-19

The instinct to reach for OTel/Grafana is right for a different shape of problem. The
mismatch here is specific:

- **Volume.** Spans at guest-call rates would be catastrophic - the exact reason D018
  specifies a non-allocating binary sink. Guest tracing can never be OTel.
- **Infrastructure.** A desktop tool needing a running collector to be debuggable is a
  worse tool, and an agent querying a backend rather than reading a file is more
  moving parts, harder to reproduce, harder to run in CI.
- **Shape.** OTel suits long-lived services with continuous telemetry and human
  observers. This is discrete runs, seconds to minutes, consumed by a machine.

**Where it does fit:** observing the crunch harness itself over hours - run counts,
success rate over time, where the loop is spinning. That is genuinely service-shaped
with a human watching. So `tracing-opentelemetry` stays available behind a cargo
feature: a flag, not a rewrite.

**Supporting rules:**

- **Structured fields, never formatted prose** - `warn!(nid = %nid, symbol = %name,
  "unresolved import")`. The consumer reads fields.
- **Level contract.** ERROR: the run cannot continue. **WARN: something was missing
  and we carried on - the project's most important level, it is the unimplemented
  signal.** INFO: lifecycle. DEBUG: decisions. TRACE: per-call detail.
- **A shared run id** across log, trace, and report.
- **Every "missing" line carries what is needed to act on it** - NID, library, symbol,
  call site, arity - so nothing has to be correlated across files.

**Trace is ring-buffered by default.** Keep the last N million events in memory and
flush only on failure or on demand. A successful run writes no trace at all; a failure
yields the tail, which is the part that was wanted; a full trace is an explicit flag.
Solves the disk problem and improves the diagnostics at once.

**Retention:** 72 hours, purged on startup, plus a manual purge command. A size budget
acts as a second guard, since 72 hours of full traces could still fill a disk.

The crunch harness lives in **this** repo, not obSCEne - obSCEne is homebrew and has
no direct relationship to the emulator. So the run report is a first-class output of
`orbistoun-service`, and the CLI, GUI, and any future agent tooling all get it
identically.

**Consequence:** `tracing` returns to the core crates as logging is written. It was
pruned during the dependency cleanup (correctly - it was unused) and that now needs
reversing per-crate, as each gains real log calls rather than speculatively.

