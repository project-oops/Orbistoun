# Symbol databases and the NID hash

guest modules do not import by name. A guest module's dynamic table
references a library plus a **NID**: a 64-bit hash derived from the symbol name.
Resolving imports therefore needs both halves of that relationship.

## The algorithm

1. Append a fixed byte suffix to the symbol name.
2. SHA-1 the result.
3. Take the first eight bytes, little-endian, as a `u64`.

Implemented in `orbistoun-nid::NidHasher`. A hash is not invertible, so the reverse
direction (NID to name) is pure lookup: hash every name you know and build a map.
That is `SymbolDb`.

## Why the suffix is not in the source

The suffix is a publicly documented constant from hardware reverse-engineering
work. It is deliberately **not** compiled in. Two reasons:

- **It keeps a magic constant out of the tree.** A bare hex blob in source with no
  derivation is exactly the kind of artefact that makes provenance questions hard to
  answer (see [CLAUDE.md](../CLAUDE.md) principle 1). As runtime data it is an
  input, like a ROM path.
- **It makes the hasher testable.** `NidHasher` is exercised against arbitrary
  suffixes in unit tests, with no dependency on the real value being present or
  correct.

The practical consequence: `orbistoun symbols` without `--suffix-hex` prints correct
*names* and meaningless *hashes*, and warns that it is doing so. Import resolution
against a real module needs the real suffix.

## File format

One file carries both halves, so a single input fully determines resolution
behaviour and cannot disagree with itself:

```json
{
  "suffix_hex": "00112233445566778899aabbccddeeff",
  "names": [
    "sceAudioOutInit",
    "sceAudioOutOpen",
    "sceKernelAllocateDirectMemory"
  ]
}
```

NIDs are **derived, never stored**. A file listing both names and hashes could
carry a pair that does not actually hash to each other, and that inconsistency
would surface as a mystery unresolved import much later.

Deserialised as `orbistoun_nid::SymbolDbFile`.

## Obtaining one

**Generate it, with `orbistoun names`.** The reverse direction is a search, and this
repository does the search itself rather than consulting anything (D068):

```bash
orbistoun-cli names --suffix-hex <HEX> --out symbols.json path/to/guest
orbistoun-cli names --suffix-hex <HEX> --out symbols.json titles   # or a whole corpus
```

**A directory is one search, not one per module.** Every module beneath it is read, their
unnamed imports are unioned, and a single sweep answers all of them - a wider target set
costs a sweep nothing, because each candidate is one hash-set lookup whatever the set
holds. It is also the only form that finds a name lying in one title's data that explains
a *different* title's import, which is where a large share of them are (D213).

Candidates come from four places, and only one is guesswork:

- **Published standard-library names.** The target C library is FreeBSD-derived, so much
  of it is ISO C and POSIX under the names those standards publish. 3,018 ship in
  `crates/orbistoun-names/data/standard.txt`. These are not guesses.
- **A module's own bytes.** Diagnostic and assertion text leaves real function names in a
  binary. Not a guess about the vendor's naming - it is the vendor's naming (D193).
- **The rest of the corpus.** The same mechanism, pooled, so the vendor C library module's
  strings name imports of titles that never mention them.
- **Generated vendor names.** The convention is strict - prefix, module, action, object,
  revision mark - so candidates are enumerated from a grammar in
  `crates/orbistoun-names/data/vendor.toml`. Billions of them, searched at around 30
  million per second, and the count grows every time a confirmed name teaches it a word.

A match is proof: the hash agrees or it does not, whichever of those proposed the
candidate. A miss proves only that the name was not among those tried, so **extending the
vocabulary is the method** - and because it is data, that costs no rebuild.

Every name is stored with a record of which of those proposed it, what kind of material it
came out of, and what somebody else would need to arrive at it again.
[PROVENANCE.md](PROVENANCE.md) is that vocabulary and how it is checked.

Public NID-to-name databases do exist, and any file matching the shape above will load.
Nothing here depends on one.

Names that no database knows stay unknown, and that is a normal, reportable state -
`orbistoun imports` prints `<unknown>` and counts it. An unknown NID means "a
function we have no name for yet", which is still useful: you know the title needs
it, how many times it is called, and from where.

## What a name does not give you

A resolved name tells you what a function is *called*. It says nothing about what it
does, what it returns, or what it expects. That is the actual hard problem, and
[TESTING.md](TESTING.md) covers the four places real answers come from.
