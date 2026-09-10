# D608 - A guard reading a shape that no longer exists

**Status:** measured
**Date:** 2026-09-08

## What it was saying

`./bin/orbistoun check` has failed its `decisions` step for some time, printing:

```text
D054 D055 D056 D084 D085 D122 D124 D125 D126 D127 D128 D129 D130
[fail] these are listed in docs/decision-number-backlog.txt
       and no longer duplicate - remove them
```

Every one of those numbers **does** still duplicate. Two files carry `D054`, two carry `D055`,
and so on down the list. The guard was reporting the exact opposite of the truth, and the
instruction it printed - delete these from the backlog - would have emptied a ceiling file whose
whole rule is that it may only shrink for real, and hidden thirteen live duplicates behind a
green step.

## Why

```bash
duplicates=$(grep -oE '^## D[0-9]+' docs/DECISIONS.md | ...)
```

`^## D123` was the shape of the log when every entry lived in one file. The entries moved into
`docs/decisions/` and their headings went with them; `DECISIONS.md` became an index of table
rows. The pattern matched nothing, `duplicates` was empty, and "listed but not in the empty set"
is every entry in the file.

Nothing about the failure said *"I found no decision numbers at all"*, because nothing asked. A
guard that reads a field which has ceased to exist does not go quiet - it answers from an empty
set, confidently, and the answer is well-formed.

This is the failure principle 3 already lists one level down: **a guard is not finished until
somebody has made it fail.** Somebody had made this one fail - it had been failing for days - and
the failure was itself the bug, which is the case that list did not cover. A red step nobody has
read the *reasoning* of is worth no more than a green one nobody has tested.

## The change

The pattern reads the index table, which is where decision numbers live now. And it refuses to
proceed on an empty read:

```bash
if [ -z "$duplicates" ] && [ "$(grep -cE '^\| [^|]* \| D[0-9]+ ' docs/DECISIONS.md)" -eq 0 ]; then
    bad "no decision numbers found in docs/DECISIONS.md - this guard is reading the wrong shape again"
fi
```

That second clause is the part worth keeping. It cannot catch every future reshaping, but it
turns *this* class of failure - "the file moved and the pattern silently stopped matching" - from
a wrong answer into a named one.

## Made to fail, in both directions

- Remove `D054` from the backlog: the guard reports it as newly duplicated. Correct.
- Add a `D999` that duplicates nothing: the guard reports it as stale. Correct.
- Restore: green.

Both branches, because a guard that only ever fires one way is half-tested and the untested half
is the one that was wrong here.

## What is not fixed

The thirteen duplicate numbers themselves. They predate the numbering guard and each is cited
from source by number, so clearing one means renumbering an entry *and* its citations - which is
what the backlog file already says, and why it exists. The ceiling is honest again; emptying it
is separate work.
