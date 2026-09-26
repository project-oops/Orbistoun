# D330 - The learned vocabulary has a cost ceiling

**Status:** decided
**Date:** 2026-09-26

`learn_words` costs a vocabulary round before writing and refuses a set of words that would
push it past a ceiling, stating the numbers and the choice: curate the words, or drop a shape
that uses the slot twice. `is_word` rejects mangled-symbol fragments, and a refusal is
`Learned::Refused`, not `None`.

**Why:** a round re-sweeps every shape using the grown slot, and a slot appearing twice makes
the cost quadratic, so a harvest can make the search unrunnable with nothing failing. Curation
kept only in the file is undone by the tool that writes the file. A filter on word shape cannot
bound cost; the cost itself can. A silent refusal and "nothing new" look alike.

**Rejected:**
- A shape filter alone: many fragments still pass.
- One-off curation: the next harvest restores the list.
