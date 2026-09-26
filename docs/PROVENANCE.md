# Provenance

How a symbol name is shown to be derived by this project rather than taken from elsewhere.
Every name orbistoun reports carries a record of how it was proposed, and every record is
re-run rather than read: by CI where CI holds the material, and by whoever holds the rest.

## The claim

> A name this repository's own generator can produce is derivable from this repository. A
> name it cannot produce is the one that needs explaining.

This turns a question about intent into a question about arithmetic that anyone can check.

| | Comes from |
|---|---|
| The names | this repository: the grammar and the word lists |
| Which of them are real | a module's import table |

The generator produces far more candidates than exist. A candidate is accepted only when its
hash equals one a module declares it imports. The names are ours; the selection is the
module's. The audit proves the first half, which is the half a provenance question is about.

## The hash is the oracle

Confirmation is the same act for every name: the candidate is hashed, and the hash either
equals one a real module imports or it does not. A match is proof and consults no external
authority. Names differ only in where the candidate came from, so a record describes how a
candidate was proposed, never how it was confirmed.

A record carries two axes:

- **Evidence** - the kind of material that proposed the candidate: `derived`, `static`,
  `runtime`, `external`.
- **Reproducible** - what someone else needs to arrive at it. This is a tier, and it is what
  the audit sorts by.

## The `found` vocabulary

| `found` | Evidence | Reproducible from | Means |
|---|---|---|---|
| `published-standard` | derived | this repository | fixed by ISO C or POSIX, from a list shipped here |
| `generated` | derived | this repository | candidate *n* of a named pattern in `crates/orbistoun-names/data/vendor.toml` |
| `static` | static | this repository and the module | read out of guest material at rest; nothing executed |
| `runtime` | runtime | this repository and a run, or hardware | learned from something executing |
| `supplied` | external | its source only | came from outside this project |

The two axes partition differently. A name a conformance probe reported is runtime evidence,
but sits a tier above a local run, because the hardware it came from cannot be handed to CI.

`static` and `runtime` carry a closed subtype, `by`, naming the mechanism, so records can be
counted rather than read. A new mechanism adds a value. Static material and runtime
observation are separate tiers because they have separate checks: a string in a file is
there on every re-read, while a guest is not obliged to reach the same place twice (D213).

## Mechanisms

| Mechanism | Evidence | `by` |
|---|---|---|
| The published standard-library list, harvested from FreeBSD's `Symbol.map` files | derived | - |
| Grammar enumeration: prefix, module, action, object | derived | - |
| Identifier-shaped strings in a module's own bytes | static | `module-strings` |
| The same, pooled across the corpus: one module's strings naming another's imports | static | `cross-module` |
| Reasoning about a real call trace, confirmed by hash | runtime | `call-trace` |
| Strings in what a guest passed to an import, read from its memory as it ran | runtime | `argument-dump` |
| A name obSCEne's conformance probe reported on hardware (`--words-from probe`) | runtime | `probe-transcript` |
| A name taken from outside this project | external | - |

Pooling the corpus matters because a name in one module's diagnostic text is the vendor's
spelling of a function every title imports, so it names imports of titles that never mention
it. Every run reports what each mechanism contributed, including nothing, so a source that
contributes nothing is visible as such.

A symbol table left in an incompletely stripped module is not a mechanism: the modules carry
no `.symtab`, and their dynamic symbol names are in the encoded-hash form.

## Import tables and strings

An import table is a list of hashes of system library function names. The same values
appear in everything built for the platform, because they identify the operating system's
interface rather than anything belonging to a title. Reading one is what a linker does: no
code is copied and nothing about how the module works is inspected.

Reading a module's strings examines content: runs of identifier-shaped bytes. Nothing is
disassembled, no control flow is followed, and no structure is reproduced. What is taken is
a function's published name, an interface identifier that is the same in every title. A
name is not behaviour, and reimplementing behaviour from another implementation is what the
provenance rule exists to prevent.

The `provenance` CI job fails the build on firmware, keys, dumps, disassembly and guest
binaries, and the corpus is never tracked.

## The record

Every name is stored in `symbols/generated.json` with a derivation: what proposed it, where,
and on what day.

```json
{
  "names": ["memcpy", "sceKernelDirectMemoryQuery", "strcpy_s", "__cxa_throw"],
  "derivations": {
    "memcpy": {
      "found": "published-standard",
      "list": "crates/orbistoun-names/data/standard.txt",
      "on": "2026-08-19"
    },
    "sceKernelDirectMemoryQuery": {
      "found": "generated",
      "pattern": "prefix-module-object-verb-object",
      "index": 87680,
      "on": "2026-08-19"
    },
    "strcpy_s": {
      "found": "static",
      "by": "cross-module",
      "from": "titles/<title>-app0/sce_module/libc.prx",
      "on": "2026-08-24"
    },
    "__cxa_throw": {
      "found": "runtime",
      "by": "call-trace",
      "how": "a libc import taking 53.5% of all calls is allocation or static initialisation, both C++ ABI - confirmed by hash",
      "on": "2026-08-20"
    }
  }
}
```

Derivations are written by whatever did the work, at the moment of discovery. A record
assembled afterwards is a reconstruction; one written at the time is evidence.

`supplied` never verifies and is listed on its own line. Taking a name from a public
database is lawful, but it is the one category that changes the answer to "did you work
this out yourselves?", so it is never quiet. The hash suffix is the one `supplied` input in
the repository (see below).

## The audit

