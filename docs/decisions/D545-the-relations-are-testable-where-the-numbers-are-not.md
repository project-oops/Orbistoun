# D545 - The relations are testable where the numbers are not

**decided** - 2026-09-04

D544 asserted the fourteen measured values whose check id names a **refusal**, and left
thirty-two whose value's meaning lives in obSCEne's C source. The obvious reading of that is
"unassertable, move on". It is wrong for a subset, and the distinction is worth stating.

## A count is not a property

`018-relational/mutex-handles-distinct` recorded `0x6`. `event-flag-handles-distinct` recorded
`0x8`. Those are counts of what the probe made, not handles - orbistoun's handles are host
addresses in the `0x222b…` range and the console's are its own kernel's. Asserting either number
would pin this project to how many objects a probe happened to allocate.

But the check's **name** states something unambiguous that needs no number at all: the handles
are distinct. A thread's identity is stable. A mutex one thread holds excludes another. Those
are properties orbistoun either has or does not, and six of them are exercisable in-process.

```text
mutex handles distinct              two live mutexes differ
condvar handles distinct            two live condition variables differ
event-flag handles distinct         two live flags differ
event-flag handles reusable         delete then create succeeds  (see the caveat)
thread identity stable              same thread twice agrees; another thread differs
mutex excludes another thread       trylock answers 0x80020010, the measured code
```

All six hold. `tests/measured_relations.rs`.

## The one with a readable number, and the one with none

`mutex-excludes-another-thread` is both: its measurement `0x80020010` is EBUSY under the vendor
encoding D398 measured, so that test asserts the property *and* the code, read from the
knowledge base rather than written into the file. It is also the one that would matter most if
it were wrong - a `trylock` that succeeded puts two guest threads inside one critical section.

`event-flag-handles-reusable` has two readings and the weaker one is taken deliberately. That
deleting and re-creating works is checkable. That the **handle value** comes back is not: these
are host allocations, and whether one address is handed out twice is the allocator's business,
so a test of it would pass or fail on what the heap did that run - the nondeterminism D535 was
written about. **If obSCEne's check means value-reuse, orbistoun's behaviour is undetermined
rather than agreeing.** Written into the test rather than resolved by guessing.

## One is not reachable and is not faked

`018-relational/file-position-tracks-reads` needs a file, and nothing opens in a bare service
test - `/app0`, a host path and `/dev/stdout` all answer ENOENT, because the filesystem has no
mounted title. Named as uncovered rather than replaced with something easier that would read as
though it were the same check.

## Breaking it taught something the passing run did not

D544's rule - where one guard covers many cases, break it more than once - earned its place
immediately. Replacing `sync::new_handle` with a constant failed the **condition variable**
assertion and left the mutex one green: mutexes do not use that allocator. They use
`sync::next_handle`, and breaking *that* failed the mutex assertion and the exclusion test.

So the three subsystems in one test are three allocators, and a single break demonstrates one of
them. Without the second break the file would have looked verified and been checking a third of
what it claimed. A third break - `pthread_self` returning a constant - fails the identity test,
which neither of the first two touched.

## What this axis has now covered

Of the 47 measured values quoted into knowledge entries: **14 refusal codes asserted** (D544),
**6 relations asserted** (here), **1 recorded as a divergence** (`sceKernelDlsym`, D543), and 26
still unasserted - values whose meaning genuinely is in the check's source. That is a reasonable
place for the number to stop, and the remainder should stay unasserted until a reason arrives to
read one, rather than being guessed at because the list looks incomplete.
