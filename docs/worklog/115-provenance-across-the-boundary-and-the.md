# Provenance across the boundary, and the demotion that has to stay visible


Read the rest of obSCEne's docs rather than only the protocol - `OUTPUT.md` defines the
record format the corpus is written in, and it grades every result.

**Both projects grade their facts and the vocabularies do not match.** The probe writes
`assumed`, `derived`, `spec`, `documented`, `hardware`; this project writes `published`,
`measured`, `guest-observed`, `assumed`. `orbistoun-probe` now parses `res` records, keeps
the probe's word verbatim, and maps at the point of use rather than on arrival - the
original is what a later reader needs when a mapping turns out to have been too generous.

The mapping is `hardware -> measured`, `spec`/`documented`/`derived -> published`,
`assumed -> assumed`. **With one condition that is the whole point: a `hardware` result is
only `measured` if the hardware was the target.** A value observed on a stand-in is
measured for that part and an approximation for this one, so it is demoted to `assumed`.

Thirteen tests now, all green.

### Surprises

**`derived` should not be downgraded, and the conservative instinct says otherwise.** It
maps to `published` because this project's own definition of `published` explicitly covers
the tree the target C library derives from. Under-reporting a grade is not free: it makes a
fact indistinguishable from a guess, and the entire accounting exists so the two can be told
apart. Being careful in the wrong direction is still being wrong.

**Clippy asked for the mapping to be made worse.** `hardware`-on-a-stand-in and `assumed`
reach the same grade, so `match_same_arms` wanted them merged - but they arrive there for
opposite reasons, one demoted and one never claiming anything. Merging deletes the only
place the demotion is visible, leaving a mapping that reads as though it never downgrades.
Allowed with a reason rather than collapsed.

**The record format documents its own drift, and it was aimed at parsers like this one.**
Four record kinds had gained a field the table did not list and five were missing entirely;
a parser written against the documentation would have been wrong about half the stream. So
an absent provenance is *absent*, never defaulted - a record predating the field claims
nothing, and inventing a grade for it is the same error as recording a value for a call
that died.


