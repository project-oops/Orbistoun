# 537. The worklog numbering race, closed

**2026-09-14** - `./bin/orbistoun worklog` reserves atomically; `worklogs` gates it

## What happened first

Two sessions working the same afternoon both created `docs/worklog/535-*.md`, twenty-five minutes
apart. Nothing refused the second. It was noticed only because a directory listing happened to run
afterwards, and the repair was manual: renumber the later entry to 536, fix its heading, fix its
index row.

Decision numbers have been safe from this since D316. Worklog numbers were still
read-the-directory-and-add-one - and worklog entries take longer to write than decisions do, so the
window was wider than the one D316 already judged worth closing.

## What landed

- **`./bin/orbistoun worklog "<title>"`** - claims the next number under a `mkdir` lock and writes a
  `**RESERVED**` stub before a word of the body exists. Deliberately the same mechanism as `decide`,
  down to the five-second wait and the `trap` that releases the lock however the call exits.
- **`./bin/orbistoun worklogs`** - refuses duplicate numbers and stale reservations, and warns about
  entries missing an index row. Wired into `check` alongside `decisions`.
- **D684** for the reasoning, including why coverage is a warning rather than a failure.

## Made to fail, three ways

A guard nobody has watched reject something is a guard nobody knows anything about, so:

| what | result |
|---|---|
| a second file numbered `534` | `[fail]`, exit 1, **both** colliding filenames named |
| a reservation left unfilled | `[fail]`, exit 1, the file named |
| three `worklog` calls raced in parallel | 537, 538, 539 - three distinct numbers |

The race test is the one that matters, since it is the thing the change exists for.

## The bug the second gate found in its own first draft

The first version printed `these worklog numbers are used twice` and then **nothing** - it named a
problem and withheld the evidence. Cause: `set -e`. `bad` returns 1, and a bare `bad` inside a
function that is not in a condition context aborts immediately, taking every line of detail after it.

It is invisible when a gate is run through `check`, because `also` calls it as `if ! "$@"` and a
condition context suppresses `set -e` - so the detail prints there and vanishes when a person runs
the gate directly to investigate. Exactly backwards from what you want.

`bad "..." || failed=1` fixes it and buys something: the gate now reports duplicates *and* stale
reservations in one run instead of stopping at the first.

`decisions` does not have this bug - it prints detail *before* `bad` - which is why the shape looked
fine until a second gate was written against it.

## Surprises

**A perl one-liner ate a shell variable and silently corrupted the script.** `$(basename "$file")` in
a perl replacement string interpolates: `$file` became empty and `$)` became the process GID, leaving
`base=197121basename "")` in the file. `bash -n` caught it immediately, which is the argument for
running a syntax check after every edit to a script rather than at the end.

**Nothing next door needs this.** obSCEne appends to one `docs/WORKLOG.md` rather than numbering
per-file entries, so it has no equivalent race.
