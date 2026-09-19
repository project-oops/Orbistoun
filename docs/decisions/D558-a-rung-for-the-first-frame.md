# D558 - A rung for the first frame, and where frames may not rank

**Status:** measured
**Date:** 2026-09-04

## The question

The reach ladder stopped at `Entered`, and D182 put it there on purpose: *not dying is an
**outcome**, not a distance*. Ranking "survived to the time limit" as a rung had sorted the least
informative run in the corpus to the top of the table, because a guest spinning on four
unimplemented functions survives and one reaching forty-seven imports before faulting does not.

That left every title above `Linked` in one bucket, distinguished only by imports and calls - and
the furthest title in the corpus had done something none of the others had.

## The decision

**Add `Flipped`, and only that.** A guest reaches it by submitting a flip that a real video-out
port accepted.

It does not repeat D182's mistake, and the difference is the whole argument: a flip is accepted
only after the guest has opened an output, set its buffer attributes, registered buffers and
configured the output, each against a real implementation. **There is no way to spin into it.** It
is a specific thing done, where surviving is a thing not happening.

**It does not mean a picture was displayed.** Nothing scans a buffer out here, and a flip
completes the instant it is accepted because there is no vertical blank to wait for - the model
`video_out_submit_flip` already documented. The claim is exactly "the guest reached the layer that
would present it".

## Where frames may not rank

`beats` now orders reach, imports, standing, **frames**, calls.

**Frames sit below imports deliberately.** A guest can sit in its present loop handing over the
same buffer for ever; ranked above imports, that run would sort above one that presented three
times and then got twice as far into the engine - which is D182's failure exactly, one rung
higher. The rung says it presented; the imports still say how far it got.

They sit **above calls** because a frame is something achieved where a call is only something
counted.

## The count is a measurement, not a claim

The promotion is driven by `orbistoun_video::flips_accepted`, which reads the port table -
**not** by counting calls to the submit function in the trace. A submission with a handle this
process never issued is refused, and is still a call to something implemented; a report counting
labels would credit a frame the guest never got.

## What it changed, and what it did not

PPSA02664 now records `flipped`, 197 imports, **1 frame**, honestly measured.

**The guest did not get further.** It still dies at `image+0xf56e09` in `sceAgcCreateShader`, at
the same address, on the same instruction. The flip was already happening - D516 made
`sceVideoOutIsFlipPending` honest and the guest submitted its one frame long before today. What is
new is that the record can *see* it. A ladder that cannot express the furthest thing its furthest
title does is measuring less than it observes, and that is the whole of what this fixes.

## What this does not establish

**That the ordering is right**, only that it is the one D182 argued for. A corpus in which every
title presents would want a different tiebreak within the rung, and nothing here would notice.

**Nor that one frame resembles running.** One flip is the threshold of frame one, not a frame
rate. Whether a further rung is owed to "presented continuously" is left open - it would need a
distance nobody can spin, and a count of repeats is not obviously one.
