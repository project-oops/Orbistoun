# D354 - A revision the file states is a claim; a revision the checkout states is a fact


**decided** · 2026-08-27

D353 made the constants harvest a command. This makes it **checkable**, which is the part
that matters: the ABI constants are the only table here that is neither derived by
experiment nor written by a person - they are copied out of somebody else's headers, so
*where did this number come from* is the only question about them.

`orbistoun.sh` now regenerates the table and diffs it. Three ways a wrong number gets in,
and the guard was pointed at each before it was believed:

| tampering | first version | now |
|---|---|---|
| hand-edit a value | **caught** | caught |
| delete a constant | **caught** | caught |
| edit the header to name a different revision | **passed** | caught |

### The hole was in taking the revision as an argument

`--revision` was stamped into the header, so the header was a *claim* - and the gate
re-derived the file **using that claim**. Regenerating with whatever the file said produced
a file saying the same thing, so `ee81cd1d` edited to `deadbeef` matched itself and passed.

For a table whose entire purpose is provenance, that is the one failure that matters: it
means the file could name any source at all and nothing would notice.

The generator asks the checkout instead - `git -C <source> rev-parse HEAD` - so the header
states what the harvest **actually read**. A hand-edited header now differs from a
regeneration, which is what the gate is already looking at. `--revision` is gone rather than
made optional: an override would restore exactly the hole it just closed.

A checkout that is not a git repository is **refused**, not stamped "unknown". A table that
cannot say where it came from is the thing this exists to prevent.

### Why it was found

By pointing the guard at three things and watching what it did, rather than by watching it
pass. Two of the three were caught and read as confirmation; the third was the one worth
running. *A guard nobody has watched reject something is a guard nobody knows anything
about* - and two-thirds of a guard looks exactly like a whole one.

### Where the checkout is

`ORBISTOUN_FREEBSD_SRC`, defaulting to a sibling of the parent directory. Absent, the step
**warns and passes** rather than failing: the checkout lives outside this repository and not
every machine has one, and a gate that cannot run on a fresh clone is a gate people learn to
skip. It says so out loud, which is the difference between unverified and verified.



