# 681. The published rung legend names all seven rungs, and a flip counter stops calling itself presented

**2026-09-17** — inbox `-288e`: two renamings, neither changing behaviour. The compatibility table's
own preamble named five of the seven reach rungs, so a reader of the published scale could not learn
that `presented` — the rung the scale exists to be able to say "not yet" about — is even there. And
`orbistoun_video::frames_presented` spent the one word this project reserves for a buffer with pixels
in it on a counter that increments when a flip is accepted.

## The legend now names seven

`crates/orbistoun-cli/src/main.rs` (the `compat markdown` preamble literal) read *"**Reach** climbs
`rejected`, `parsed`, `linked`, `entered`, `flipped` — the last meaning the guest got a frame to the
output layer"*. It now climbs all seven in order — `rejected`, `parsed`, `linked`, `entered`,
`exited`, `flipped`, `presented` — keeps `flipped`'s "a place reached, not a picture shown" (D558),
and adds that `presented`, the top rung, means a buffer whose pixels differ from what it held before
the guest ran, **and nothing in the corpus has reached it**. A table showing six titles at the top of
a five-rung ladder was D558's misreading arriving one layer up: a ladder whose top rung everything has
reached measures nothing about the work left.

Regenerated `COMPATIBILITY.md` carries the new preamble.

## The counter says flips, not presented

`frames_presented` → `flips_accepted` (`crates/orbistoun-video/src/lib.rs`), its heading reworded from
"frames handed to the output layer" to "flips accepted against a real port" (the body already drew the
accepted-vs-displayed line). Its one caller (`crates/orbistoun-worker/src/report.rs:1932`) and the two
prose references that name a live symbol followed it: the test doc at
`crates/orbistoun-report/src/trace.rs:1536` and the present-tense mechanism claim in the D558 decision
file. `grep -rn 'frames_presented' crates/` is now empty; the two remaining hits are in dated records
(worklog 406 and D558's history), left as written. The `Report.frames` field keeps its name — 288e
scoped the rename to the function.

## Generated files brought level with the records they derive from

Running `compat markdown` and the gates surfaced three generated artifacts sitting behind the records,
all traceable to this session's own work — no foreign change:

- **`docs/titles/` pages** (PPSA02664, PPSA03416, PPSA04263, PPSA25872) were days behind their own
  committed records; regeneration moved each forward to match. This is the drift `-17e5` also names,
  fixed mechanically from committed data.
- **`docs/compat-frontier.txt`** (the golden the `orbistoun-overrides` frontier test pins) lagged by
  one line — the PPSA02664 corpus-sweep numbers (211 answered / 418425 calls) I recorded this session.
  Regenerated with `UPDATE_FRONTIER=1`; the diff is exactly that line.
- **`README.md` / `docs/PROJECT_STATUS.md`** number blocks: `status --check` was red on
  `implemented 761 → 764`, `behaviours 812 → 814` (+1 measured, +1 assumed) and the PPSA02664 call
  count. I had earlier deferred this as possibly another session's compat change; measuring the actual
  `status --write` diff disproved that premise — **every** changed figure is this session's work (the
  three functions added across worklogs 664–680, their two knowledge entries, and the sweep). Leaving
  it would ship a README claiming 761 implemented when 764 are, the stale-count failure principle 3
  forbids, so I regenerated. Comparing values before acting is what turned a deferral into a fix.

## Gate state

`orbistoun-video`/`-worker`/`-report`/`-cli`/`-overrides` tests all pass (frontier green again);
`clippy -D warnings` clean on the four touched crates; `./bin/orbistoun prose` exit 0; knowledge-audit
27 pass; `status --check` exit 0; `cargo fmt --check` clean; identity scan clean. No commit.
