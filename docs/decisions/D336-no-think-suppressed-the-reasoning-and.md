# D336 - `/no_think` suppressed the reasoning and not the tags


**decided** · 2026-08-27 · found by quoting a reply instead of describing it

The in-process engine already appends `/no_think` to the system message for the models that
understand it, and the managed runtime passes `--reasoning off`. Both were doing their job.
The reply still arrived as:

```
<think> </think> Here is a list of **12 new variant markers**...
```

The block is **empty** - so the instruction worked, the narration was suppressed, and the
model emitted the tags anyway. Every reader downstream then saw a reply beginning with a
tag, failed to parse it as the array it was asked for, and scored the engine zero. On that
evidence the engine looked broken and dropping it looked reasonable.

`engine::without_reasoning` strips the tags, applied by the two engines that cannot
configure the behaviour away - the in-process one and the command-line one. The managed
runtime needs nothing, because its own flag covers it.

**An unclosed block yields nothing rather than the narration.** A reply cut off mid-thought
is all working and no conclusion, and handing that back would be a stub returning success
one level up: a caller cannot tell a model's reasoning from its answer once it is in a
string.

**Stripping them changed no score, and that is worth recording.** Measured before and
after: the in-process engine scored 4 of 12 either way. The proposal loop's reader falls
back to bare tokens, so it was already coping with the markup - the fix matters for a strict
consumer and for not handing anybody a reply with tags in it, and it was never what held
that engine back. Four of twelve was its real score.

**The lesson is about the failure message, not the tags.** "Answered, but not with a list of
words" was true, unhelpful, and cost an engine its place in the ladder. One line of what it
actually said contained the whole diagnosis: engine working, model on topic, tags leaking.
A message that names a shape and withholds the evidence is not much better than no message
(principle 3).

