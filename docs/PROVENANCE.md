# Provenance: how to show a name was derived, not taken

This project resolves imports by name, and names are the one artefact here that
somebody could reasonably ask hard questions about. A hash-to-name mapping is valuable,
public databases of them exist, and "we worked it out ourselves" is a claim like any other
unless something can check it.

So nothing here asks to be believed. Every name orbistoun reports carries a record of how
it was arrived at, and every record is **re-run rather than read** - by CI where CI holds
the material, and by whoever holds the rest.

## The argument, in one line

> A name this repository's own generator can produce is self-evidently derivable from
> this repository. A name it cannot produce is the one that needs explaining.

That reduces a question about intent - did you generate this, or copy it? - to a
question about arithmetic, which anybody can answer without trusting anyone.

## Be precise about what that does and does not claim

It is easy to read the sentence above as "the whole database could be rebuilt with no
guest module at all". **It could not, and it is worth being exact about why.**

| | Comes from |
|---|---|
| The **names** | This repository. The grammar and the word list, and nothing else |
| Knowing **which** of them are real | A module's import table |

The generator produces billions of candidates. It has no idea which of them name a
function that exists - that is precisely what it cannot know, and precisely what a real
import table supplies. A candidate is accepted only when its hash equals one the module
declares it needs.

So: **the names are ours; the selection is the module's.** The audit proves the first
half, which is the half a provenance question is actually about - it shows every name in
the tree came out of inputs that are visible in the tree, rather than out of somebody
else's database.

## Two questions, and only one of them varies

Every record here answers **how a candidate was proposed**. None of them describes how it
was *confirmed*, because confirmation is the same act every time:

> The candidate is hashed. The hash either equals one a real module declares it needs, or
> it does not.

**The hash is the oracle.** A match is proof, not a judgement, and it needs no external
authority to confirm - nothing is consulted, and there is nothing to consult that would
help. That is true of a name generated from the grammar, a name read out of a module's own
data, and a name a guest printed while running. They differ in where the candidate came
from and in nothing else.

Which leaves two axes worth recording, and the vocabulary carries both:

- **Evidence** - what kind of material proposed the candidate. `derived`, `static`,
  `runtime`, `external`.
- **Reproducible** - what somebody else would need in order to arrive at it. This is a
  tier, not a yes/no, and it is what the audit sorts by.

### The vocabulary

| Record | Evidence | Reproducible | Means |
|---|---|---|---|
| `published-standard` | derived | from this repository | Fixed by ISO C or POSIX, from a list shipped here |
| `generated` | derived | from this repository | Candidate *n* of a named pattern in `crates/orbistoun-names/data/vendor.toml` |
| `static` | static | + the module | Read out of guest material **at rest**. Nothing executed |
| `runtime` | runtime | + a run of it, or hardware | Learned from something **executing** |
| `supplied` | external | only from its source | Came from outside this project |

The two axes are deliberately not the same partition. A name a conformance probe reported
is runtime evidence, but it sits a tier above a local run, because the hardware it came off
is not something CI - or anybody here - can be handed.

`static` and `runtime` carry a closed subtype naming the mechanism, so records can be
counted rather than read. A new mechanism adds a value; it does not add a new sentence.

### Why `observed` is gone

There used to be a value called `observed`, covering everything that was neither generated
nor imported. Its own documentation said **"a name worked out by watching something run"** -
and 137 of the 154 names carrying it had never run anything. They were literal strings read
out of a file at rest.

That is not a wording problem. The two are different claims with different checks:

- A string in a module is there deterministically. Re-read the file and it is there again.
  **Anyone holding that title can verify the record**, and now something does
  (`audit --verify-harvest`).
- A conclusion drawn from watching a guest run is reproducible only by running it again,
  and a guest is not obliged to reach the same place twice.

A bucket holding both had to call both *documented, not verified*, because there is no
single check that applies to both - so neither got one, and the first was sold far short of
what it could prove. Splitting them made the static tier checkable for the first time
(D213).

## Every way a name gets here

Eight mechanisms. The `by` column is the value a record carries; the last column is what
each has actually put in `symbols/generated.json`, which is a different question from
whether it runs.

