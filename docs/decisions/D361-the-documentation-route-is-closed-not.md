# D361 - The documentation route is closed, not untried


**decided** · 2026-08-29

[PAYLOADS.md](../PAYLOADS.md) listed three routes to the handoff structure, and put reading the
SDK's own documentation first: cheapest, and squarely permitted - principle 1 allows another
project's prose and forbids its source.

**There is no such prose.** Neither the SDK's README nor the loader's documents the calling
convention, what the loader passes, or any argument structure. Both describe how to build
and how to invoke, and point at source for the rest.

So the route is **closed rather than untried**, which is a different fact and the one worth
recording: the next person should not spend the afternoon that suggested itself here.

Two routes remain, both more expensive and both already written down: grind the marker sweep
(`[entry] argument = "sentinels"` names a field per boot), or build a payload with the open
toolchain and know the contract because you wrote the thing receiving it.

### One thing was seen that will not be used

A search surfaced a summary of the argument structure belonging to **a different SDK**
(`PS5Dev/PS5SDK`). The payloads here are built with the dynamic-linking one, which is why
they carry real import tables - so it is not the ABI in question and nothing here derives
from it. It is in `ACKNOWLEDGEMENTS.md` because an uncredited sighting is exactly what makes
a provenance question unanswerable later.

Worth naming the temptation, because it was real: that SDK's first field is a function
pointer, and the measurement here says the entry point calls its first field immediately
(D308). **Agreement between a measurement and something adjacent is not evidence**, and
taking it as such is the convergence problem arriving by the route principle 1 warns is
hardest to see - a fact that was recalled, then dressed as reasoning.

