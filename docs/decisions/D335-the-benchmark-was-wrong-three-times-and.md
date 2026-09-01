# D335 - The benchmark was wrong three times, and the third nearly retired a working engine


**decided** · 2026-08-27 · asked whether to drop an engine, and the honest answer was to fix the measurement

`local-cpu` - a 1.7-billion-parameter model run in this process on the processor - scored
nothing on the benchmark and looked like dead weight in the ladder. The question was whether
to drop it. It should not be dropped: **it is the second-best engine on this machine**, and
better than the four-billion-parameter model on the accelerator above it.

| | novel words | time |
|---|---|---|
| Claude Code | 7 of 12 | 55.8 s |
| in-process 1.7B on the processor | **4 of 12** | 194.2 s |
| 4B on the accelerator | 2 of 12 | 0.6 s |

**Every one of the three ways it scored zero was the measurement's fault, not the engine's.**
D334 records the first two - ranking by speed picks the engine that says least, ranking by
volume cannot tell twelve-of-twelve from twelve-of-twelve. This is the third and the worst,
because it produced a confident recommendation to delete something that works.

The scorer demanded a JSON array. The proposal loop reads a reply with **three fallbacks** -
a JSON array, then quoted strings, then bare tokens - so a reply this engine makes daily use
of scored zero in a benchmark meant to predict exactly that. A benchmark stricter than its
consumer measures the benchmark.

So `measure` takes the caller's scoring function, and the tool passes one built on the
loop's own reader. The strict version survives only as a default for a caller with no parser
of its own.

**What made it findable was quoting the reply.** "Answered, but not with a list of words"
names the shape and withholds the evidence. The quote read
`<think> </think> Here is a list of **12 new variant markers**...`, which says three things
at once: the engine works, the model is on topic, and it is emitting a reasoning block the
managed path suppresses with `--reasoning off` and this one has no way to. None of that is
visible without the words.

**A note on variance, having twice claimed more than one sample supports.** Across three
runs Claude Code scored 9, 10 and 7; the accelerator model scored 2, 2 and 2. The top and
bottom of that ladder are settled. `local-cpu` at 4 is one measurement, and one measurement
is what this entry is about.

**The tier stays regardless of the number.** It is the only thing that answers on a machine
with no accelerator, no coding assistant, no model server and no key - which is the case the
whole crate was asked to handle without setup.

