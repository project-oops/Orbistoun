# D179 - Runs produce ranked findings

**Status:** decided
**Date:** 2026-09-26

A run produces findings, each with a machine-routable kind, a subject, evidence from the trace,
a suggested action and a confidence, ranked by confidence before weight. A fault is a finding,
classified by arithmetic on its address. A finding is `Certain` only when the trace shows it.
The report describes a fault and stops; diagnosis is the reader's.

**Why:** the consumer is a program, not a person, and it needs what is wrong, where, on what
evidence and what would address it. A confidently wrong suggestion gets acted on, so a heavy
guess must never outrank a light certainty.

**Rejected:**
- Prose diagnostics: need a person who knows each shape.
- Ranking by weight alone: guesses outrank certainties.
- Reports that narrate a cause: a story presented as a measurement.
