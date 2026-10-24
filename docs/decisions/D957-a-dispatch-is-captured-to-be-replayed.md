# D957 - A dispatch is captured to be replayed alone

**Status:** decided
**Date:** 2026-10-24
**known_by:** measured

A guest compute dispatch can be written to disk with everything it was handed - its module, the
guest-memory window it binds, push constants, groups, the images and buffers it binds - and run
again alone by `orbistoun-cli replay-dispatch`, which reports each run's device time, whether the
device was lost, and a hash of everything it left. A dispatch is captured before it is submitted
when `ORBISTOUN_CAPTURE_DISPATCH` names its program, and after it fails when it is the one that lost
the device. The capture is taken where every guest dispatch passes, from the inputs it already
holds, and a replay hands them back through the same path, so a fix to that path is what the
replay runs.

**Why:** Grand Theft Auto V's first in-world dispatch of the program at `image+0x3dc3400` lost the
device forty minutes into a run. Reproducing it meant booting the game again, and whether a change
fixed it could only be seen at the end of another such run. A capture reproduces it in seconds, and
two builds compare by the output hash.

**Rejected:**
- Copying each dispatch's inputs ahead of time, in case it fails: the memory window, on every
  dispatch.
- Capturing every program the first time it runs: a write for each new program, most of which never
  fail.
