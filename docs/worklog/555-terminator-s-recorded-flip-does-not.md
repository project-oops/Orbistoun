# 555. Terminator's recorded flip does not reproduce, and it is not a regression

**2026-09-14** - chasing a lost frame, and finding the frame was never reproducible

## What I set out to check

`PPSA25872` (Terminator 2D: NO FATE) records `flipped`, 192 imports, **1 frame**. Runs today give
153 imports and **0 frames**. `obscene-payload` showed the same shape, 8 frames recorded against 0
now. Both looked like a regression in the flip path.

## It is not a regression

HEAD (`62bc866`) produces **exactly** the state worklog 528 documents as its *"Before"*:

| | worklog 528 "Before" | HEAD today | worklog 528 "Measured Now" |
|---|---:|---:|---:|
| imports | 153 | **153** | 192 |
| calls | 321,965 | **321,956-321,962** | 339,537 |
| fault | `image+0x17554a3` | **`image+0x17554a3`** | `image+0x3b32b9` |
| frames | 0 | **0** | 1 |

So the tree has not moved backwards. It is sitting where worklog 528 says it was *before* the advance
that worklog 528 reports - at the very commit whose message announces that advance.

## What was eliminated, and how

Everything cheap, and each by measurement rather than reasoning:

- **Uncommitted work** - a  at HEAD, built and run in isolation, gives the same numbers
  as the working tree. Neither reproduces the advance, so the other session's in-flight
  `orbistoun-gpu` changes are exonerated, and so is this session's `GetLoginUserIdList` (it accounts
  for +2 imports, 153 -> 155, and nothing else).
- **The time limit** - the record was made at `limit_seconds = 2`; I had been running 20. Both give
  153/0.
- **Nondeterminism** - five consecutive runs at HEAD: identical imports, identical fault, calls
  varying by ±3. The title is deterministic here, so the advance is not a lucky thread interleaving.
- **Overrides** - the record's `[status]` slot carries none, and a propped run would have gone to
  `[experiment]` (D312, D323).
- **The guest binary** - `eboot.bin` is dated 08-25, untouched.
- **The retained sandbox** - `ORBISTOUN_SANDBOX` defaults to *retain*, so runs share filesystem state
  and that looked promising. The title's `fs/` is empty.
- **Missing code** - the four AGC shader-linkage functions worklog 528 credits, plus
  `sceAgcCreateShader` and the `libSceAppContent` handlers, are all present at HEAD.

## What that leaves

The advance was recorded **twice** on 2026-09-13 - `docs/titles/PPSA25872-app0.md` says
`image+0x3b32b9` / 339,537 calls, `compat/PPSA25872-app0.toml` says `image+0x3b383b` / 339,539 - so
two runs did produce it. Nothing in this repository now does.

I have not found the cause and am not going to guess at it. The remaining candidates need the session
that made that commit: most plausibly the advance depended on tree state that was revised before it
was committed, which the commit could not capture and no rerun can recover.

## Why this matters more than one title

`compat/` holds 192/1 as Terminator's best-ever, and by the recording rules nothing reaching 153/0 can
displace it - correctly. But that means **the frontier advertises a result the project cannot
currently demonstrate**, and ranks Terminator third-furthest on it. A record is a claim about what the
emulator does; this one is a claim about what it did once.

**Not corrected here.** Overwriting another session's recorded measurement on my own reading is
exactly the wrong move, and `--force` exists for a person to decide rather than for me to. Reported
instead.

## Surprises

**The regression I was chasing was the baseline.** Every number I had been treating as evidence of a
fall was already written down as the state before a rise - in a worklog I had read that morning, in
the "Before" column of its own table. Reading the change description before measuring the change would
have cost one minute and saved the whole investigation.

**`obscene-payload` was never comparable.** Its binary is re-synced from `../obscene/build` on every
corpus run as a `LocalSnapshot`, and obSCEne has been rebuilt repeatedly today. Its 8 frames and its 0
frames are different guests, so nothing follows from the difference.

## Addendum: the parent commit too, and what it rules out

A hypothesis worth testing was raised: that the guest takes a *different* code path once more is
implemented, so a more correct run touches fewer imports and only looks like a fall. That has a
strong form and a weak form, and they came apart.

**The strong form - that `62bc866` itself moved Terminator onto a different path - is refuted.** Its
parent `ae28a80`, built and run in its own worktree, gives **153 imports, 0 frames,
`image+0x17554a3`** - identical to `62bc866` and to the working tree. Neither commit produces the
advance, so no committed change moved the guest between them.

That closes the last candidate this repository can reach. Eliminated in total: uncommitted work, the
time limit, nondeterminism (five runs), overrides, the guest binary, the retained sandbox, the
credited code being absent, and now the commit boundary itself. The symbols database is shared
mutable state and was the remaining suspect, but the **main working tree** - which has it - gives the
same 153/0 as the worktrees, so it is not the difference either.

The cause of the two 09-13 runs that produced 192/1 is outside anything I can reach, and I am
stopping rather than proposing a mechanism I cannot test.

**The weak form stands, and on its own evidence.** A guest doing the right thing can touch fewer
imports: binding `_exit` this morning stopped the conformance payload making six calls it had only
ever made by running past its own refused exit, and the verdict came back `BACK - reaching less of
the interface than it did`. Strictly more correct, scored worse. One number here points the same way -
the record is 192 imports with **14 unanswered**, today is 153 with **2** - which is the shape of a
path where far less is missing, not of a run that simply died earlier.

That is a finding about `ranking_key`, not about Terminator, and it is the third instance today
(D686, D687 and this). Recorded rather than acted on: changing what counts as progress on the
strength of an unreproducible record would be the wrong way round.
