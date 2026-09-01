# D019 - Greenfield: no legacy, no compatibility shims

**decided** · 2026-08-19

Nothing has shipped. Edit the original, change the format, wipe the file. No
migrations, no deprecated aliases. Applied already: aspirational dependencies were
pruned rather than kept "for later", which left `orbistoun-core` with zero
dependencies - the right shape for the bottom of the graph.

