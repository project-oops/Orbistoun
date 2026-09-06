# D506 - The encoder measurements record a non-resolution, and thirty names had no provenance

**measured** - 2026-09-03 (reading the probe, one day after asserting against it)

The question was whether declaring `libSceVencCore` and `libSceVideoRecording` (D505) made the
thirty-three `106-encoder` symbol measurements claimable. It does not, because **the premise
was inverted**: those records say the symbols did *not* resolve.

## What the probe actually does

```c
if (addr != NULL) {
    obs_report_measure("106-encoder/symbols", name, "vaddr",  …);
    obs_report_measure("106-encoder/symbols", name, "handle", …);
} else {
    obs_report_measure("106-encoder/symbols", name, "unresolved", 0, "status");
}
```

The `unresolved` record is emitted in the **null branch**, after the lookup has tried the kernel
export table, every loaded module handle, and `0x2001`. The `0` beside it is filler for a status
field, not an address.

Two things confirm it beyond the source:

- **There is not one `vaddr` or `handle` measurement in the entire encoder group.** Every symbol
  it probed failed. Had any resolved, its address would be recorded.
- obSCEne's own `related-libs` check separately records `libSceVideoRecording`,
  `libSceAvcEnc`, `libSceHevcEnc`, `libSceVideodec`,
  `libSceMediaFrameworkInterface` and `libSceVideoCoreServerInterface` as **absent** - and those
  entries in this file already said so correctly. The reasons for the *libraries* were right
  while the reasons for the *symbols inside them* were the opposite.

## So the thirty-three are not claimable, and the reason has been rewritten

They record a **non-resolution under one capture's application category** - the same shape as
the sysmodule refusals (D503), and matching it would pin orbistoun to that process's loaded set.
The category-0 capture already asked for in obSCEne's backlog 022 is what would say whether
these libraries are absent to every title or only to that one.

The old reason - *"whether a symbol resolves inside a video-encoder library"* - was not merely
vague, it was backwards, and it would have justified a claim in the wrong direction.

## Thirty declared names are withdrawn

D505 set the standard in its own text: every declared name is *"read out of a real module's
import table"* or measured resolving on hardware. Against that standard:

| | provenance | verdict |
|---|---|---|
| `libSceVencCore`, 24 names | obSCEne's candidate list only - **no guest in the corpus imports or calls it**, and nothing resolved | **withdrawn** |
| `libSceVideoRecording`, 6 of 10 | same | **withdrawn** |
| `libSceVideoRecording`, 4 of 10 | modules in the recorded corpus are seen calling them | kept |

```text
declared   947 -> 917        names-only   273 -> 243 across 29 libraries
```

A name from a candidate list is the one thing a declaration here is not allowed to be. It puts a
NID in the table that no import will ever match, and it inflates a surface figure with
functions no guest has been seen to want - which is the failure D505 exists to have fixed,
committed inside D505 itself.

## The check that would have caught it, stated so it is cheap next time

Rule 6 in this project's working notes is *read the probe before asserting against its numbers*,
and it settled two things the day before this. What it needs adding is the direction test: **an
`unresolved`/`absent`/`refused` field name is emitted by a failure branch, so its presence is
the finding and its value is filler.** Reading `0x0` as "zero unresolved" instead of "this one
did not resolve" is the same mistake as reading a probe's own initialiser as a measurement
(D497), one field name further out.
