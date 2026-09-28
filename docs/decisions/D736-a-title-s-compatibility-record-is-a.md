# D736 - A title's compatibility record is a directory

**Status:** decided
**Date:** 2026-09-28

Each title's record lives in `compat/<title>/`, not a single `compat/<title>.toml`. The directory
holds:

- `report.toml`, the record as before, with a `[reproduce]` section naming what a run needs to
  repeat: the title build's hash, the orbistoun commit, the limits, the clock and the diagnostics.
- `inputs.toml`, the pad script the run replays, when it needs one.
- `frames.png`, a sheet of the frames the run drew, one every so many flips, in order.

The title's generated page embeds the sheet and says how to reproduce the run. `report.toml` and
`inputs.toml` are committed for every title: they are settings, hashes and button presses, never
guest material. `frames.png` is committed only for a title built from open sources in this
collection; a retail title's frames are the publisher's output, so they are written to the local
title library, and the page says the sheet is local and how to make it.

**Why:** a record said how far a run got, never what it looked like or how to see it again. A
sheet of frames shows at a glance whether a menu came up and drew, and the reproduction steps
beside it make the record something anyone with the title can check rather than take on trust.
One directory keeps a title's evidence together. Runs repeat under the logical clock (D582, D735)
until a title's logic waits on another thread's progress, so a re-run reproduces the sheet that
far.

**Rejected:**
- Loose files beside `compat/<title>.toml` (`<title>.inputs.toml`, `screenshots/<title>.png`): a
  title's evidence scattered across names that must be kept in step by hand.
- Committing every title's frames: a retail title's frames are not ours to publish (the provenance
  guard exists for exactly this).
- A sheet per run: history belongs to git; the directory holds the current evidence.
