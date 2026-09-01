# D240 - The numbers in the documentation are generated, and a check fails when they drift


**decided** · 2026-08-25 · the page's own rule, applied to the page

`docs/PROJECT_STATUS.md` says its numbers are printed by the tool rather than counted by
hand, and that *"a number in this document that no command produces is a number that will
be wrong within a week"*. Both statements were true. The numbers drifted anyway:

| | the docs said | the tool said |
|---|---|---|
| Knowledge oracles | 44 published, 28 guest-observed, 9 assumed | 45, 26, 10 |
| Open questions | 76 | 70 |

`README.md` carried the same stale figures independently, so two files disagreed with the
tool and with each other. Being right about the rule is not the same as being bound by it.

`orbistoun-cli status` emits the block; `--write` splices it between markers; `--check`
fails when a file differs, and `./orbistoun.sh check` runs it.

### What is deliberately not in the block

Only numbers the tool can recompute **anywhere, from what the repository ships**. Two of the
rows could not be: "6 of 6 titles execute guest code" and "733 imports, 565 unnamed" both
need a title, and the corpus is not tracked and never will be.

A generated block containing them would fail for every contributor who has no titles, and
would be unverifiable in CI - so the check would have to be weakened to ignore them, which
is the drift it exists to catch wearing a badge. They moved into the prose around the block
instead, where they read as what they are: measured from a run, on one machine, on material
this repository does not ship.

### Made to fail first

A drift check that cannot fail is a check that reports success. Before trusting it, a number
in `README.md` was changed by hand: `status --check` exited 1 and named the file; restored,
it exited 0. `splice_block` is tested for the case that would be worst - a file whose
markers have gone - and refuses rather than skipping it, because a check reporting success
over a file it never looked at is worse than no check at all.

That is the same lesson as D229, D230 and D239, which is now four in one day: **the failure
mode of this project's tooling is reporting success it did not establish.**

