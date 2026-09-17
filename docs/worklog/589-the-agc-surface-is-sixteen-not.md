# 589. The AGC surface is sixteen builders, not nothing

**2026-09-15** - a module document that undersold its own code, and the guard that stops it
happening again

## What was wrong

`crates/orbistoun-gpu/src/agc.rs` opened with:

> Nothing here is implemented. That is deliberate: `docs/ROADMAP.md` puts the first frame at
> Phase 6, which has not begun...

Sixteen handlers answer through `implementations()`. The paragraph was true when it was written
and nobody deleted it as the builders landed one at a time.

**It was believed.** A gap analysis drafted against this tree read it at its word and recorded the
graphics surface as emptier than it is, listing the staleness under its own "stale facts found
while reading" heading. That is the cost: not that a comment was wrong, but that a document
written *from* the comment inherited the error, and a reader of the second document has no way
back to the first.

A document claiming **less** than the code does is the mirror of the failure principle 3 names,
and it misleads in exactly the same way - by being trusted.

## What replaced it

The count, what the sixteen are, and why the remainder are refusals rather than omissions:
`sceAgcDcbDrawIndex` has a known size and two unknown body dwords, `sceAgcDcbSetIndexSize` was
measured at one input, and a builder guessed into this file would be amended by the
`sceAgc*Patch*` family before anyone could read it back (worklog 553).

## The guard, and the number it corrected

`the_wired_set_is_the_size_the_module_documentation_claims` asserts
`implementations().len() == 16`. Wiring or removing a builder now fails a test whose message names
the document to update.

**It corrected itself on its first run.** The number I wrote was fifteen, from
`grep -c` over the handler pattern. The test said sixteen. `rustfmt` had wrapped
`sceAgcCbSetShRegisterRangeDirect` onto its own line, so the grep pattern missed it - the same
wrapping that has twice made a registered handler *look* registered when it was dead.

The declared count was wrong the same way: my grep said 55, the macro block holds 57, because two
declarations wrap too. The gap analysis had 57 and 16 right and both of my greps were wrong.

**Counting the built slice is the only count that cannot be fooled by the layout**, which is why
the test reads `implementations()` and not the source text. A grep over source is a measurement of
formatting.

## Checked

`agc_driver.rs` carries no equivalent claim; its 5-of-13 is not stated in prose, so there is
nothing there to drift.

## Surprise

**The stale sentence had a date stamp on it in the form of its own reasoning.** It justified
itself with "Phase 6 has not begun" - a claim about the roadmap, checkable in seconds, and false
by the time sixteen measured encoders were sitting underneath it. A document that explains *why*
it says something leaves a thread to pull; this one had the thread and nobody pulled it for long
enough that a second document copied the conclusion.
