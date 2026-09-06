# 2026-09-04 - (/loop) Two thirds of the ask list is forty sentences

```
719 open questions -> 275 premises   40 of them carry 484
questions --premises   |   one premise was written two ways; now gated
```

Twenty-fourth cron tick. The plan's item (a) settled in a minute - the 35 remaining empty records
hold 86 calls and twenty of them have never been called at all, so that axis is closed rather
than worked. The clustering run to check it is what produced this.

## The finding

`questions` prints one line per function per open question. Grouped by the sentence:

```text
719 open questions
275 premises
 40 premises carry 484 of the 719      (235 rest on one entry each)
  6 premises carry 355                 (49% of the list)
```

The heaviest is one sentence under fourteen functions across two libraries and 872,904 calls -
the POSIX-correspondence claim under `sceKernelWrite`, `sceKernelRead`, `sceKernelOpen`,
`sceKernelClose` and the whole `scePthread*` mutex family.

Fourteen identical asks are not fourteen pieces of work. A shared premise is answerable by
**sampling** it, and a single counter-example refutes the group outright.

## Built

`orbistoun_hle::knowledge::shared_premises` - a pure grouping over (function, question) pairs -
and `questions --premises`, which ranks groups by the calls behind them and names the functions
under each so a sweep can pick which to sample. `--premises --json` too, since the caller writing
backlog 022 is the one that wants it.

Grouping is **word-for-word identity**, never similarity: case, spacing and punctuation forgiven
and nothing else. That keeps D356's boundary intact - nothing here reads a question - and makes
275 a floor rather than an estimate.

## The defect on the way in

Fourteen entries meant one sentence and four punctuated it with a semicolon. My own census had
said ten functions and 872,301 calls; it was fourteen and 872,904. **A third census filtered by
its own key** - the loop file's check 4, again.

Fixed in the data rather than by matching more loosely, and gated:
`no_premise_in_the_knowledge_base_is_written_two_ways` went red on the data that prompted it.

## The mode's own defect

`--top` was applied to the queue before grouping was added, so `--premises --top 1` reported
**one** function on the heaviest premise, 806,669 calls, under `3 premises behind 3 open
questions`. It is fourteen and 872,904. Nothing about that output looks wrong - a fact about
`--top` printed as a fact about the knowledge base.

Fixed by grouping before truncating, and guarded in `crates/orbistoun-cli/tests/premises.rs`
against the tool's own full-run answer rather than a hardcoded count. Both guards were made to
fail by reinstating the ordering.

## What this is not

Progress on the wall, and nothing established about the platform. It is a claim about the ask
list being countable - the same failure as D537 one level up.

Gates: 125 suites, clippy/fmt/identity clean on both repos. Noted in obSCEne backlog 022.

Decision: [D538](../decisions/D538-two-thirds-of-the-ask-list-is-forty-sentences.md).
