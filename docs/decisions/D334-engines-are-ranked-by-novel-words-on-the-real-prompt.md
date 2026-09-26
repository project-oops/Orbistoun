# D334 - Engines are ranked by novel words on the real prompt

**Status:** decided
**Date:** 2026-09-26

`Llm::benchmark` asks every configured engine the caller's own request, at the caller's
temperature, scores each reply by words the vocabulary does not already hold using the
caller's own reply parser, and orders the ladder by that score. The in-process processor engine
stays in the ladder whatever it scores.

**Why:** a proposal already in the vocabulary is refused before it costs anything, so novelty
is what the loop values. Model time is negligible beside the sweep that follows each round. A
benchmark stricter than its consumer measures the benchmark. The processor engine is the only
one that answers with no accelerator, assistant, server or key.

**Rejected:**
- Ranking by speed: promotes the engine that says least.
- Ranking by word count: every engine returns the count asked for.
- A fixed local-first order: its only consumer sends no guest material.
- A strict JSON-only scorer: scores a working engine zero.
