# 682. `compat markdown --check` — the two generated docs `status --check` never covered

**2026-09-17** — inbox `-17e5`: `status --check` guards `README.md` and `docs/PROJECT_STATUS.md`,
and nothing guarded `COMPATIBILITY.md` or the `docs/titles/` pages. That gap is how a generated file
whose own header says *"Do not edit by hand; re-run it"* was hand-edited (commit `e89139a`) and its
sibling pages left days behind — the drift worklog 681 had to regenerate by hand. A gate is what
stops that being a recurring correction.

## The `--check` arm

`cmd_compat_markdown` (`crates/orbistoun-cli/src/main.rs`) gained a `--check` flag mirroring
`cmd_status`'s shape: it renders the table and every per-title page **into memory**, compares each
against what is on disk, and fails naming every file that differs — a missing file (a whole page
deleted) reads as drift the same as a differing one. It is read-only: `--check` touches nothing.
Orphan pages (a page with no record) are deliberately left alone, because a write leaves them alone
too, so check and write agree on exactly which trees are current.

Refactored to stay honest and under the line limit (principle 8, a pure function plus a thin
wrapper): the record-reading loop is now `compat_rows`, and the disk comparison is `compat_drift`
(pure, read-only, returns the named files). `cmd_compat_markdown` renders once and either writes or
checks from the same rendered content, so the two arms cannot disagree about what "current" means.

## Wired into the gate, and watched failing

`bin/orbistoun`'s `generated_numbers` step (run by `check`) now runs `compat markdown --check` beside
`status --check`. Note the inbox's "the `docs` step" was a mislabel — `docs()` is `cargo doc`; the
`status --check` at main.rs's old line 323 lives in `generated_numbers`, which is where the sibling
belongs and where this went.

Verified failing on material built to make it fail (D213), both by hand and by an automated test:

- **By hand:** editing one cell of `COMPATIBILITY.md` (`444296` → `444297`) made `--check` exit 1
  naming `COMPATIBILITY.md`; deleting a line from `docs/titles/PPSA99980.md` did the same naming the
  page; regenerating restored exit 0. Both files restored (regeneration / `git checkout`).
- **Automated:** `compat_check_rejects_a_hand_edited_generated_file` builds a one-record temp tree,
  writes it, asserts `--check` passes, then corrupts a table cell (asserts it names the table) and
  deletes the page (asserts it names the page). A check that only ever answered "current" would pass
  either alone, so both are exercised.

## Gate state

`clippy -p orbistoun-cli --all-targets -D warnings` clean (the refactor cleared the `too_many_lines`
the first draft tripped); `orbistoun-cli` tests 19 + integration all pass; `compat markdown --check`
exit 0 on the real tree; `./bin/orbistoun prose` exit 0; `cargo fmt --check` clean; identity scan
clean. No commit.
