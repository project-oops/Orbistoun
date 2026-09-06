# D547 - The relation needed a mount, not a guest

**decided** - 2026-09-04

Two plan items, and they came out opposite ways.

## The encoder group checks out, and that is the whole finding

Six `106-encoder/related-libs` entries say the console reports an encoder library *absent* -
"a fact about which tier a title may reach", with the completion condition "model the tiers the
manifest records". The neighbouring `110-modules/tier-probe` entry says "orbistoun models no
tiers".

Nothing models them. Every occurrence of "tier" in the source is the **provenance** tier the
audit sorts symbol derivations into - a different word for a different thing. The closest thing
to a module tier is `FIRMWARE_MODULE_DIRECTORIES` in `orbistoun-kernel`, whose comments name the
three directories and what each holds, and it refuses all three identically: a `/system` path is
ENOENT whichever tier it belongs to.

So seven reasons are accurate as written. That is the result, and it is worth one paragraph
rather than a manufactured finding - four ticks of re-deriving stated blockers has produced two
that were wrong and thirteen that were right, which is roughly the ratio a person should expect
before deciding the exercise is finished.

## The one that was wrong was mine, from two ticks ago

D545 recorded `018-relational/file-position-tracks-reads` as unreachable: nothing opens in a bare
service test, because `/app0`, a host path and `/dev/stdout` all answer ENOENT with no mounted
title. Every clause of that is true.

The conclusion drawn from it was not. **The relation needed a mount, not a running guest.**
`orbistoun_fs::mount::mount` is public, `descriptor.rs`'s own tests already stand a title up with
it in four lines, and `orbistoun-service` already depends on the crate.

What I had actually established was "the filesystem answers nothing when nothing is mounted",
which is a fact about the default state, and I read it as "a service test cannot mount anything".
The check I skipped is the cheapest one in the loop notes: **ask whether the tool has a seam
before concluding the tool cannot be used.** It cost two ticks of the item sitting in a plan as a
standing question.

## What the test asserts

Two four-byte reads of a sixteen-byte file return `0123` then `4567`, and a third after seeking
back to zero returns `0123` again. **The bytes, not the counts** - a descriptor that re-read the
start every time returns the right length twice and the wrong content, which is exactly the
failure this relation is about. The seek is what makes it a relation rather than two calls: it
shows the position is something that can be moved, not a counter that goes up.

The console's `0x20` is not compared to anything. It is very likely a byte count, given a
sixteen-byte file read twice, but that is a reading rather than a citation - the D545 rule, that
the check's *name* is what can be asserted.

Broken twice, in the two mechanisms it spans: a `read` that seeks to zero first fails the
advancing half, and a `seek` that reports success without moving fails the seeking half.

## Where the relations stand

All seven measured relations are now asserted. The one message that said otherwise has been
rewritten rather than left - a sentence describing a gap outlives the gap unless somebody greps
for its own words when the gap closes (check 13), and this is the second time that habit has
caught my own prose in a week.
