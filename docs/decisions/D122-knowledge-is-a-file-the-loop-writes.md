# D122 - Knowledge is a file the loop writes

**Status:** decided
**Date:** 2026-08-20

What is known about a guest function - arity, argument meanings, return kind, edge cases - lives
in the knowledge TOML under `orbistoun-hle`, written by `orbistoun-cli learn` and merged rather
than replaced. It is never stored in the generated symbol database, which a search overwrites. An
unknown arity is absent, not zero, and a test holds that `guest_module!` declarations never
contradict it.

**Why:** a name can be regenerated; a measured fact costs an experiment and cannot. Two stores
with different lifetimes share a key without duplicating content. Zero is a real arity a trace
renders differently from "unknown".

**Rejected:**
- Facts in the generated database: destroyed on the next search.
- Facts in decision prose: recoverable only by grepping.
- Arity compiled into declarations alone: answering it needs a rebuild.
