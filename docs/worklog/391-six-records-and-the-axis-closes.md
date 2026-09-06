# 2026-09-04 - (/loop) Six records said nothing was known while holding a measurement

```
743 questions / 136 premises -> 743 / 142      "nothing established": 35 -> 29
seven guards over the ask list, every one watched failing.  Axis closed.
```

Twenty-eighth cron tick. The plan's one open question - is the converse of check 10 worth a
gate? - with the instruction to **check what it would fire on before building it**. That was the
right instruction, because the obvious rule is wrong.

## The obvious rule fires on 22 and is wrong about 16

*An entry with a hardware measurement may not be `assumed`.* Twenty-two entries are like that,
and sixteen are **correct**: a console answering one behaviour does not establish the rest, and
check 10 exists to stop one measured fact promoting an entry past the questions it still lists.
A guard firing mostly on things that are fine is one somebody weakens.

## Six are a contradiction

The other six itemise nothing, so their records print *"Nothing about this entry has been
established"* - two lines under `Measured on hardware: obSCEne ... reported pass, value ...`.

D537 again, and it **survived five ticks of auditing this exact thing**, because `questions`
ranks by call count and these have between none and eleven. The audit had a blind spot shaped
like its own ranking.

The reasoning waiting in the code was not thin. `sceKernelGetSystemSwVersion` documents the
structure, that the number is *not* the firmware, and that obSCEne measured `13.090.001` across
three runs while the console's banner says 12.40 - wiring it to the firmware would answer a value
hardware refutes (D420). `sceVideoOutSetFlipRate` **discards the rate entirely**. All six written
down.

Guard: `an_entry_holding_a_measurement_does_not_say_nothing_is_established` - narrow, because a
record contradicting itself is a fact and a label is a judgement. Made to fail by emptying one.

## The axis closes

Six ticks, D537-D542.

**Bought**: `questions --premises`; one question written 149 ways now written once (a fifth of
the list); nine false claims gone and the worst thing they hid recorded (`sceKernelWaitSema`
ignores two of three arguments); one ask withdrawn from the console queue because hardware had
already answered it; six self-contradicting records fixed; seven guards, every one watched
failing.

**Cost**: six ticks, no movement on the wall. The defence is narrow: the ask list is the handoff
to hardware, hardware is the only thing that can move the wall, and a handoff asking for a
function that does not exist spends the scarcest resource here on nothing.

**Left deliberately**: sixteen ambiguous labels (check 10 governs them), three terse arity
questions and two unpunctuated ones (cosmetic - saying so is cheaper than fixing them), and 29
empty records holding 86 calls and no measurements, which is the honest state of a declaration
nobody has exercised.

Decision: [D542](../decisions/D542-six-records-said-nothing-was-known-while-holding-a-measurement.md).
