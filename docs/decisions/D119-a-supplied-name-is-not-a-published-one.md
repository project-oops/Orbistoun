# D119 - A supplied name is not a published one

**decided** · 2026-08-19 · found while designing the probe pipeline

`--words` names were merged into the published-standard list and the whole batch recorded
as `published-standard`, citing a file they had never been in.

It failed safe - an audit checks membership in that file, does not find them, and reports
them unaccounted - but it reported the *wrong problem*, and the sharper issue was this:
**the `Supplied` variant was unreachable.** D073 defined the category whose entire job is
labelling outside material, and no code path could produce one. The mechanism for being
honest about imported names did not work.

The two lists are now searched separately, because they are different kinds of claim, and
`--words-from` says which:

- `observed` - learned by running something we wrote. Ours, but not reproducible by
  re-running an index, so an audit lists it separately rather than counting it verified.
- `supplied` - came from outside. Never verifies. **The default**, because assuming the
  more generous label is exactly the mistake an audit exists to prevent.

> D213 replaced the first of those with `probe`, which says what the list actually is: names
> our own conformance probe reported, on hardware. Same distinction, named after its source
> rather than after the fact that something ran.

