# D537 - The reasoning was in the code, and the record said nothing

**decided** - 2026-09-04

`orbistoun-cli questions` ranks every unverified claim by how often a guest calls the function,
and obSCEne's backlog 022 is generated from it. So the ask list is only as good as the records
it reads.

Forty entries said **"Nothing about this entry has been established"**, and behind them sat
**431,213 calls**.

## They were not unknown; they were unrecorded

Every one of the five heaviest is implemented, and every one carries substantial reasoning in
its own doc comment. `sceVideoOutGetFlipStatus` - 200,168 calls - documents which field it
writes, why only that field, and what a caller reading past it gets. The knowledge record for it
was an arity, a one-line purpose, and nothing else.

So `questions` reported a total unknown about a function orbistoun has reasoned about carefully,
and the generated backlog asked a console to establish it from scratch.

## The split that makes the ask precise

Transcribing is not copying. The doc comments hold two different kinds of statement and they
belong in different fields:

- **What orbistoun does, and why** - "the channel argument is not modelled", "only the count is
  written", "an address in no region is answered with the code the console answers for one".
  These are *facts about this emulator*. They go in `edge_cases`, where they inform a reader
  without asking anybody for anything.
- **What is assumed about the platform** - "that both of the console's channels are the
  operator's log", "that the completed count is the head of `SceVideoOutFlipStatus` at offset
  0". These are the open questions. They go in `assumptions`, where `questions` ranks them and
  the backlog asks for them.

An empty record collapses that distinction into one useless sentence. Filling it in changes what
a console is asked to do from *"establish this function"* to *"check whether the count is at
offset 0"*.

## What it moved

```text
before   40 entries with nothing established, 431,213 calls behind them
after    35 entries,                                86 calls behind them
```

Five entries carried **99.98%** of the weight. That is worth stating on its own: the ranking was
right and the records were the problem, so a small, targeted transcription fixed almost all of
it - which is the opposite of what "forty empty records" suggests as a work item.

## And two of them are the ones already blocking

`sceVideoOutGetFlipStatus` and `sceVideoOutSubmitFlip` are the flip model, which is the same
subsystem `sceKernelWaitEqueue` is waiting on (D528). Their assumptions now say precisely which
byte is uncited, beside the two layout asks already in backlog 022 - so a console sweep covering
the flip structure answers three entries rather than one.

## What this is not

Progress on the wall, and not new knowledge either - nothing here was learned this tick. It is
the same failure D528 found one level up: **a mechanism that asks for what it needs was asking
badly, because nothing had been written into it.** The fix is transcription, and the reason it
was worth a decision is the ratio - five records, four hundred thousand calls.
