# D671 - The pad structure was measured, so the shim D345 deferred could be written

**Status:** decided
**Date:** 2026-09-10

## Comparing against the right leg, third time

D669 fixed orbistoun reporting itself a payload, and the conformance comparison moved to
obSCEne's **package** leg. That was still wrong by one axis: the package leg is
`title/ps4-bc` - previous generation, `libSceGnm` mapped, `libSceAgc` absent - and orbistoun
presents `prospero`. Two of the divergences it produced were `165-gnm/dispatch-*`, which
orbistoun should differ on.

**No hardware leg anywhere reports `ps5-native`.** Eighty-eight logs, every one
`payload/unknown-gpu`, `title/unknown-gpu` or `title/ps4-bc`. The closest match to orbistoun is
a populated **eboot** leg, `title/unknown-gpu`, which is what orbistoun reports today - so that
is the comparison from here. Against it: 167 hardware passes, 170 orbistoun, ten checks where
hardware passes and orbistoun does not, and the generation artefacts gone.

Four of the ten were input, which is where this went.

## D345 deferred it for the right reason, and the reason expired

`scePadReadState` was declared and unimplemented. D345 says why: the transport carrying pad
state from the window to the guest was ours and testable, but the *structure a guest reads* was
"a size and layout nobody here has measured", and building a shim around a guessed encoding is
how a confident wrong answer gets made. It built the transport and left the shim, deliberately.

obSCEne has since measured it. `100-input/read-extent` fills the destination with a sentinel,
calls the function, and reports how far the change reached:

```
extent 120, changed 120
 0: 00000000 80808080 00000000 00000000
16: 00000000 00000000 0000803f 00000000
32: 0000803f 00000000 00000000 00000000
48..120: zero
```

`100-input/batched-read` reports the identical extent and contents for `scePadRead`.

## Bytes, not a structure

The shim writes those 120 bytes verbatim. It does not declare a `struct`, and that is the whole
of the decision.

What was measured is *the byte image a console produced for a pad at rest*. Which offset
carries the buttons and which the sticks is an **inference** from it - the obvious reading is a
mask in the first four bytes, four axes at `0x80` centre in the next four, and `1.0f` at 24 and
32 - and a shim that laid out fields would publish that inference in the place the measurement
belongs. Nothing needs the reading to answer this call correctly.

`changed 120` is why the whole extent is written every time. A shim writing only the fields it
believed it understood would leave the rest of the guest's buffer holding whatever was there
before, which is precisely what that check exists to catch.

## What it cannot do, said out loud

**It cannot report input.** Mapping orbistoun's live pad state onto these bytes needs the field
offsets, and those are exactly the inferred part - so `latest`, the transport D345 built, stays
unconsumed. A guest polling the pad under orbistoun sees a pad at rest, forever.

That is a limit rather than a gap to paper over, and obSCEne already names the run that would
close it. Under orbistoun the two checks that would settle it now answer:

```
100-input/button-bits        pending  controller attached, but no button was seen in the window
100-input/stick-trigger-range pending  controller read, but nothing moved: sweep the sticks and re-run
```

Those are the right questions asked of the wrong machine - orbistoun cannot answer them because
it is the thing that does not know. On hardware they settle it in one run, and that is filed.

## What it bought

Three checks flipped: `100-input/read-extent` and `100-input/batched-read` from `partial` to
**pass 0x78**, and `100-input/oops-sdk-poll` from **fail** to **pass 0x0**. Input divergence
against the matching leg went from four to one.

The count of "hardware passes, orbistoun does not" stayed at ten, and the composition is the
interesting part: audio went from two to five. Those checks were reading `partial` on the old
positive placeholder and read `fail` on the honest one (D670) - they were passing partially on
a refusal the guest could not see. Audio is the next cluster, and it is only visible now.
