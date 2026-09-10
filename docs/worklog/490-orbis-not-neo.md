# 490. Orbis, not neo

**2026-09-10** - directed, continuing 489

Four inbox items serviced, all from the umbrella thread or oops-libs.

## The manifest said `neo` and meant `orbis`

The target axis has four values and they are not interchangeable: `orbis` and `prospero` are the
base machines, `neo` and `trinity` the mid-generation refreshes. obSCEne's probe is not
Pro-specific, so its previous-generation artifacts are `orbis`.

**D663 got this right the same day the manifest got it wrong.** The `Platform` enum derives
exactly those four from `Generation × Revision`; the manifest copied `neo` from the sketch it was
written to. A vocabulary settled in code does not settle itself in data (D666).

Renamed before obSCEne publishes, which is free because of the origin list: every entry names a
sibling checkout first and the release second, so a URL that does not exist yet costs nothing
while the local path answers. Sequencing recorded: orbistoun renamed first.

## Two more from the same audit

**`target=` is a third sense of the word.** Prosperous has a *registered target*, SELFish a *build
target*, this manifest an *install target*. A manifest entry holds two at once - the file it
fetches is named for a build target while the field says where it lands - so the field keeps its
name and the file says which sense it is.

**Generation-4/5 wording swept** from the three tracked files still carrying it. Only the wording:
`data/hardware/ps5-full.txt` is a real obSCEne path and `shadPS4` is somebody's project, and
renaming either would have been a different kind of wrong.

## The delegation, finally

oops-libs shipped the two-argument `enable_portable_sentinel`, so orbistoun's copy is gone. The
note stays here and is passed in - its words name this tool, so a shared function hardcoding them
would be wrong for every other caller. Their crate now owns the heal orbistoun had and they
lacked.

## Surprises

- **A filename cannot carry a compatibility environment.** obSCEne measures it as `OBS|context`,
  and the same physical machine answers differently depending on where a run lands. That lands
  squarely on the conformance differential: pairing a run against a sweep by *filename* is the
  mistake I already made once, and pairing by measured context is the fix.

## Next

- Pair the differential by measured `OBS|context` rather than by leg filename.
- Audit findings C through E, still open.
- The corpus fetch path, still not walking the origin list.
