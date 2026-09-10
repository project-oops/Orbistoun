# 486. The differential was comparing two sandboxes

**2026-09-10** - directed, continuing 485

## The pairing was wrong

485's conformance differential ran obSCEne under orbistoun and diffed it against
`20260909-232735-payload.obs.log`. Orbistoun was running `PPSA99980` - a **native title**. The
payload leg is a different sandbox with different resolvable symbols, so the comparison was
between two unlike things, and most of the 119 `orb=pass / hw=skip` rows are that mistake rather
than a finding.

The correct pairing is by mode: a native-title run against the eboot leg, a compatibility run
against the pkg leg, a payload run against the payload leg. The newest eboot sweep on disk is
zero bytes, so the right comparison has not been made yet - which is worth saying plainly, because
the eleven divergences 485 named were drawn from the wrong pair and are not established.

## Category is a permission, and orbistoun had none

Execution is governed by two orthogonal axes - a privilege tier in the container header, and an
`applicationCategoryType` a title declares in its own `param.json`. The category decides direct
memory, scanout ownership and whether a title runs alongside others.

Orbistoun modelled neither; searching the tree finds only the GUI's shell categories. Every title
ran as the most privileged kind. All three titles with a `param.json` declare `0`, so nothing has
been visibly wrong yet - it has been unconstrained by accident rather than by decision.

`orbistoun_core::category` now models the five documented kinds from SELFish's
`docs/features/writing.md`, at `published` grade because no probe has confirmed any of it (D662).

## Surprises

- **The rebrand is smaller than I claimed.** I warned that Prospero / Trinity / Neo was a
  three-way split against a two-way `Generation` enum and therefore a design change. It is not:
  `Generation { Ps4, Ps5 } x Revision { Base, Pro }` already expresses four machines, and the
  work is labelling. The earlier warning was wrong and the question it prompted was avoidable.
- **`Unknown` had to be its own variant.** Mapping an unrecognised category onto the default
  would grant it everything a big app gets on the strength of a number nobody recognises - the
  permissive direction, one level below the one the differential just found six instances of.

## Next

- A non-empty eboot sweep, then the differential re-run against the matching leg.
- Enforcement, once a title exists that would exercise it: nothing in the corpus declares anything
  but `0`, and a guard nothing runs is a guard nobody knows anything about.