```bash
orbistoun-cli audit symbols/generated.json                    # what CI runs
orbistoun-cli audit symbols/generated.json --verify-harvest   # adds a local corpus
orbistoun-cli audit symbols/generated.json --repair           # re-derive stale coordinates
orbistoun-cli audit symbols/generated.json --deep             # search the whole space
```

Each recorded derivation is re-run:

- `generated` - the named pattern is evaluated at the recorded index and must equal the
  name. A forged record pointing at a pattern and index that produce something else fails
  as loudly as no record.
- `published-standard` - the name must appear in the shipped list.
- `static` - with `--verify-harvest`, the module named in the record is re-read and must
  contain the string. CI has no corpus; anyone holding the title can run it. Absent modules
  are counted and named as not checked, never passed.
- `runtime` - cannot be re-run from a file, and is reported in its own tier with what
  checking it would take.

`--deep` searches the entire candidate space for names with no record, answering "could this
repository have produced this at all?" without help from the file being audited.

`--repair` exists because a generated record's index is a position in a mixed-radix
enumeration over the vocabularies: adding a word renumbers every candidate built from it.
Confirmed names are split into words and fed back into the grammar (D195), so indexes go
stale while the names stay derivable. `--repair` hashes the stale names, hands them to the
pattern search as targets, and re-derives all of them in one sweep.

`--ceiling symbols/unaccounted-ceiling.txt` compares the unaccounted set against a written
ceiling. It fails on a name that is unaccounted and not listed, and on a listed name that
has since been accounted for, so the list only shrinks.

The audit runs in `./bin/orbistoun check` and in CI over every database in `symbols/`.
Verifying a recorded derivation is an array lookup, which is what makes it a gate on every
commit.

### The knowledge base's `found_by`

`crates/orbistoun-hle/data/knowledge/*.toml` carries a `found_by` on each function: the same
claim about the same name. A test compares it against `symbols/generated.json` and requires
current vocabulary (D213). An implemented function's name never enters the unnamed set, so
no search records a derivation for it; the audit's claim is only ever that this repository
could produce the name.

## Behavioural provenance: `known_by`

Names record `found`. Behaviour - what a function does, returns or requires - records
`known_by` on each claim in the knowledge base:

| `known_by` | Means |
|---|---|
| `published` | stated by a public standard or document, which is cited |
| `measured` | observed on hardware, with the observation cited |
| `guest-observed` | inferred from what a guest did while running |
| `assumed` | a written-down assumption |

Every value is falsifiable, and none means "already known". `published` and `measured` claim
outside support, so they cite it; an uncheckable claim of support is worth less than an
honest `assumed`. `assumed` is a normal state: a written assumption can be counted, ranked,
probed and retired. `assumptions` lists what `known_by` does not cover. `orbistoun-cli
learn` refuses an entry without `known_by`, and CI refuses a tree without one.

## Search inputs

**Published standards.** The target C library is FreeBSD-derived, so much of it is ISO C
and POSIX under the published names. `crates/orbistoun-names/data/standard.txt` is generated
from the `Symbol.map` files FreeBSD publishes with its source, the authoritative statement of
what those libraries export, and its header names the source and revision:

```bash
git clone --filter=blob:none --sparse https://github.com/freebsd/freebsd-src
cd freebsd-src
git sparse-checkout set lib/libc lib/libthr lib/msun lib/libutil
orbistoun-cli harvest <freebsd-src> --revision <tag-or-commit>
```

**Vendor names.** The vendor convention is strict - prefix, module, action, object, revision
mark - so the plausible space can be exhausted. `crates/orbistoun-names/data/vendor.toml`
defines the patterns; extending it needs no rebuild.

**Module strings.** The corpus is searched as one thing rather than a module at a time, and
anything confirmed feeds the grammar (D195).

## Excluded material

- No NID database is bundled, downloaded or read. Any file of the right shape loads, but
  nothing depends on one and no name in `symbols/` came from one.
- No disassembly and no vendor binaries; the `provenance` CI job fails the build on them.
- The hash suffix is a salt on a name-mangling hash, not a key: it decrypts and protects
  nothing. It lives in selfish's `data/hash-suffix.toml`, read through `selfish-nid`, with
  its origin and limits beside it (D071). It is `supplied`: a sixteen-byte salt cannot be brute-forced and a
  known name-and-hash pair does not invert SHA-1. It comes from publicly published material
  and is checked against real imports, which makes it checkable rather than ours.

## Limits of the audit

- It proves derivability, not history: that this repository can produce a name, not how the
  name first arrived. That is why `static`, `runtime` and `supplied` are separate tiers.
- A name outside the grammar is not thereby wrong. Vocabularies and patterns change, and a
  failure means a person decides about the name deliberately.
- A miss proves nothing; the search says only "not among those tried".
- `--verify-harvest` is only as good as the corpus present. Without the titles, every static
  record is reported as not checked, by name.

## Commands

```bash
# Name what can be named from the local corpus; the unnamed hashes go to wanted.txt.
orbistoun-cli names <titles-dir> --out symbols/generated.json --wanted symbols/wanted.txt

# Prove every name came from here, and re-read the modules behind the static records.
orbistoun-cli audit symbols/generated.json --verify-harvest

# Search the whole space for names with no record.
orbistoun-cli audit symbols/generated.json --deep
```

`./bin/orbistoun names` runs all three in order, with `--repair` between them.
`symbols/wanted.txt` is the work list: the hashes still unnamed, which the next round of
vocabulary work targets.
