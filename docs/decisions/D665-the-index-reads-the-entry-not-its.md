# D665 - The index reads the entry, not its title

**Status:** measured
**Date:** 2026-09-10

## Two instructions that destroyed work

An audit from the umbrella thread found `CLAUDE.md` telling every session to add a numbered entry
to `DECISIONS.md` and append to `WORKLOG.md`. Both are **generated** - their own headers say so -
so an entry typed into either is lost on the next build, and `check-decisions.sh` reds the gate on
a `## Dnnn` heading in the index.

I had been doing exactly that all session: nineteen decisions and nineteen worklog rows, appended
by hand with `sed`. The files under `decisions/` and `worklog/` were safe; the index rows were
one build away from gone.

`CLAUDE.md` now points at `./bin/orbistoun decide "<title>"`, which reserves the next number
atomically so two sessions cannot collide, and says plainly that neither index is to be edited.
The "Needs review index" it named has not existed for some time - the status column *is* that
index, rendering `assumed` as 🟡.

**This is the finding that compounds**, because every session bootstraps from that file. Every
one of them would have made the same mistake.

## And then the generator disagreed with me five times

Running it revealed the second half. The status column was read from `head -6` of each entry -
which includes the **title** on line 1 - and `grep -m1` takes the first match. So any decision
whose title contains a vocabulary word was indexed by its title:

```text
D659  "Two walls that hardware cannot reach"     -> hardware
D649  "A name that cannot be confirmed"          -> confirmed
```

Both carry a real status two lines below. `published` and `guest-observed` were missing from the
vocabulary entirely, so six orbistoun entries rendered as `unrecorded` - the project's own
provenance grades, unknown to the tool that indexes them.

Fixed in `tools/split-decisions.sh`: read lines 2-6, and add the two grades mapped green, since
`assumed` is the only grade this project flags for review.

## A shared tool, and I ran it where I should not have

`tools/split-decisions.sh` serves all four repositories. To measure the blast radius I ran it
against the other three - **which writes**, and is not a dry run however I described it to
myself. Three sibling working trees gained a modified `DECISIONS.md` while their own agents were
working in them.

Reverted immediately, and the correct move recorded here: the measurement went into a request in
each of their inboxes instead, with the two rows that would change and which of them look like
corrections rather than regressions. obscene's `D014` says `assumed` about itself and its index
says `measured`; that is theirs to confirm, not mine to assert by writing to their repository.

The rule this leaves: **a tool that writes has no dry-run mode unless somebody wrote one.**
Reading a shared script's behaviour off its name is the same class of error as reading a status
off a title.
