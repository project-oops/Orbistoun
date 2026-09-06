# 2026-09-04 - (/loop) The reasoning was in the code, and the record said nothing

```
empty records   40 -> 35     calls behind them   431,213 -> 86
suites 125   tests 2015   clippy/fmt/identity clean
```

Twenty-third cron tick. A new axis, since the named list was empty: run "implemented but
unverified" against libkernel with `orbistoun-cli questions` rather than the differential, which
is a *glibc* reference and cannot speak for platform functions.

## What the ranking showed

Forty entries said **"Nothing about this entry has been established"**, and **431,213 calls** sat
behind them.

They were not unknown - they were **unrecorded**. Every one of the five heaviest is implemented
and carries real reasoning in its doc comment. `sceVideoOutGetFlipStatus`, at 200,168 calls,
documents which field it writes, why only that one, and what a caller reading past it gets; its
record was an arity, a one-line purpose, and nothing else.

So `questions` reported a total unknown about a function orbistoun has reasoned about carefully,
and the generated backlog asked a console to establish it from scratch.

## The split that makes the ask precise

Transcribing is not copying - the doc comments hold two kinds of statement:

- **What orbistoun does and why** ("the channel argument is not modelled", "only the count is
  written") are facts about this emulator. They go in `edge_cases`, informing a reader without
  asking anybody for anything.
- **What is assumed about the platform** ("that the completed count is the head of
  `SceVideoOutFlipStatus` at offset 0") are the open questions. They go in `assumptions`, where
  `questions` ranks them and the backlog asks for them.

An empty record collapses that into one useless sentence. Filling it changes the ask from
*"establish this function"* to *"check whether the count is at offset 0"*.

## The ratio is the finding

Five entries carried **99.98%** of the weight. The ranking was right and the records were the
problem, so a small targeted transcription fixed nearly all of it - the opposite of what "forty
empty records" suggests as a work item.

## And two are already blocking

`sceVideoOutGetFlipStatus` and `sceVideoOutSubmitFlip` are the flip model - the same subsystem
`sceKernelWaitEqueue` waits on (D528). Their assumptions now name the uncited byte, beside the
two layout asks already in backlog 022, so **one flip-path capture answers three entries rather
than one**. Noted there, along with the count, which had drifted to 719.

## What this is not

Progress on the wall, and not new knowledge - nothing was learned this tick. It is D528's failure
one level up: **a mechanism that asks for what it needs was asking badly, because nothing had
been written into it.**

Decision: [D537](../decisions/D537-the-reasoning-was-in-the-code-and-the-record-said-nothing.md).
