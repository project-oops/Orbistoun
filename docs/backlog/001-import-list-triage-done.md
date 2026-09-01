# Import-list triage *(done)*

Cluster a module's unresolved imports by library and by first-touch order, so the
output is a ranked work list rather than an alphabetical dump.

Done, and by a better route than the one imagined here: `orbistoun-cli worklist` ranks by
**observed call count across every run**, not by first-touch order in a static dump. What
a module might call and what it actually called turn out to be very different lists, and
only the second one says where to spend an hour.

