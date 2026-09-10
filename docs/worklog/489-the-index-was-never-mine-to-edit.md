# 489. The index was never mine to edit

**2026-09-10** - directed, continuing 488

An audit arrived in the inbox from the umbrella thread. Two findings were urgent and both were
real.

## The instruction that destroyed work

`CLAUDE.md` told every session to add a numbered entry to `DECISIONS.md` and append to
`WORKLOG.md`. Both are generated - their own headers say so - so entries typed into them are lost
on the next build, and the gate reds on a `## Dnnn` heading in the index.

**I had done that nineteen times this session.** The files under `decisions/` and `worklog/` were
safe; the index rows were one build from gone. `CLAUDE.md` now points at
`./bin/orbistoun decide "<title>"` - which reserves the number atomically - and says neither index
is to be hand-edited. The "Needs review index" it named has not existed for a while: the status
column is that index (D665).

## The gate was already red

`orbistoun-cli status --check` was failing: the generated titles block had drifted from `compat/`
- PPSA25872 recorded 151/321973 while the block said 141/310987, and PPSA99980 had no row at all.
`status --write` and `compat markdown` fixed it. Worth noting the gate had been red for a while
and `check` reported only `prose`, so the red was there to be seen and was not seen.

## Then the generator disagreed with me five times

Running it properly revealed a second defect, in the shared tool. Status was read from `head -6`,
which includes the title, and `grep -m1` takes the first match - so a title containing a
vocabulary word beat the entry's own status line:

```text
D659  "Two walls that hardware cannot reach"   -> hardware
D649  "A name that cannot be confirmed"        -> confirmed
```

`published` and `guest-observed` were missing from the vocabulary too, so six entries rendered
`unrecorded`. Fixed: read lines 2-6, add the two grades.

## Surprises

- **"Dry run" was a word I made up.** To size the blast radius I ran the generator against the
  other three repositories. `--index` writes. Three sibling trees gained a modified
  `DECISIONS.md` while their own agents were working in them. Reverted immediately; the
  measurement went into their inboxes as a request instead, which is what should have happened
  first.
- **Two of the three changes look like corrections, and that does not matter.** obscene's `D014`
  says `assumed` about itself while its index says `measured`. Theirs to confirm - a correct
  change to somebody else's repository is still a change to somebody else's repository.
- **The audit was right about the compounding one.** Every session bootstraps from `CLAUDE.md`,
  so every session would have made the same mistake, and the evidence is that this one did.

## Next

- The remaining audit findings, ranked C through E: provenance number drift, a `generation =
  "ps5"` that the code can no longer parse, and a front-page call count that contradicts the
  records five ways.
- The corpus fetch path, still using `repo`/`tag`/`path` rather than the origin list.
