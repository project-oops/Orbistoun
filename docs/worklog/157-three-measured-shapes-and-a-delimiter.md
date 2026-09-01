# Three measured shapes, and a delimiter that could not spell a library


**The three cheapest missing shapes were added** - `prefix-learned-verb`,
`prefix-module-learned-learned-verb`, `prefix-learned-verb-learned-learned` - chosen by
`tests/shapes.rs` rather than by intuition. None takes `tail`, and that is the whole reason
they are affordable: `tail` holds the empty string, so a shape with it is a strict superset
of the same shape without, and including it would have cost +225% on the candidate space
instead of +15%. Measured after: 13 patterns, 4,363,170,660 candidates, exactly the 15%
predicted.

**And the diagnostic delimiter was fixed** (D263). `ORBISTOUN_WRITE` split its clause left
to right, so `libkernel::sceFoo:1:0x1100` was five fields where three were expected and the
whole clause was dropped - the bug behind two hundred and seventy-six runs that planted
nothing. Read from the right instead, since the trailing fields are fixed. An existing
rejection test immediately caught the first version being too permissive, which is the
argument for writing them.

### Outcome of the three shapes

Measured after the search ran. **+5 names** (782 -> 787), all in the predicted shapes:
`sceRudpRead`, `sceRudpTerminate`, `sceRudpWrite` from `prefix-learned-verb`, and
`sceAudioPropagationPortalDestroy`, `sceAudioPropagationSystemDestroy` from
`prefix-module-learned-learned-verb`. The third shape found nothing.

Worth noting *which* names: not the ones that motivated the shapes. The measurement pointed
at `sceRudpBind/End/Init` and the search returned `Read/Terminate/Write`. A shape generalises
past the evidence for it, which is the entire argument for choosing shapes by measurement
rather than by adding the specific names one wanted.

Reachability over the same 183-name sample moved **28 -> 43**, so ten already-known names
became spellable as well as the five newly found.

### Both halves of the question, in one tool

`tests/shapes.rs` now also reports the fragment each unsplittable name stalls on, ranked -
so it answers "which shape" and "which word" together. It immediately paid: twelve
`sceKernelApr*` names were all stalling in the same place for want of `Apr`, a word the
morning's `learned` reduction had dropped along with `Batch` and `Mspace`. The re-harvest
during `names` brought it back. What remains is `GpuMaskIdCommand` (3 names), `URL` (2), and
a tail of one-offs.

### And one of the three came straight back out

`prefix-learned-verb-learned-learned` was the largest group the measurement found, and it
lasted an afternoon. It found no names, and it cost **67% of every vocabulary round** -
because a round re-sweeps each pattern that uses the slot it grew, at full size, and that
shape takes `learned` three times.

The ranking that put it third-cheapest was measuring its share of the *whole* sweep, which is
a different quantity and ranks the opposite way. Caught by the narrowing test, which asserts
a round stays under a tenth of the space and was seeing 13.7% (D264).

Left in place: 12 patterns, 3,963,913,884 candidates, and the two shapes that between them
found five names.

