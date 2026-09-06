# D542 - Six records said nothing was known while holding a measurement, and the axis closes

**decided** - 2026-09-04

The plan's one open question: `known_by` was wrong on an entry for weeks and nothing noticed.
Check 10 is gated in one direction - a `measured` entry may not list open questions. Is the
converse worth a gate?

**Check what it would fire on before building it** was the instruction, and it was the right
one, because the obvious rule is wrong.

## The obvious rule fires on 22 and is wrong about 16

*An entry with a hardware measurement may not be `assumed`.* Twenty-two entries are in that
state. Sixteen of them are **correct**: a console answering one behaviour does not establish the
rest, and check 10 exists precisely to stop one measured fact promoting an entry past the
questions it still lists. `known_by` describes the entry, not its strongest single fact.

So the rule would have been a guard that fires mostly on things that are fine - which is a guard
somebody weakens the first time it costs them an afternoon.

## Six are a contradiction, not a judgement

The other six list **no** assumptions at all, so their records print the sentence an entry gives
when it rests on a guess and itemises nothing:

> Nothing about this entry has been established.

Two lines above, in the same record: `Measured on hardware: obSCEne ... reported pass, value ...`

```text
sceKernelGetSystemSwVersion  sceGnmDispatchDirect  sceGnmDispatchInitDefaultHardwareState
sceSysmoduleIsLoaded         sceVideoOutGetResolutionStatus       sceVideoOutSetFlipRate
```

That is D537 again - reasoning in the code, nothing in the record - and it **survived five ticks
of auditing this exact thing**. `questions` ranks by call count; these have between none and
eleven, so they sat at the bottom of every list read from the top. The audit had a blind spot
shaped like its own ranking.

And the reasoning waiting in the code was not thin. `sceKernelGetSystemSwVersion` documents the
whole structure, that the number is **not** the firmware, that obSCEne measured `13.090.001`
across three runs while the same console's banner says 12.40, and that wiring it to the firmware
would answer a value hardware refutes (D420). `sceVideoOutSetFlipRate` **discards the rate
entirely** - it validates the handle and throws the value away - which no reader of the record
could have known. All six are now written down.

## The guard is the narrow one

`an_entry_holding_a_measurement_does_not_say_nothing_is_established`. Not a claim about how
entries should be labelled - a claim that a record must not contradict itself. Made to fail by
emptying one of the six.

It says what it cannot do: it does not look at the sixteen, and it cannot see a measurement
described in prose that does not begin `Measured on hardware:`, because that prefix is what the
absorption writes and this checks the absorption's own output.

## The axis closes here

Six ticks, D537 to D542, on the list this project hands to a console.

**What it bought.** `questions --premises`, which turned a queue nobody could see the shape of
into 142 sentences with their weight attached. One question that was written 149 ways is now
written once - a fifth of the list. Nine claims that were **false** are gone, and the one they
were hiding worst (`sceKernelWaitSema` ignoring two of its three arguments) is recorded. One ask
was **withdrawn from the console queue** because hardware had already answered it and the entry
said so in its own open-question list. Six records stopped contradicting themselves. Seven
guards now hold all of it, and every one has been watched failing.

**What it cost.** Six ticks and no movement on the wall, which has not moved since D528 named it
as needing a console. That is the honest accounting, and the defence is narrow but real: the ask
list *is* the handoff to hardware, hardware is the only thing that can move the wall, and a
handoff that asks for a function that does not exist, or for an answer already given, spends the
scarcest resource this project has on nothing.

**What is deliberately left.** The sixteen ambiguous labels above - `known_by` is a judgement
there and check 10 already governs it. Three terse arity questions and two unpunctuated ones:
cosmetic, and saying so is cheaper than fixing them. The 29 records that still say nothing has
been established hold 86 calls between them and no measurements, which is the honest state of a
declaration nobody has exercised.

The next axis is not this one. If it is the ask list again, it should be because a *console run*
came back, not because the prose can be improved further.
