# D538 - Premises group by exact wording

**Status:** decided
**Date:** 2026-09-04

`questions --premises` groups open questions only when they are the same sequence of words, with
case, spacing and punctuation forgiven, and groups before `--top` truncates. A premise shared by
many entries is written one way in the data, and a test gates that.

**Why:** a premise shared across a family is answerable by sampling a few of its functions, which
a one-line-per-function list hides. Nothing reads what a question means, so a rule over words
cannot fail silently. Grouping after truncation reports a fact about `--top` as a fact about the
knowledge base.

**Rejected:**
- Grouping by similarity or classifying prose: fails silently where the differing word is the difference.
- Truncating before grouping: undercounts the heaviest premise with no visible sign.
