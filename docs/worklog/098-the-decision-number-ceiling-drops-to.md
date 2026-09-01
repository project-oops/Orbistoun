# The decision-number ceiling drops to thirteen


Flagged "growing debt" in a reassessment and was wrong about the important half. The
duplicate decision numbers are already **capped**: `./orbistoun.sh check` fails on any new
one, and fails again if a listed number stops duplicating, so
`docs/decision-number-backlog.txt` is a ratchet rather than a backlog. It cannot grow, and
the stated reason for not batch-fixing is sound - each one means renumbering an entry *and*
its citations, so "take one when you are already in that area".

One of the fourteen was mine. I wrote `## D157` and later `## D157 (resolved)`, using the
same number for a superseding entry because it read as a continuation. It reads as a
duplicate, and someone else had to absorb it into the ceiling.

Taken, being about as in-that-area as it gets. The resolving entry is D178; all three
citations referred to the original, which keeps D157. Ceiling: 14 -> 13.

Two of those citations were also **stale** - the knowledge file and a code comment both
still said the direct-memory mapping was parked and off, which stopped being true when it
was turned on by default. Fixed while in there, which is the same principle: a citation is
only worth having if what it points at is still true.

The lesson for me: a "(resolved)" entry under the same number is not a superseding pattern,
it is a collision. Supersede with a new number that names what it resolves.

