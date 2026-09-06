# 2026-09-04 - (/loop) The relations are testable where the numbers are not

```
6 measured relations asserted, none previously; 1 named as unreachable, not faked
suites 130   clippy/fmt/identity clean on both repos
```

Thirty-first cron tick, plan item (a): the 32 measured values that are not refusals.

## A count is not a property

`018-relational/mutex-handles-distinct` recorded `0x6`; `event-flag-handles-distinct` recorded
`0x8`. Those are counts of what the probe made, not handles - orbistoun's are host addresses,
the console's are its own kernel's - so asserting either number would pin this project to how
many objects a probe happened to allocate.

The check's **name** states something needing no number: handles are distinct, a thread's
identity is stable, a mutex one thread holds excludes another. Six are exercisable in-process
and all six hold, in `tests/measured_relations.rs`.

`mutex-excludes-another-thread` is both kinds at once: its `0x80020010` is EBUSY under the
encoding D398 measured, so that test asserts the property *and* the code, read from the
knowledge base. It is also the one that would matter most if wrong - a `trylock` that succeeded
puts two guest threads inside one critical section.

## Two things deliberately not asserted

`event-flag-handles-reusable` has two readings. That delete-then-create works is checkable; that
the **handle value** comes back is not - these are host allocations, so a test of it would pass
or fail on what the heap did that run (D535). If obSCEne's check means value-reuse, orbistoun's
behaviour is **undetermined rather than agreeing**, and that is in the test.

`file-position-tracks-reads` needs a file and nothing opens in a bare service test - `/app0`, a
host path and `/dev/stdout` all answer ENOENT, since the filesystem has no mounted title. Named
as uncovered rather than replaced with something easier.

## Breaking it taught what passing did not

D544's rule - break a multi-case guard in more than one place - paid immediately. Replacing
`sync::new_handle` with a constant failed the **condition variable** assertion and left the mutex
one green: **mutexes do not use that allocator.** They use `sync::next_handle`; breaking that
failed the mutex assertion and the exclusion test. A third break, `pthread_self` returning a
constant, failed the identity test that neither of the first two touched.

Without the second break the file would have looked verified while checking a third of what it
claimed.

## Where the axis stands

Of the 47 measured values quoted into knowledge entries: **14 refusal codes asserted** (D544),
**6 relations asserted** (here), **1 recorded as a divergence** (D543), 26 still unasserted
because their meaning is genuinely in the check's source.

Decision: [D545](../decisions/D545-the-relations-are-testable-where-the-numbers-are-not.md).
