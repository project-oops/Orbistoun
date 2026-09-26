# D394 - The presented machine is one setting

**Status:** decided
**Date:** 2026-08-30

Which machine this project presents - generation, kind, and revision - is one
setting, and every function answering a question about machine identity
derives its answer from that one setting rather than returning its own
independent constant.

**Why:** The identity questions are not independent: exactly one kind can be
true, and separately hardcoded answers can silently disagree with each other
in a way nothing detects. A single setting removes the possibility, and
travels with the run rather than with the machine the work happens on,
because a guest's behavior on a given presented machine is a fact about that
pairing.

**Rejected:**
- Separate hardcoded answers per function: cheap individually, and nothing
  stops them contradicting each other.
