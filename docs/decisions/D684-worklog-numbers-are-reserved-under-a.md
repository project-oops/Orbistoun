# D684 - Worklog numbers are reserved under a lock, like decision numbers

**Status:** decided
**Date:** 2026-09-14

## The choice

`./bin/orbistoun worklog "<title>"` claims the next worklog number under a `mkdir` lock and writes a
`**RESERVED**` stub, exactly as `decide` has done for decision numbers since D316. A new gate,
`worklogs`, refuses duplicate numbers and stale reservations, and runs as part of `check`.

## Why it matters

D316 closed this race for decisions and left it open for worklogs, and the reasoning it gave applies
unchanged: the window between choosing a number and writing the file is a race, and the number is
*cited* - "see worklog 534" - so a duplicate makes every citation ambiguous.

**It was not theoretical.** On 2026-09-14 two sessions working the same afternoon both created
`docs/worklog/535-*.md`, twenty-five minutes apart. Nothing refused the second. It surfaced only
because somebody happened to list the directory afterwards, and the fix was to renumber the later
one by hand and repair its index row. Worklog entries take longer to write than decisions do, so the
window here is *wider* than the one D316 judged worth closing.

## Why coverage is a warning and not a failure

The gate also notices a worklog file with no row in `docs/WORKLOG.md`, and reports it with `warn`
rather than `bad`. That is deliberate and is the one non-obvious part.

An unindexed entry is invisible to anyone reading the index, so it is a real defect. But the index
row is added when an entry is *finished*, which means a file another session is writing right now is
**expected** to have no row. Failing on it would red the tree for somebody else's work in progress -
the exact problem `--only` exists to avoid, stated in this script's own scope comment. So it is
reported where a person will see it and not where it can block them. The very first run demonstrated
the case: it named another session's half-written 535.

Files beginning with `_` are skipped entirely. `_preamble.md` is structure, not an entry; it carries
no number and belongs in no row, and the first draft flagged it.

## Two details worth keeping

**`bad` is called as `bad "..." || failed=1`, not `bad "..."` followed by `return 1`.** Under
`set -e`, a bare `bad` aborts the function immediately when the gate is run standalone - and every
line of detail after it is lost. The first version did exactly that: it printed "these worklog
numbers are used twice" and then nothing, so the gate named a problem and withheld the evidence. The
`||` form both suppresses the abort and records the failure, and as a side effect the gate can report
duplicates *and* stale reservations in one run rather than stopping at the first.

`decisions` does not have this bug - it prints its detail *before* calling `bad` - which is why it
took writing a second gate to notice the shape was fragile.

**The index row is not written by the reservation.** A `RESERVED` stub listed in `docs/WORKLOG.md`
would advertise work that does not exist yet.

## Not needed next door

obSCEne appends to a single `docs/WORKLOG.md` rather than numbering per-file entries, so it has no
equivalent race and needs no equivalent verb.
