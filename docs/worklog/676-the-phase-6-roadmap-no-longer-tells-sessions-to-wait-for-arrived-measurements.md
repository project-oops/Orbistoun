# 676. The phase-6 roadmap no longer tells sessions to wait for arrived measurements

**2026-09-17** — inbox `-732b`. `docs/roadmap/015-phase-6-s-contents-built-ahead-of-it.md` still told a
reader that two steps "needs a capture" for work the tree has done, which is the framing that makes
phase 6 look circular (a capture needs a rendering path, a rendering path needs a capture). Two of the
places the circle was said to bind are already open. Restated - and while there, the G15 rows stale
after this session's `-db54` consume.

## What was corrected

- **G9** ("harness done, corpus empty - needs a capture"): the corpus is not empty. `tests/vocabulary.rs`
  walks the two `agc-gl-cube-fw1240` captures in `tests/captures/` through `register_writes` against
  `data/packets.toml`. The row now says so, and names the narrower thing actually open: `oracle_gl_cube.rs`
  prints its pixel hashes rather than asserting them, because the streams' inputs are deliberately
  unmapped.
- **G11 step (d)** ("which registers configure colour buffer zero - needs a capture"), in the table row
  *and* the (d) sub-table and its "only the third needs a capture" lead-in: that decode is written -
  `colour_target_at` reads `CB_COLOR0_BASE`/`ATTRIB2`, `colour_swizzle_mode_at` the tiling, cited to a
  register database (worklogs 655/637/657), and the `Submission` carries them now (worklog 675). What is
  left of (d) is the backend consuming that state, which waits with the backend, not a capture.
- **G15** (two places - the summary row and the detiling narrative): both said obSCEne's full
  texel→offset sweep "is in flight / being built". It landed: `-db54` confirmed `(15,15)` → 4348,
  `(32,21)` → 2640 and a bijection over the whole 128×128 macro-tile (worklog 674), and the equation
  reproduces the second point on its own. Updated to the delivered fact, and to note the format guard
  (worklog 667) and the `Submission` carry (worklog 675).

## Why it is worth a change on its own

A plan is read by the next session to decide what to do. One that points at measurements which have
arrived spends direction, not just accuracy - it tells a session to wait for something it could pick up
today. That is the same class of cost the decision/worklog gates exist to catch one directory over;
the roadmap has no generator, so it is corrected by hand when the code moves under it.

## Gate state

Documentation only, no code. `./bin/orbistoun prose` is not affected (markdown, no Rust string
literals); identity scan clean. No commit.
