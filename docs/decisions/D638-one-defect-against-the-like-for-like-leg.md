# D638 - One defect, against the leg that compares like for like

**Status:** measured
**Date:** 2026-09-09

## The eboot was never stuck

`obscene` sits in the frontier as *"ran to the time limit"*, which reads as a guest that hung. It
finishes:

```text
OBS|tally|521|9|6|19
OBS|end|sceKernelWrite
```

**521 passed, 9 failed, 6 partial, 19 skipped, and the suite ended.** It runs to the limit
afterwards because `obs_screen_present` is doing what its own comment says - *"With a display, stay
and show it… Without a display this returns at once"* - and orbistoun gives it a display. The
payload exits instead because it has none.

So the most complete run in the corpus was filed under an outcome that reads as a failure.

## Which hardware leg you compare against changes the answer by two orders of magnitude

Every differential so far has been the local **payload** against the console's **pkg** leg. The
eboot has its own hardware leg, and had never been diffed against it:

| local | against | passed there, failed here |
|---|---|---|
| payload | pkg | **106** |
| eboot | eboot | **1** |

The one is `900-surface/control` - the weak symbol that should be null and is bound (D633).

D622 recorded the same asymmetry pointing the other way and drew the right conclusion for the
payload: *"the comparison that means something is against the console's pkg run, which is the one
with full access"*. This quantifies it for the eboot: **370 checks go fail-to-pass**, orbistoun
passing what the console's sandboxed eboot could not attempt. That is a difference in *reach*, not
in correctness, and reading it as orbistoun being better would be the same mistake in reverse.

What the like-for-like leg buys is the other direction: among checks both ran and concluded
differently, exactly **one** is orbistoun failing where the console passed.

## And four of the six failures are the console's failures too

The most complete run fails six checks. Asked what the console's eboot leg did with the same five
that are comparable:

| check | orbistoun | console |
|---|---|---|
| `010-kernel/is-stack` | fail | **fail** |
| `018-relational/handle-fits-its-out-parameter` | fail | **fail** |
| `110-modules/info-size` | fail | **fail** |
| `110-modules/names` | fail | **fail** |
| `137-kernelcall/system-version` | fail | skip |

Four are the probe disagreeing with the platform, not with this emulator.

## The one I nearly "fixed"

`010-kernel/is-stack` fails with *a stack address and a static one were reported alike*, and
`is_stack` does indeed ignore its first argument. An obvious defect, an obvious fix, and both
wrong: this function's own doc comment already records that **the console answers `0` to a local
and to a static**, across twenty-three runs, and that the answer is the bounds rather than the
return.

Making the two differ would have invented a distinction the platform does not make, to turn a
check green. That is D227 exactly - *an intervention that moves a wall is not a diagnosis* - and
what stopped it was reading the thirty lines above the function before changing it.

The remaining `skip -> fail` is honest: the console **could not build a syscall gadget**, so it
never asked; orbistoun has one, asked, and its dispatcher refused the number with `-ENOSYS`. A
refusal to a question the other side never put is not a disagreement.
