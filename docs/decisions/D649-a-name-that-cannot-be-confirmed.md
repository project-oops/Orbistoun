# D649 - A name that cannot be confirmed

**Status:** published
**Date:** 2026-09-09

## The first unnameable hash that got an answer

`libSceAgc::0x53bbd82b51d172db` is imported by PPSA02664, PPSA03416 and PPSA28061, and 30,184
names plus 3.9 billion generated candidates never explained it. SELFish answered
`REQ-20260909T1250Z-1f74`: five independent published identifier tables attribute it to
**`sceAgcInit`**, and

```text
Nid::of("sceAgcInit") = 0x916dc62dbed07cf8   ≠   0x53bbd82b51d172db
```

The same tables list *both* identifiers against that one name. The name does not hash to the value
a title actually imports, so **no name search can ever reach it**. That is why the search failed,
and the failure was not a gap in the vocabulary.

It is systematic rather than a typo: of 116 published `libSceAgc` identifiers, 108 reproduce from
their names and 8 do not, five of those being a second identifier on a name whose first one
reproduces. Decorations, the empty suffix, and both byte orders against 837,352 words were all
tried and ruled out before the answer came back.

## Why it does not go in the symbol database

Every `StaticSource` variant in `orbistoun-nid` ends the same way - *and then the hash confirms
it, as every name here must be confirmed*. That is the property the whole database rests on:
nothing in it can be wrong in a way that produces a false name, because a wrong name simply does
not hash. `sceAgcInit` against this identifier cannot satisfy that, and adding it would replace a
structural guarantee with a policy.

`crates/orbistoun-names/data/vendor.toml` is no better a home for the same reason from the other
direction: it is a vocabulary for brute force, and a word that does not hash to the target
contributes nothing to a search.

## So it is recorded as knowledge, at `known_by: published`

The knowledge files already have the vocabulary for exactly this: a fact whose provenance is a
document rather than a derivation or a console. The entry carries the attribution, its five
sources, the hash that *does* reproduce, and the explicit statement that this project cannot
confirm it the way it confirms every other name.

**The ceiling is stated in the entry rather than left to the reader.** `published` is the second
strongest grade this project has, and it would be easy to read an entry at that grade as settled.
This one is settled about *what the tables say* and about nothing else.

## What it is worth

Three titles stall inside this library's initialisation, which corroborates the attribution
semantically without confirming it - and that corroboration is itself only suggestive, since a
first call is where anything stalls. What the entry actually buys is that a reader meeting the
bare hash now knows what it is believed to be, and knows not to extend a word list to find it.

The systematic finding is worth more than the single name: `libSceAgc` carries a class of alias
identifiers, so the next unnameable hash in this library should be checked against that pattern
before anyone spends another 3.9 billion candidates on it.
