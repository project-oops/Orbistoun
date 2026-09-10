# D618 - Evidence going missing made a claim stronger

**Status:** measured
**Date:** 2026-09-08

## The limit D617 documented, and what it actually costs

D617 noted, as an aside, that the report directory is overwritten: the probe writes six files into
one place and an hour later they are six *different* files. It called that a limit of the
arrangement.

It is worse than a limit. `orbistoun-gen measurements` rebuilt the table from whatever was on
disk, so the previous batch's observations did not merely become unreadable - they were **deleted
from the committed table by a successful run**. `20260908-094705` appears in the table zero times
today, and every number D609 drew from it is unreproducible.

The dangerous half is the direction it moves a claim. `constant` means *every run that took this
measurement agreed*, and it is the flag that decides whether anything may assert a value. A
measurement marked non-constant **because two batches disagreed** becomes constant again the
moment one batch is deleted - and the coverage gate then demands it be claimed or declared, and a
claim is what it will get.

Evidence disappearing should never make a conclusion safer. Here it did, silently, in a run that
reported success.

## Measured, by taking the fix out

Regenerating with only `../obscene/data/hardware` present - which is what an hourly overwrite
looks like from the tool's side:

| | measurements in the table |
|---|--:|
| before | 429 |
| regenerated without the fold | **38** |
| regenerated with it | 429 |

A 91% loss, no error, exit zero.

## The change

The committed table is read back into observations and folded in beside whatever the captures
say. `observations_in_table` parses `disagreed` from the shape `table` renders it in - `value in
a.txt and b.txt` - so a disagreement survives a regeneration that cannot see either file.

This is the rule the other two generated files have followed for months: `write_symbol_db`
accumulates names and `write_wanted` accumulates hashes, both citing D074, both for the same
reason - each run sees only part of the corpus, and writing only what it saw discards the rest.
The measurement table was the one that did not.

Idempotent: two regenerations in a row produce the identical file.

## What cannot be recovered

The `20260908-094705` batch. It was already gone when this was found, so the table holds the
`143022` observations and the archive's, and not the ones in between. D609's conclusions stand in
the decision log; the evidence behind the numbers it quotes does not.

That is the cost of finding this one batch late rather than a reason to be quiet about it.

## The guard, made to fail

`a_committed_table_is_carried_through_a_regeneration_that_cannot_see_its_captures` round-trips a
two-row table with one disagreement and asserts the disagreement survives. Dropping the
`disagreed` parse makes it fail on exactly that assertion, which is the one that matters: losing a
row is visible, and losing a row's *contradiction* is not.
