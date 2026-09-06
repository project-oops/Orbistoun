# 2026-09-03 - (/loop) The encoder measurements say the opposite, and thirty names go back

```
declared          947  ->  917
names-only        273  ->  243 across 29 libraries
```

Asked to check whether the thirty-three encoder symbol measurements are claimable now that
`libSceVencCore` and `libSceVideoRecording` are declared. **No - and the premise was inverted,
mine included.**

## `unresolved = 0x0` means it did not resolve

```c
if (addr != NULL) {
    obs_report_measure("106-encoder/symbols", name, "vaddr",  …);
    obs_report_measure("106-encoder/symbols", name, "handle", …);
} else {
    obs_report_measure("106-encoder/symbols", name, "unresolved", 0, "status");
}
```

The record is emitted in the **null branch**, after the lookup has tried the kernel export
table, every loaded handle and `0x2001`. The `0` is filler for a status field. I had read it as
"zero unresolved" and told the user every one of those symbols resolves on the console.

Two independent confirmations: **there is not one `vaddr` or `handle` measurement in the whole
encoder group** - every probed symbol failed - and obSCEne's `related-libs` check records six of
those libraries as *absent*. The reasons for the **libraries** in this file were already
correct; only the reasons for the **symbols inside them** were backwards.

## So thirty declared names had no provenance and are withdrawn

D505 set the standard in its own text - every name *"read out of a real module's import table"*
or measured resolving. Against it: **no guest in the corpus imports or calls `libSceVencCore`**,
and nothing resolved, so its 24 names rested on obSCEne's candidate list alone. Six of
`libSceVideoRecording`'s ten were the same. Withdrawn. The four the corpus shows modules calling
stay.

That is a bad declaration committed inside the decision that exists to prevent bad declarations.
A name from a candidate list puts a NID in the table no import will ever match and inflates a
surface figure with functions nothing has been seen to want.

## The thirty-three reasons are rewritten

The old text - *"whether a symbol resolves inside a video-encoder library"* - was not vague, it
was backwards, and it would have justified a claim in the wrong direction. They now say what
they record: a **non-resolution under one capture's application category**, the same shape as
the sysmodule refusals (D503), unblocked by the category-0 capture already in obSCEne's
backlog 022.

## The cheap check to add

Rule 6 is *read the probe before asserting against its numbers*. What it needs is the direction
test: **a field named `unresolved`/`absent`/`refused` is emitted by a failure branch, so its
presence is the finding and its value is filler.** Reading `0x0` as a count rather than as a
marker is D497's mistake one field name further out.

## State

`cargo test --workspace` green - 119 suites, **1994 tests**, 0 failures. clippy `--tests` clean,
fmt clean, identity scan clean on both.

Nothing committed. The day holds worklogs 292-355 and D466-D506.

**Next**: the differential's uncovered functions (`strtok_r`, `strtof`, `sprintf`, `vsnprintf`),
and the two singletons still untried - `120-measure/timer-ratio:tsc_hz_calibrated` and
`035-libc/fpu-environment`.
