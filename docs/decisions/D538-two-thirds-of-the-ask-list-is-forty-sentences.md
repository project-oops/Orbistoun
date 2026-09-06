# D538 - Two thirds of the ask list is forty sentences

**decided** - 2026-09-04

`orbistoun-cli questions` prints one line per function per open question, so the list is as long
as the number of entries resting on something unverified. It reports **719 open questions**, and
obSCEne's backlog 022 is generated from it.

Grouping them by the sentence they are written in:

```text
719 open questions
275 premises
 40 premises carry 484 of the 719          (the other 235 rest on one entry each)
  6 premises carry 355                     (49% of the list)
```

The heaviest is one sentence under fourteen functions across two libraries and **872,904 calls**:
*"Modelled on the POSIX call of the same shape. The correspondence is inferred from the name and
the guest's usage, never verified against the target library."*

## Why the shape matters more than the count

Fourteen identical asks are not fourteen pieces of work. A premise shared across a family is
answerable by **sampling** it - two or three functions chosen for how cheap they are to call -
and the result speaks to the shared claim. Read one line at a time, that structure is invisible,
and a console operator either enumerates the whole group or skips it.

Being exact about what a sample buys, because the temptation is to overclaim: measuring
`sceKernelWrite` settles `sceKernelWrite`. For the other thirteen it moves the *premise's*
credibility, which is precisely what "never verified against the target library" is about - and
a single counter-example refutes the group outright, which is the cheaper outcome to go looking
for.

## Deduplication, not classification

`orbistoun-turn` refuses to decide what a question means from its prose: a rule over words fails
silently and reads exactly like a question nobody can act on (D356). That still holds, and this
does not weaken it, because **nothing here reads a question**. Two group only when they are the
same sequence of words - case, spacing and punctuation forgiven, and nothing else.

So the grouping is deliberately unable to do the useful-looking thing. Two entries asking a
closely related question in different words stay apart, because the differing word may be where
the difference is. The 275 is therefore a **floor**: the number of distinct things unknown here
is no larger, and may well be smaller.

## The defect it found on the way in

Fourteen entries meant one sentence; ten ended the first clause with a full stop and four with a
semicolon. Under word-for-word grouping that is one premise with two wordings - but under any
exact-text grouping, including the census that started this, it is two. My own count said ten
functions and 872,301 calls; it was fourteen and 872,904.

The fix is the data, not a fuzzier match, so the four now read like the ten - and
`no_premise_in_the_knowledge_base_is_written_two_ways` gates it. That test went red on the data
that prompted it, which is the only evidence that a guard works.

## The second defect, which the mode wrote itself

`--top` was already applied to the queue, and the grouping was added after it. So the first
version of this mode grouped a *shortened* list: `--premises --top 1` reported that **one**
function rested on the heaviest premise, with 806,669 calls behind it, under a header reading
`3 premises behind 3 open questions`. The truth is fourteen functions and 872,904 calls.

Nothing about that output looks wrong. Same words, same confidence, a number that is a fact
about `--top` presented as a fact about the knowledge base - which is exactly what principle 3
forbids, in a tool rather than in a stub.

Grouping now happens before truncation, and `--top` shortens the list of premises rather than
the premises. `crates/orbistoun-cli/tests/premises.rs` holds two guards: the heaviest premise
must be identical with and without `--top`, and the header must not move when the window onto
it does. Both were made to fail by reinstating the ordering, and both fired.

The guards assert against the tool's own full-run answer rather than against a count written
into them, because a count would drift with the knowledge base and start passing for the wrong
reason.

## What this does not do

Move the wall, or establish anything about the platform. It is a claim about the ask list being
**countable**: the same failure D537 found one level down, where records said nothing while the
code held the reasoning. Here the records say the right thing 719 times and the list never
admits that it is saying it 275.
