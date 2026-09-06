# D510 - A message that describes a gap outlives the gap

**decided** - 2026-09-03

`orbistoun-cli imports` ended its summary with:

```text
13 of them name data, not a function - a thunk is the wrong kind of answer there and
orbistoun has no other one yet
```

It has had one since **D323**, which reserves a zeroed page per data import and has
`ImportResolver` consult it before the thunk table. The worker's own relocation summary says so
in the same breath - *"{} imports name data and were given storage rather than a stub"*.

So the surface a reader checks first said the gap was open, and the surface nobody reads said it
was closed. Corrected to say what happens.

## This is the eighth stale work item this week and the pattern is now clear

`/dev/random`, the differential's "missing" cases, D488's note that the oscillation does not move
the verdict, Phase 0d, R9, and two entries I wrote into obSCEne's backlog myself. Every one was a
**description of a gap that outlived the gap**, and each cost a fresh reader - usually me - the
time to rediscover that the work was done.

They share a shape worth naming: **the text describing a problem lives somewhere other than the
code that fixes it.** A decision is written when the gap is found; the fix lands later, in
another file, under another number; nothing links the two, so the description is never revisited.

D307 is the exception that shows the mechanism working. Its "what was deliberately not done"
section carries an inline correction - *"(done in D323)"* - added when D323 landed, which is why
this took ten minutes to settle rather than an afternoon. **The decision log was right and the
program was wrong**, which is the opposite of the usual failure and only happened because
somebody went back.

## What is worth doing about it, and what is not

**Worth doing:** when a change closes a gap, grep for the gap's own words. `imports`, `no other
one yet` - the phrasing of a limitation is short and distinctive, and the search costs less than
this decision did.

**Not worth doing:** a gate. A message claiming a limitation cannot be checked against the code
that lifts it without the prose being machine-readable, and making it so would mean writing
every limitation twice - which is the failure one level up. This is a habit, not a mechanism,
and saying so is more honest than inventing a guard that would only catch the phrasings someone
thought to enumerate.

## The queue is the last place to learn the work was done

Stated separately because it is the sharper half. Six of the eight were found by *working* the
item - opening the file, reading the check, running the command - and none by reading the queue,
which said the work was outstanding in all six cases. **An entry in a work queue is a claim with
a date on it.** Verify the item still exists before working it; it is cheaper than the work.
