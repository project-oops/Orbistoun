# The worklist ranks by reachability first


Review queue, item one. D086 promoted from `assumed` to `decided`, and revised: blockers
sort by **effort tier** before shaders blocked, and the output separates the tiers.

The evidence arrived the same day it was needed. The old ranking's top entry was `exp` -
two shaders, and an entire render-target model - above an ordinary multiply-add worth
twenty minutes. Following the list would have produced nothing.

The tier comes from the translator's blocked-instruction table, which already records why
each refusal stands. No new table, and no second place for the two to disagree.

613 workspace tests green.