| # | Mechanism | Evidence | `by` | Names today |
|---|---|---|---|---|
| 1 | Published standard-library list, harvested from FreeBSD's own `Symbol.map` files | derived | - | 247 |
| 2 | Grammar enumeration - prefix, module, action, object | derived | - | 273 |
| 3 | Identifier-shaped strings in a module's own bytes (D193) | static | `module-strings` | 182 |
| 4 | The same, pooled across a corpus: one module's strings naming another's imports | static | `cross-module` | 18 |
| 5 | Reasoning about a real call trace, confirmed by hash | runtime | `call-trace` | 17, all by hand |
| 6 | Strings in what a guest passed to an import, read out of its memory as it ran | runtime | `argument-dump` | **0** - runs, finds nothing yet |
| 7 | A name our own conformance probe reported on hardware | runtime | `probe-transcript` | 0 - never exercised |
| 8 | A name taken from outside this project | external | - | 0 - deliberately |

**Three zeroes, and they are not the same zero.**

Mechanism 6 **runs over the whole corpus on every `./bin/orbistoun names`** and reports what
it did: 20 identifier-shaped candidates out of 113 captures across 6 previous runs, none of
which hashed to a wanted import. The mechanism is not the limit - the source is. Argument
dumps are forced per-import rather than captured broadly, so almost all of them so far are
scalars carrying no text at all. It stays wired in and says so out loud each run, because a
source that quietly contributes nothing looks exactly like one that is working.

Mechanism 7 is reachable - `--words-from probe` - and has still never been given a list, but
the reason has changed and is now a much smaller one. **obSCEne exports the list.** Its
`Makefile` builds `build/symbols.txt` from `obscene-host --symbols`, generated from the check
registry rather than hand-maintained, so it cannot drift from what the probe actually calls.

What does not line up is the shape. obSCEne emits `<library> <symbol>` per line - it has to,
because `mkmodule` needs to know which library resolves each name - and
[`word_list`](../crates/orbistoun-names/src/lib.rs) hashes each line whole. So every entry
would hash as `"libkernel sceKernelWrite"` rather than as `sceKernelWrite`, match nothing, and
report zero named - which reads as the mechanism failing rather than as a column that needed
dropping. Taking the second field is the whole of the remaining work.

Mechanism 8 has produced nothing on purpose, and the day it produces something the audit
will say so on its own line. That is the whole reason the category exists.

Mechanism 4 is the newest and it changed the shape of the search. A name lying in one
title's diagnostic text is the vendor's own spelling of a function that **every** title on
the platform imports - so pooling the corpus found the whole C11 Annex K family
(`memcpy_s`, `strcpy_s`, `sprintf_s` and the rest) in the vendor C library module's own
strings, and used them to name imports of titles that never mention them. A
module-at-a-time search structurally could not have found those, however many titles
arrived.

### And one that was checked and does not work

**Symbol-table residue.** An incompletely stripped module would leave real names in a
`.symtab`, which would be the cheapest source on this list. Every module in the local
corpus was checked: **none has one.** Five carry a `.shstrtab` and nothing else, and the
dynamic symbol names are the encoded-hash form rather than text. It is not implemented
because there is nothing for it to read - which is worth writing down, so the next person
does not spend an afternoon establishing it again.

Three more are described but not built, in [BACKLOG.md](BACKLOG.md): strings from mapped
memory after relocation, call-position inference, and cross-title argument correlation.

## Why reading an import table is a different kind of act

An import table is a list of hashes of **system library** function names - the same
values appear in everything built for the platform, because they identify the operating
system's own interface rather than anything belonging to a title. Reading one is what a
linker does, and it is the minimum possible act of interoperability: no code is copied,
no content is examined, and nothing about how the module works is inspected.

**Reading a module's strings is a larger act, and the document should not blur that.**
Mechanisms 3, 4 and 6 above do examine content - specifically, runs of identifier-shaped
bytes. What they do not do is examine *code*: nothing is disassembled, no control flow is
followed, and no structure is reproduced. What is taken is a function's published name,
which is an interface identifier and is the same in every title on the platform. The
convergence problem principle 1 exists to prevent is about reimplementing behaviour from
someone else's implementation; a name is not behaviour.

