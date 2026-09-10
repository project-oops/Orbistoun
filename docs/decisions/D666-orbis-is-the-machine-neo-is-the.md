# D666 - Orbis is the machine, neo is the refresh

**Status:** decided
**Date:** 2026-09-10

## The manifest said `neo` and meant `orbis`

The download manifest named obSCEne's previous-generation artifacts `obscene-probe-neo.{zip,pkg}`,
copied from the sketch it was written to. The umbrella thread corrected it: the target axis has
**four** values and they are not interchangeable - `orbis` and `prospero` are the base machines,
`neo` and `trinity` the mid-generation refreshes. oops-sdk owns the axis in
`include/oops/target.h`.

An artifact for *any* previous-generation machine is `orbis`. `neo` is right only for a genuinely
Pro-specific build, and the probe is not one.

**D663 got this right and the manifest did not**, which is worth recording as a pair: the
`Platform` enum derives exactly these four from `Generation × Revision`, and was written the same
day the manifest was written wrong. A vocabulary settled in code does not settle itself in data.

## Renamed before the release exists, deliberately

obSCEne has an open request to publish under the new asset names, so for now both spellings 404.
That is safe here **because of the origin list**: every entry names a sibling checkout first and
the release second, so a rename to a URL that does not exist yet costs nothing while the local
path answers. The fallback built in D664 is what makes the rename order free to choose.

Recorded so the sequencing is on the record: orbistoun renamed first.

## A third sense of `target`, qualified rather than renamed

Three live meanings in the collection - Prosperous's *registered target* (a machine it can reach),
SELFish's *build target* (the machine an artifact is built for), and this manifest's *install
target* (which root an artifact lands in). All three are legitimate.

A manifest entry has two of them in play at once: the file it fetches is named for a build target
while the field says where it lands. So the field keeps its name and the file says which sense it
is, next to the entries where both appear.

## What a filename cannot carry

The same correction notes that a compatibility environment is **measured, never chosen** -
obSCEne owns it as `OBS|context`, and the same physical current-generation machine answers
differently depending on the environment a run lands in. So a filename cannot encode one, and
nothing here should infer an environment from an artifact name. Written down because the
conformance differential is exactly where that inference would be tempting: pairing a run against
a sweep by *filename* is the mistake, and pairing by measured context is the fix.
