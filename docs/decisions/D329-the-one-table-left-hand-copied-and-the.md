# D329 - The one table left hand-copied, and the rule that seemed to forbid generating it


**decided** · 2026-08-27 · found because a generated number moved and a typed one did not

`docs/PROJECT_STATUS.md` carried a per-title table - reach, imports, calls, standing, where
each ends - typed by hand. It said PPSA02664 reached 23 imports and ended at `image+0xafc959`.
The records in `compat/` said 25 and `image+0xafcc08`, and had done since a run earlier the
same day. **Four days of drift in the one table a reader looks at first.**

D240 is why it was typed: a generated block may hold only numbers the tool can recompute
*anywhere*, because the corpus is not tracked and a block claiming "6 of 6 titles execute
guest code" would fail for every contributor who owns no titles. That reasoning is about
needing a **run**.

`compat/` is committed. Reading it recomputes from what the repository ships, works in CI, and
works for somebody with no titles at all - so D240 permits this, and the table is generated
now. Worth writing down precisely because it *looks* like a contradiction: the rule is not
"nothing about titles", it is "nothing that needs one".

**The honest slot only.** `[experiment]` is a real result about a different question, and one
table holding both would be the propped-up number wearing the honest one's clothes. Titles
with an experiment are named underneath, saying what they reached and how many functions were
answered by name (D312).

### The small thing that would have made it read as a regression

The first generated table printed `98957030` where the typed one had `98,957,030`. Nobody
would call that a bug and everybody would notice it. A generated artefact replacing a
hand-written one has to be *better in every respect a reader can see*, or the generation gets
blamed for the loss and reverted - and the drift comes back with it.