The `provenance` CI job fails the build on any of the material this could shade into -
firmware, keys, dumps, disassembly, guest binaries - and the corpus itself is never
tracked.

## What is recorded

Every name found is stored with a **derivation**: what proposed it, and where.

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
      "from": "titles/PPSA02664-app0/sce_module/libc.prx",
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

Every entry carries the day it was recorded. **Derivations are written by whatever did the
work, at the moment of discovery**: a provenance record assembled afterwards is a
reconstruction, one written at the time is evidence.

### On `supplied`

Never verifies, and is listed loudly and on its own. Taking a name from a public
database is lawful and sometimes sensible - but it is the single category that changes
the answer to "did you work all of this out yourselves?", so it must never be quiet.

Nothing in `symbols/` is `supplied` today. The one thing in this repository that is comes
from outside is the hash suffix, discussed below.

## How it is checked

```bash
orbistoun-cli audit symbols/generated.json                    # what CI runs
orbistoun-cli audit symbols/generated.json --verify-harvest   # and what a corpus adds
orbistoun-cli audit symbols/generated.json --repair           # re-derive stale coordinates
```

For each name, the recorded derivation is **re-run, not trusted**:

- `generated` - the named pattern is evaluated at the recorded index, and the result must
  equal the name. A forged record does not help: pointing at a pattern and index that
  produce something else fails exactly as loudly as no record at all.
- `published-standard` - the name must actually appear in the shipped list.
- `static` - **with `--verify-harvest`, the module named in the record is re-read and
  re-scanned, and it must contain the string.** Not something CI can do, because CI has no
  corpus and never will; entirely something a person with the title can do, in seconds.
  Absent modules are counted and named, never passed.
- `runtime` - cannot be re-run from a file, and is reported in its own tier saying what it
  would take.

Names with no record can be searched for from scratch with `--deep`, which walks the
entire candidate space and answers "could this repository have produced this at all?" with
no help from the file being audited.

**This runs in `./bin/orbistoun check` and in CI**, over every database in `symbols/`. It
is cheap - verifying a recorded derivation is an array lookup - which is what makes it a
gate on every commit rather than a thing somebody meant to do before publishing.

### Why `--repair` exists, and what it revealed

A generated record is a pattern plus an index. The index is what makes checking it a
microsecond instead of a full sweep, and it is also a position in a mixed-radix
enumeration over the vocabularies - so **adding one word to a vocabulary renumbers every
candidate built from it**.

Adding words is the loop. Every confirmed name is split into parts and fed back into the
grammar so the next search reaches further (D195). Each time that happens, some
already-recorded name stops being at the index its record names. Nothing is wrong with the
name and nothing is wrong with the claim; the coordinates moved underneath it.

Those names were falling onto the unaccounted ceiling - a file whose whole rule is that it
may only shrink. It had accumulated **202 entries**, and its own header described them as
vendor names the grammar could not spell. That description was wrong: the grammar could
spell every one of them. `--repair` hashes the stale names, hands them to the ordinary
pattern search as targets, and re-derives all of them in **one** sweep. The ceiling is now
empty (D213).

### The knowledge base holds a second copy, and it is checked now

`crates/orbistoun-hle/data/knowledge/*.toml` carries a `found_by` on each function - the
same claim about the same name, hand-written. It drifted into **eleven contradictions**
with the audited record, including one `supplied` on a name the generator produces, and a
test asserting the knowledge base "accounts for everything it claims" was green throughout,
because it had never looked at that field (D213).

It compares against `symbols/generated.json` now, and the label must be current vocabulary.
The duplication itself survives for one reason worth knowing: **an implemented function's
name never enters the unnamed set**, so no search ever records a derivation for it. **57 of
the 95 declared functions have no record at all**, and the 38 that do have one only because
a search happened to confirm them before anything implemented them.

So the audit covers the names this project has *not* yet acted on, and covers the ones it
has only where the ordering favoured it. That is the largest remaining hole in this
document's claim, it is not hidden, and [BACKLOG.md](BACKLOG.md) carries the fix - which is
not a retrofitted record, because one written now would be a reconstruction, but a sweep
that answers the only thing the audit ever claims: *could this repository produce this
name?*

