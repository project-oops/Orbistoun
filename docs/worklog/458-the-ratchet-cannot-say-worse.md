# 458. The ratchet cannot say worse

**2026-09-08** - directed, continuing 457

Bus idle a seventh pass. Two decisions still with the operator. So: verify the corpus, which turned
out to be the productive thing to do.

## PPSA28061 has lost 44% of its imports and nothing said so

Running the whole corpus to confirm nothing regressed found something that had:

```text
PPSA28061-app0   26 distinct, 334 calls, the guest called abort
committed record  47 imports, 933 calls, image+0x43c4, measured_on 2026-08-23
```

Stable across three runs. **Not this session**: `git stash -u`, run on a clean `HEAD`, restore -
same 26, same abort. Sixteen days old at least.

Neither instrument can express it (D634). The compat record is a **ratchet** - `beats` gates every
write, for the good reason that a looser stub policy would otherwise overwrite an honest entry
permanently - so a worse run leaves no trace. And the run verdict compares against the **previous
run**, so once a regression is the new normal every run afterwards says `(+0)` and `same`, forever.

Between them they answer *"did this beat the last run"* and *"what is the best ever seen"*, and
neither answers *"is this build below its own record"*.

The frontier's own test uses `status(Reach::Entered, 47, 933)` as its example of an honest record.
That is this title's number, and the build that produced it is gone.

## Three walls, one subsystem

Consolidated separately (D633), because three findings from three directions turned out to be one
decision rather than three: a title's own mapped code answered with a placeholder (D630), a symbol
that should have been bound left null (D628), and - established this pass - a symbol that should be
null, bound.

That last is `900-surface/control`: obSCEne checks its own instruments with a weak symbol it
defines and one it deliberately never defines. The console reports the second absent. **Orbistoun
reports both present**, so the probe declares its own census meaningless - which is why ninety-four
census entries carry a warning rather than a reading. Neither symbol is an import; the mechanism is
relocation-shaped and which relocation has not been established, so it is written at that strength.

All three need a change to import resolution, which this session's brief places outside the work.

## Surprises

- **The corpus check was the finding.** It was meant to confirm nothing broke.
- **PPSA28061's `[experiment]` record reached 60 imports** under a diagnostic, against 47 honest
  and 26 today. So the title has been measured three ways and the two useful numbers are both in
  the past.

## Next

- Give `report_progress` the committed record, so a run below its own best says so. This would
  have caught the above the first time it happened.
- Bisect PPSA28061 against the build of 2026-08-23 - the record carries the date, so the method is
  clear.
- The three relocation walls (D633), waiting on scope.
- `ORBISTOUN_DLSYM_STUBS` as a default (D632), waiting on a decision.
