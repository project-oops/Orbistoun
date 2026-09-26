# Symbol databases and the NID hash

Guest modules do not import by name. A guest module's dynamic table references a library plus
a **NID**: a 64-bit hash derived from the symbol name. Resolving imports needs both directions
of that relationship.

## The algorithm

1. Append a fixed 16-byte suffix to the symbol name.
2. Take the SHA-1 digest of the result.
3. Take the first eight bytes of the digest as a `u64`, with `digest[0]` as the most
   significant byte.

`orbistoun_nid::NidHasher` implements it, and a round-trip test against the encoded form in an
import table pins the byte order. A hash is not invertible, so the reverse direction (NID to
name) is lookup: hash every known name and build a map. That is `orbistoun_nid::SymbolDb`.

## The hash suffix

The suffix ships in selfish's `data/hash-suffix.toml`, embedded in the binary by `selfish-nid`
at build time. It is a salt on a name-mangling hash, not a key: it decrypts, signs and protects
nothing. It lives in a labelled data file rather than a Rust literal so that its origin and
verification are written next to it (see [CLAUDE.md](../CLAUDE.md) principle 1).

The value verifies itself. The target C library is FreeBSD-derived and exports ISO C and POSIX
functions under their published names, so hashing those names with the right suffix matches
real imports in a real module, and a wrong suffix or byte order matches none.

`--suffix-hex <HEX>` overrides the shipped value for any command. `NidHasher` takes the suffix
as a parameter, so its unit tests run against arbitrary suffixes.

## File format

One file carries the suffix and the names, so a single input determines resolution and cannot
disagree with itself:

```json
{
  "suffix_hex": "00112233445566778899aabbccddeeff",
  "names": [
    "sceAudioOutInit",
    "sceAudioOutOpen",
    "sceKernelAllocateDirectMemory"
  ],
  "derivations": {}
}
```

NIDs are derived, never stored. A file listing both names and hashes could carry a pair that
does not hash to each other, which would surface later as an unexplained unresolved import.

`derivations` is optional. It records, per name, how the name was arrived at, so anyone can
re-derive it; a name without an entry is unaccounted for, which the audit reports.
[PROVENANCE.md](PROVENANCE.md) describes that vocabulary and how it is checked.

The file deserialises as `orbistoun_nid::SymbolDbFile`. Any file of this shape loads through
`--symbols-db <path>`.

## The shipped database

`symbols/generated.json` is embedded in the binary and loaded unless `--symbols-db` names
another file. `./bin/orbistoun names` regenerates it from the modules in the title library,
and `./bin/orbistoun symbols-audit` checks that every committed name re-derives from this
repository.

## Generating names

`orbistoun-cli names` searches for names; the repository derives them itself rather than
consulting an external database (D068):

```bash
orbistoun-cli names --out symbols.json path/to/guest
orbistoun-cli names --out symbols.json path/to/titles    # a whole corpus
```

A directory is one search. Every module beneath it is read, their unnamed imports are
unioned, and a single sweep answers all of them. Each candidate costs one hash-set lookup
whatever the target set holds, and a name found in one title's data can explain a different
title's import (D213).

Candidates come from four sources:

- **Published standard-library names.** ISO C and POSIX names, in
  `crates/orbistoun-names/data/standard.txt`.
- **A module's own bytes.** Diagnostic and assertion text in a binary carries real function
  names (D242).
- **The rest of the corpus.** The same mechanism, pooled across every module searched.
- **Generated vendor names.** Vendor names follow a strict convention (prefix, module,
  action, object, revision mark), so candidates are enumerated from the grammar in
  `crates/orbistoun-names/data/vendor.toml`. Confirmed names teach it new words.

Other options: `--words <file>` adds verbatim candidates (`--words-from` records their
source), `--grammar <file>` replaces the built-in vocabulary, `--wanted <file>` writes the
hashes still unnamed as a work list, and `--from-trace` reads candidates from guest memory a
previous run captured.

A match is proof: the hash agrees or it does not, whichever source proposed the candidate. A
miss proves only that the name was not among those tried, so extending the vocabulary is the
method, and because the vocabulary is data that costs no rebuild.

## Unknown NIDs

A NID no database names stays unknown, which is a normal, reportable state.
`orbistoun-cli imports` prints `<unknown>` in its place. An unknown NID still says
that the title needs the function, how often it is called, and from where.

## What a name does not give

A resolved name says what a function is called, not what it does, returns or expects.
[TESTING.md](TESTING.md) covers where those answers come from.