## Why the search itself is defensible

**Published standards are not guesses.** The target C library is FreeBSD-derived, so a
large part of it is ISO C and POSIX under the names those standards publish. **3,018**
of them ship in `crates/orbistoun-names/data/standard.txt`. Nothing was read out of a
vendor binary to write that list; the standards are the source, and they are public.

That list used to be hand-curated, **which was the weakest link in this whole document**.
"Somebody wrote these down from the standards" cannot be audited, and it is bounded by
what one person thought of. It is now generated, and the file says so in its own header:

```bash
git clone --filter=blob:none --sparse https://github.com/freebsd/freebsd-src
cd freebsd-src
git sparse-checkout set lib/libc lib/libthr lib/msun lib/libutil lib/libsys
orbistoun-cli harvest /path/to/freebsd-src --revision releng/14.0
```

That reads the `Symbol.map` files FreeBSD publishes with its own source - the
authoritative statement of what those libraries export - and overwrites the word list
with a generated one whose header names the source and revision it came from. Bigger,
current, and repeatable by anyone: 470 hand-written names became 3,018 cited ones.

**Vendor names are enumerated, then confirmed by arithmetic.** The convention is strict -
prefix, module, action, object, revision mark - so the plausible space is small enough to
exhaust. `crates/orbistoun-names/data/vendor.toml` generates billions of candidates across
eight patterns, searched at around 30 million per second, and the exact figure moves every
time a confirmed name teaches it a word.

**Reading a module's own strings outperforms both**, and it is why the corpus is searched
as one thing rather than a module at a time. Anything confirmed is split into words and
fed back into the grammar, so each success makes the next search cheaper (D195) - and the
`--repair` pass above is what keeps that from costing the records already written.

## What is deliberately not here

- **No NID database is bundled, downloaded, or read.** Public ones exist and any file of
  the right shape will load, but nothing here depends on one, and no name in `symbols/`
  came from one.
- **No disassembly, and no vendor binaries.** The `provenance` CI job fails the build on
  those, and always has (D003, D042).
- **The hash suffix is not a key** and is not treated as one. It is a salt on a
  name-mangling hash - it decrypts nothing and protects nothing. It lives in
  `crates/orbistoun-nid/data/hash-suffix.toml` with its origin and its limits written
  next to it, rather than as an unexplained constant in source (D071).

  **It is the one thing here that is `supplied` rather than ours**, and the document
  should not blur that. It could not have been derived - a sixteen-byte salt cannot be
  brute-forced, and a known name-and-hash pair does not invert SHA-1. It was recalled
  from publicly published material and then checked against real imports, which makes it
  *checkable*, not *ours*. See that file for the full account (D085).

## The honest limits

- **The audit proves derivability, not history.** It shows this repository *can* produce
  a name; it cannot prove that is how the name first arrived. That distinction is real,
  and it is why `static`, `runtime` and `supplied` are reported in their own tiers - a
  name that did not come out of the generator should say so, and does.
- **A name outside the grammar is not thereby wrong.** Vocabularies shrink, patterns get
  rewritten, and a name derivable last month may not be today. An audit failure means
  "a person should decide about this deliberately", not "this is illegitimate".
- **A miss proves nothing.** The search only ever says "not among those tried".
  Extending `data/vendor.toml` is the method, and it needs no rebuild.
- **`--verify-harvest` is only as good as the corpus in front of it.** A machine without
  the titles reports every static record as *not checked*, by name, and that is the
  correct answer rather than a pass.

## If you are picking this up cold

```bash
# What can this repo name, and how much is still unknown? One search over the whole corpus.
orbistoun-cli names titles --out symbols/generated.json --wanted symbols/wanted.txt

# Prove every name in that file came from here, and re-read the modules that back the rest.
orbistoun-cli audit symbols/generated.json --verify-harvest

# And for names with no record, search the whole space rather than trusting the file.
orbistoun-cli audit symbols/generated.json --deep
```

`./bin/orbistoun names` is all three, in order, with `--repair` in between.

`symbols/wanted.txt` is the work list: the hashes still unnamed, which is what the next
round of vocabulary work is aimed at.
