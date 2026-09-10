# 443. Re-derived

**2026-09-08** - directed, continuing 442

## What was done

**Made the correct invocation the easy one.** `orbistoun-cli turn` needs `--symbols-db` to
measure the program the loop measures, and a flag somebody has to remember is what cost a
session's verdicts (D599). `./bin/orbistoun turn <title>` now resolves the title and passes the
database exactly as `run` does. Fixing the tool made it capable of being right; this makes being
right the default path.

**Re-derived PPSA03416's turn against the right symbol set** (D600):

- **The starred `libc::memcpy answered the code the guest followed` finding is gone.** It was the
  headline of D583 - the thing `CheckRepeats` caught as drift - and it does not occur when the
  guest gets the symbols the loop gives it. The check was right that it was noise and wrong about
  the kind: a configuration artefact, not run-to-run variation. D583's conclusion stands.
- **`two runs agree: 193 imports, fault 0xa0`**, where the same turn previously reported a
  disagreement.

## The determinism work landed, on one signal

| | |
|---|--:|
| imports, five runs | 193 every time |
| the same, four runs two iterations ago | 192, 192, 193, 192 |
| **mapping sequence, three runs** | **1 to 2 of 47 identical** |

So the logical clock, the single region for guest-visible handles, and the configuration fix
together made the coarse signal stable - and the mapping sequence is still not.

`CheckRepeats` compares where the guest died and how far it got, and both are stable now. **D583
wrote this case down before it happened**: a run can repeat on those two and vary in something no
step reads. The anticipated case arrived, which is worth recording, because a caveat nobody
revisits is indistinguishable from one nobody needed.

## Surprises

- **The bogus finding was worse than "noise".** It was a stable, reproducible artefact of a
  configuration nobody knew was different - so re-running it would have reproduced it every time
  and confirmed it.
- **The mapping sequence diverges at the first mapping**, not at the thread creation D582
  identified around mapping seventeen. Something earlier moved and nothing here says what.

## Next

- Re-derive the rest of this session's turn conclusions. One down; the axis sweeps, the
  placeholder hunts and the read-structure results are each a boot away.
- Why the mapping sequence diverges immediately.
- The clean title's wall, which is platform work with no shim in it.
