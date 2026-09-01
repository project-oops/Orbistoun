# D086 - Rank blockers by shaders blocked, within an effort tier

**decided** · 2026-08-19, revised 2026-08-20

The single judgement in `coverage.rs`, and it decides what gets worked on.

An instruction appearing ten thousand times inside one shader blocks exactly one
shader. An instruction appearing once each across four hundred shaders blocks four
hundred. Ranking by raw frequency puts the first at the top of the list and buys a
week of work for one shader.

Occurrence count is kept, as context and as a tiebreak, but it is not the ranking.

The same entry separates *undecodable* from *decodable-but-untranslatable*, because
they are different work: the first is usually a fix to the table above, the second is
a feature in a translator that does not exist yet. Collapsing them sends someone to
the wrong file.

### Revised: effort is the first key, shaders blocked the second

Confirmed with input, and extended, the first time the list had real data in it.

Ranking by shaders blocked answers *what would help most* and says nothing about what is
**reachable**. The two came apart immediately: the top entry was an export, blocking two
shaders and needing an entire render-target model, while the bottom was an ordinary
multiply-add that took twenty minutes. A single ordered list puts a week of work above a
morning's and offers no way to tell them apart - and following it would have produced
nothing that day.

So blockers now sort by **effort first**: ordinary work above anything waiting on a
subsystem, and shaders blocked within each tier. The output separates the tiers too,
because a reader scanning for *what do I do next* should not have to know that the list
silently changes meaning partway down.

**Two tiers, not a score.** Blocked-divided-by-effort would rank them precisely and the
precision would be invented - nothing here can measure effort, and a ratio built from a
guess reads like a measurement. Two tiers claim only what is known.

**The tier is not a new hand-maintained table.** It comes from the translator's existing
blocked-instruction list, which already records *why* each refusal stands and already has
a reason to stay accurate. The shader crate can see what blocks a shader and not what it
would cost; the translator knows the reason; the caller joins them. Nothing new to keep
current, and no second place for the two to disagree.

