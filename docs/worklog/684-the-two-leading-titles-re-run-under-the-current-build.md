# 684. The two leading titles re-run under the current build — the wall holds, the answered count climbs

**2026-09-17** — inbox `-0adc`: the two titles nearest a picture (PPSA02664, PPSA03416) carried
outcomes measured before this session's AGC and register work, so their published position was the
least trustworthy figure in the table. `-4059`'s own note said the `0xf7ff0001`-through-`memcpy`
fault was cleared in-tree and *"whether they then reach a new wall needs a run."* This is that run.

## Both re-run, both confirmed at the same wall

`./bin/orbistoun run` (which auto-records an improvement, keeping the best) was run on each under the
current build:

- **PPSA03416-app0** advanced: `measured_on` 2026-09-15 → **2026-09-17**, and **unanswered calls
  29 → 9** (answered functions 193 → 213). That is the AGC patch family (`agc_patch_returns_ok`)
  and the register work answering twenty imports the older record still had on stubs. The wall is
  **unchanged**: `VCRUNTIME140.dll+0x1dc8d`, a read of `0xa8`, reached at `flipped`.
- **PPSA02664-app0** was already refreshed to 2026-09-17 by this session's sweep; a confirming run
  returned verdict `same` (222 imports, `VCRUNTIME140.dll+0x1dc8d`), so its record stands
  (211 answered, unanswered 11).

**The wall is the same wall, and that is a result, not a disappointment** (0adc's own words): the AGC
placeholder fault `-4059` cleared is gone — both titles now die deeper, at the host `memcpy` reading
`0xa8` (a null-object field inside Unity's command-buffer build), not at the old
`0xf7ff0001`-carrying patch chain. The builder work did what it was meant to; it exposed the next
wall rather than removing this one.

**On the import jitter.** Sampled across five runs, PPSA03416 returns 221–222 distinct imports and
470419–470423 calls — a one-import, thread-timing non-determinism around the crash, the wall
identical every time. The recorded 222/470421 is the honest high-water reach, not a cherry-pick; the
tool keeps the best a run legitimately reaches and refuses a regression without `--force`.

## Generated surfaces regenerated (and 58d7 closed with them)

After the records moved, all four generated surfaces were brought level: `docs/compat-frontier.txt`
(`UPDATE_FRONTIER=1`), `COMPATIBILITY.md` + `docs/titles/` (`compat markdown`), and
`README.md`/`docs/PROJECT_STATUS.md` (`status --write`). The frontier now reads PPSA03416 at 213
answered and PPSA02664 at 211.

This also satisfied **`-58d7`** (two generated surfaces behind the tool): its three acceptance checks
— `status --check` exits 0, a second `compat markdown` leaves `git status` unchanged, and
`docs/titles/PPSA02664-app0.md` reads 222 imports / `VCRUNTIME140.dll+0x1dc8d` — all hold. As of
worklog 682 `compat markdown --check` runs in `./bin/orbistoun check`, so this surface can no longer
drift silently.

## Gate state

`status --check` exit 0; `compat markdown --check` exit 0; the `orbistoun-overrides` frontier test
passes on the regenerated golden; `./bin/orbistoun prose` exit 0; `cargo fmt --check` clean; identity
scan clean. No commit.
