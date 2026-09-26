# D221 - Environment variables are declared in one crate

**Status:** decided
**Date:** 2026-09-26

`orbistoun-env` declares every environment variable with a summary, an example, its reader, and
whether it is a setting or a diagnostic; `orbistoun-cli env` prints the registry and an
undeclared `ORBISTOUN_` variable is reported. Diagnostics - fills, plants, pokes, dumps, forced
returns - are environment variables for one run, share one parser and one conditions entry, and
never come from a file.

**Why:** without a list a misspelled variable runs an ordinary experiment and reports an ordinary
result. A diagnostic is a question asked once; one left in a file becomes an undocumented
workaround. Shared plumbing keeps one matching rule, including matching an unnamed import by
hash.

**Rejected:**
- Variables read ad hoc per crate: no list, typos pass silently.
- Diagnostics as persistent settings: experiments outlive their question.
- A crate named for configuration: `config.toml` belongs to the service near the top of the spine.
