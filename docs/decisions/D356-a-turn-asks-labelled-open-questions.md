# D356 - A turn asks labelled open questions

**Status:** decided
**Date:** 2026-08-27

A turn reads the knowledge base's open questions, ranked by call volume, and runs the highest
one that names its experiment in the `answerable_by` field. Unlabelled questions are reported as
having no rule, and an unrecognised label is an error. A run that never faulted cannot "stop
faulting".

**Why:** the questions were already recorded and ranked, and the dispatcher read only what
crashed. A question is written for a person, so classifying it by its words is guesswork; a
field is checkable. "No rule for this" and "nothing to ask" must not look alike, and a label
naming no experiment is a claim nobody can act on.

**Rejected:**
- Matching question text: fails silently.
- Dropping unlabelled or unknown questions: hides work.
