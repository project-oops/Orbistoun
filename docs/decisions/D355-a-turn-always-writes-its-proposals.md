# D355 - A turn always writes its proposals

**Status:** decided
**Date:** 2026-08-27

`turn` always writes what it measured to the untracked `patches/` directory; `--apply` gates
only the policy change. `record_compat` refuses a run made under any diagnostic intervention,
and each boot records itself through `run`.

**Why:** a measurement left as terminal output is lost, and a proposal nothing applies is
undone by deletion, so writing it needs no gate. Applying changes what the next run does and
needs an oracle. An intervened run reaches further by construction and is not a compatibility
claim.

**Rejected:**
- Gating proposals behind `--apply`: the ordinary turn persists nothing.
- Recording intervened runs as experiments: a diagnostic is not a policy.
