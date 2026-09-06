# 2026-09-04 - (/loop) The ask list was asking for a function that does not exist

```
713 questions / 135 premises   ->   745 / 140      (up, and correct - see below)
"POSIX analogue of the same name"  33fn (9 of them false)  ->  32fn, all real
"Which errno values the target returns"  4fn -> 28fn
```

Twenty-sixth cron tick, fourth pass over the ask list.

## The negative first, because it was the plan's item (b)

Does the D539 pattern - one question with a symbol inside it, hiding as many - recur among the
108 singleton premises? **No.** Two filters, both stated: replacing every backtick-quoted span
with a placeholder groups **zero** additional premises; pairwise similarity across all 135
sentences finds five pairs above 0.72, none covering more than a couple of entries. D539 was the
only instance and it is closed.

(Similarity used to *find candidates for a person to judge* - not to group. D538 refuses grouping
by similarity and still does.)

## What the similarity pass did surface

Four wordings of one claim, and one of them false for a third of its members.

*"Semantics follow the POSIX analogue of the same name"* covered 33 entries. Twenty-four are
`scePthread*`/`posix_pthread_*`, where it holds. The other nine are the event-flag and semaphore
families - **POSIX has no `CreateEventFlag`, and `WaitSema` is not `sem_wait`**. The sentence read
exactly like the twenty-four beside it where the claim is true.

## What it was covering

`sceKernelWaitSema(semaphore, need, timeout)` had **one** open question and it was that sentence.
`sync::semaphore_wait(handle, until)` takes no count and no deadline: **two of three arguments are
ignored**, and nothing recorded it. Same in `PollEventFlag` (only bit 0 of the mode word is
modelled) and `CreateEventFlag` (attribute word and fifth argument are not).

Replaced with one true shared sentence plus, per entry, the split this audit keeps finding - what
orbistoun does into `edge_cases`, what the platform does into `assumptions`. `WaitSema` went from
one false question to one fact and three answerable questions.

The reading also turned up an inconsistency: a bad handle answers the **measured** ESRCH
(`0x80020003`) for event flags and this project's **placeholder** (`0x7fff_0003`) for semaphores.
obSCEne measured only event flags, so that is now a recorded question rather than an unnoticed
difference.

## The guard

`a_claimed_posix_namesake_is_one_the_harvest_lists` spells a vendor name into its C-library form
(`scePthreadCondWait` -> `pthread_cond_wait`) and requires it in `orbistoun-names`' harvest of
FreeBSD exports. A transformation of the **name**, not a judgement about behaviour. All 32 current
claimants pass; all nine removed ones fail. Made to fail by putting the claim back on two entries.

## The count went up

745 questions, from 713. That is the correct direction: a false sentence was hiding real unknowns.
D539 said this axis is not about shrinking the list; this is the tick that proves it, because the
honest move made the number worse.

Two merges besides: the same-name premise is one across 32 (was two wordings), and the errno
question is one across 28 (was 4), after the placeholder clause was split out of the 24 entries
that had glued it onto their question.

Decision: [D540](../decisions/D540-the-ask-list-was-asking-for-a-function-that-does-not-exist.md).
