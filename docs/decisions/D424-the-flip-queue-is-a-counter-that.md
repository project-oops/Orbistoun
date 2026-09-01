# D424 - The flip queue is a counter that completes on submit, and it unblocked the whole suite


**measured** - 2026-08-31

With the sandbox and the payload shape sorted, the current obSCEne ran to 29 of 38 sections and then
faulted in the video section: `sceVideoOutSubmitFlip`, `sceVideoOutSetFlipRate` and
`sceVideoOutGetFlipStatus` were declared but not in `video::implementations()`, so a flip was
submitted, its completion polled, and the placeholder read into a fault - the classic "renders one
frame then freezes" the crate's own header warns about.

The model is the honest one for a headless run: a flip **completes the instant it is submitted**.
There is no scanout and no vertical blank here, so nothing picks a queued flip up later; the port's
completed-flip count advances on submit, and `GetFlipStatus` writes that count at offset 0 - the one
field with a citable position (obSCEne assembles a `uint64` from `status[0..8]`, and the count is the
documented head of `SceVideoOutFlipStatus`). The rest of the structure is left unwritten: no lawful
source here gives its offsets, and a guessed layout is the invention principle 3 forbids. A guest
polling for the count to move past what it was sees it move, and proceeds.

What it bought, measured on the spot: the run went from 158 unique tests over 29 sections to the
**whole 543 over all 38, reaching `OBS|end`** - the first complete obSCEne run inside orbistoun. That
is the thing the stale corpus (D419), the sandbox (D422/D423) and the payload shape were all in the
way of: a full suite that a hardware report can now be diffed against test-for-test. Under the
default stub-everything resolver orbistoun answers 515 pass / 10 partial / 4 fail / 14 skip; those 4
fails and 10 partials are the real fidelity gaps, now legible because the run finishes.

`SubmitFlip` advancing the count immediately is the assumption most worth flagging: a title that
submits many flips fast and reads a *pending* count (a field this does not model) would see a
different shape. Nothing measured does yet; when one does, the pending count is where this grows.

