# 2026-09-03 - (/loop) The decision log had the answer

```
tests   1984  ->  1984   (a constant moved and an instrument added)
```

D487 left the run nondeterministic and the cause unknown. This finds the larger half of it, and
the finding is that **it was introduced by worklog 329 and D443 had already written down why.**

## The hunt, including what it ruled out

The two states differ by exactly one allocation round - fourteen rounds, or thirteen and an
`mprotect`. Working back from there:

- **Not thread scheduling.** The leading candidate, and wrong: the guest never calls
  `scePthreadCreate`. It is single-threaded when it dies. That hypothesis was in the loop prompt
  as "leading candidate, NOT a finding", and it was right to say so.
- **Not the reservation failures.** Thirteen fail on every run, in both states.
- **Not host address-space luck.** Same count, same first and last base, every run.

Each was killed with a measurement rather than an argument, which is the only reason the fourth
one was reached.

## The instrument that made it findable

The report showed only the **last** reservation failure. So thirteen failures looked exactly
like one, and *"did this run fail more than that one"* was not a question it could answer -
which is precisely the question when two runs of the same binary diverge.

`reserve_failures()` and `first_reserve_failure_base()` now answer it. First run with them:

```text
orbistoun: 13 reservation(s) failed, first at 0x6b0000000000, last: base=0x6b0000000000 …
```

Thirteen, all at one base, with **different lengths** - a caller repeating, not a walk running
out of room. That is what turned guessing into a mechanism.

## And then the actual cause

`TITLE_MODULE_BASE`, where D482 places the modules a title ships, was `0x5000_0000_0000`.

**D443 records that address as PPSA02664's own heap arena.** Its C++ allocator reserves there;
a policy region placed on it once already cost a null-pointer fault, through
`tlsf_add_pool` rejecting a pool whose arena-relative arithmetic had underflowed. That decision
moved the *policy* arena off `0x50…`. Worklog 329 then put the *modules* there, because the
address looked empty.

Moved to `0x4800_0000_0000`. Eight runs after: **69 distinct imports every time.** The
oscillation that swung the verdict is gone.

## Which corrects yesterday's claim

Worklog 337 reported **+18 calls** from linking the title's modules, off a properly interleaved
A/B with three samples a side. The measurement was sound; the attribution was not. With the
collision removed the linked path answers 10884-10887 - the *unlinked* path's own range.

**The +18 was the collision, not the linking.** Linking the modules has no measured effect on
how far this title gets. That is the honest result and it is not the one that was written down
four hours ago.

## Two things I got wrong on the way, and one of them was nearly shipped

**The causation was backwards.** Seeing the guest hint `0x6b0000000000` for all thirteen of its
range reservations, and `POLICY_REGION_BASE` being that address, read as "orbistoun sat on the
guest's arena". It is the other way round: a policy region plants its base into a guest
argument, and the guest's allocator then reserves *at* what it was handed. Confirmed by moving
the constant twice and watching every failure follow it.

**A documented decision was nearly undone.** Acting on that misreading, `POLICY_REGION_BASE` was
moved off `0x6B…` - which is the address D443 deliberately chose, for reasons written in a
comment directly above the constant that was being edited. Reverted. The comment now says the
thirteen failures are expected and why, so the next reader does not repeat the afternoon.

The rule that has been in the loop prompt for nine ticks is *check any note against the thing it
describes*. This is the inverse failure: **read the note that already exists before recording a
discovery.** The decision log answered both questions and was consulted only after the fact.

## What is still not true

A **±3 call oscillation remains** across those eight runs. It does not move the verdict, which
keys on distinct imports, but the run still does not reproduce and the cause is unknown. D487
stands for that remainder.

The guest crashes where it did, at `0x7fff0001`, which the reporter flags as orbistoun's own
code rather than the guest's.

## State

`cargo test --workspace` green - **117 suites, 1984 tests**, 0 failures. clippy `--tests`
clean, fmt clean, identity scan clean.

Nothing committed. The day holds worklogs 292-338 and D466-D488.

**Next**: the fault at `0x7fff0001` - the reporter already names it as orbistoun's own and
prints the host stack that reached it, which is more than most bugs here start with.
