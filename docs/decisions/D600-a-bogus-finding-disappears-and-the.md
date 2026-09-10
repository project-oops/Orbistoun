# D600 - A bogus finding disappears, and the check passes on a run that still varies

**Status:** measured
**Date:** 2026-09-08

## Re-deriving what the wrong configuration produced

D599 found the dispatcher had been sweeping a guest with a different set of named imports from
the one the loop measures. Every conclusion it drew this session was therefore taken against a
different program, and the honest position was that each needed re-running rather than
re-reading.

Re-run against PPSA03416 with the database:

**The starred `libc::memcpy answered the code the guest followed` finding is gone.** It was the
headline result of D583 - the thing `Step::CheckRepeats` caught as drift - and it does not occur
when the guest is given the symbols it is given by the loop. So the check was right that the
finding was noise, and wrong about what kind: not run-to-run variation but a configuration
artefact.

D583's conclusion survives unchanged. The check earned its keep by refusing a finding, and the
finding was worse than it looked.

## The check now passes, and the run is still not repeatable

```text
two runs agree: 193 imports, fault 0xa0
```

and five runs of the loop give 193 distinct imports every time, where four earlier ones gave
192, 192, 193, 192. **That claim was too strong and is withdrawn.** Three runs immediately after D601 gave 193, 192,
193. Five runs agreeing is not a measurement of determinism - which the caveat at the foot of
this entry says in as many words, and the headline said the opposite anyway. The drift is
smaller than it was and it is not gone.

On the sharper signal it did not:

| | |
|---|---|
| imports, five runs | 193, 193, 193, 193, 193 |
| mapping sequence, three runs | **1 to 2 of 47 identical** |

`CheckRepeats` compares where the guest died and how far it got. Both are stable. The order and
addresses of the mappings it made are not, and nothing in the dispatcher reads them - so the
check passes on a run that varies in a way a future step could easily depend on.

**D583 wrote that down before it happened**: *"a run that repeats on them and varies elsewhere
would pass here and still poison a step that read something else - and D581's mapping record
already shows one"*. It is worth recording that the anticipated case arrived, because a caveat
nobody revisits is indistinguishable from one nobody needed.

## The correct invocation is a verb now

`orbistoun-cli turn` needs `--symbols-db` to measure the right program, and a flag somebody has
to remember is what cost a session's verdicts. `./bin/orbistoun turn <title>` resolves the title
and passes the database the same way `run` does.

The fix in D599 made the tool capable of being right. This makes being right the default path,
which is the half that stops it happening again.

## What this does not establish

**That every other turn conclusion from this session is void or sound.** One was re-derived. The
axis sweeps, the placeholder hunts and the read-structure results were not, and each is a boot
away from being checked.

**Nor why the mapping sequence varies.** The first mapping already differs, so it is not the
thread creation D582 identified - that begins around mapping seventeen. Something earlier moved
and nothing here says what.

**Nor that five runs is a measurement of determinism.** It is five runs. The figure that has
repeatedly misled here is the single pair, and five is better rather than sufficient.
