# D265 - A language model proposes names, in its own binary

**Status:** decided
**Date:** 2026-08-25

A model is used only to propose vocabulary for the NID hash oracle, through the separate
`orbistoun-suggest` binary. `orbistoun-cli` does not depend on `orbistoun-llm`, and a run
report mentions the tool without invoking it.

**Why:** a model earns a place only where the candidate space cannot be enumerated and every
proposal can be checked mechanically. Naming vocabulary is the one place that satisfies both:
nouns cannot be looped over, and the hash decides each proposal for free. Keeping the model
out of the CLI keeps `run` fast and never blocks the person using it.

**Rejected:**
- Models writing implementations: no oracle checks the result.
- Models choosing stub return values: an exhaustive sweep is faster and complete.
- Models mutating or combining existing words: a loop does it exhaustively in milliseconds.
- A model inside `run`: it blocks.
