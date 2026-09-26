# D288 - Environment variables are named from one registry

**Status:** decided
**Date:** 2026-08-26

Every crate names an environment variable through `orbistoun_env::<VAR>.name`. A trial clears
every variable of `Kind::Diagnostic` before it runs and leaves settings alone.

**Why:** a second hand-written list drifts silently; a new diagnostic missing from it leaks
into every trial of a sweep that claims to be controlled. Settings such as the data directory
are how a trial is pointed at its own trace directory, so clearing them would send runs at the
machine's real one. Literals remain only where `option_env!` needs one or a test asserts the
spelling on purpose.

**Rejected:**
- A per-crate list of diagnostics to clear: already missed one variable when it was replaced.
- Clearing every registered variable: discards the settings a trial depends on.
