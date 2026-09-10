# D606 - A name is a seed for the next name

**Status:** assumed
**Date:** 2026-09-08

## What the export table showed

A console's own kernel export table (D605) puts 2,443 hashes against 2,405 addresses. Thirty-eight
of those addresses carry two hashes each, which means one function under two names - a fact about
the platform that no amount of searching recovers, because a hash carries nothing of its neighbours.

Thirty of the thirty-eight already had both sides named. Every one of them looks like this:

```text
0x8000006f0   getpeername            =  _getpeername
0x800012e00   inet_pton              =  __inet_pton
0x800105ae0   fileno                 =  fileno_unlocked
0x80001af50   sceKernelEnableDmemAliasing  =  sceKernelEnableDmemAliasing2
0x8001683f0   _ZNSt6_WinitC1Ev       =  _ZNSt6_WinitC2Ev
0x800035640   sceKernelPrintBacktraceWithModuleInfo
              =  sceKernelInternalHeapPrintBacktraceWithModuleInfo
```

**Not one of them is compositional.** The second name is the first with something stuck on it, or
with one substring swapped. The generator in `vendor.toml` cannot reach any of them, and no
extension of its word lists ever will: `snprintf` is not a word, it is a finished name.

## The change

A new source in the sweep. It takes names this project **already holds** and applies rules from a
new data file, `crates/orbistoun-names/data/affixes.toml` - prefixes, suffixes, and substitutions
for the Itanium ABI's constructor and destructor numbering. Everything else about the search is
unchanged: a candidate is hashed, the hash agrees or it does not, and a match is proof.

Run against the 8,321 hashes on `wanted.txt` plus the 273 kernel exports nothing named, on a
seed list of every held name, it costs 471,654 candidates - a rounding error beside the 2.6-billion
generative sweep - and named twelve:

```text
_ZNSt14error_categoryD0Ev                  <- _ZNSt14error_categoryD2Ev
_ZNSt14error_categoryD1Ev                  <- _ZNSt14error_categoryD2Ev
_err                                       <- err
mmap2                                      <- mmap
sceKernelGetAppInfo2                       <- sceKernelGetAppInfo
sceKernelGetResidentFmemCount2             <- sceKernelGetResidentFmemCount
sceKernelKernelHeapUsage2                  <- sceKernelKernelHeapUsage
sceKernelMapNamedFlexibleMemoryInternal2   <- sceKernelMapNamedFlexibleMemoryInternal
sceKernelMapNamedSystemFlexibleMemory2     <- sceKernelMapNamedSystemFlexibleMemory
sceKernelTitleWorkaroundIsEnabled2         <- sceKernelTitleWorkaroundIsEnabled
sceVideoOutSubmitChangeBufferAttribute2    <- sceVideoOutSubmitChangeBufferAttribute
snprintf_s                                 <- snprintf
```

`_ZNSt14error_categoryD1Ev` is worth singling out. The export table said the hash at `0x8000c4580`
is an alias of `_ZNSt14error_categoryD2Ev`; the rules said the name is the `D1Ev` spelling; the hash
agreed. The prediction and the proof are independent and they met.

## Why this is its own provenance tier

`Method::Affixed { seed, rule }`, not `Method::Generated`.

The obvious shortcut was to express the rules as a `Pattern` - `prefix × seed × suffix` is a
product, and `Pattern` already indexes products, so `verify` and `derive` would have worked
untouched. It does not survive contact with what the seed list *is*: **every name this project
holds**, which changes on every successful search. A name derived at index *N* today is at a
different index tomorrow, so every affixed record would go stale for a reason having nothing to do
with the record. `repair_generated_records` exists because that already happens once (D304); doing
it deliberately is not an improvement.

So a separate variant, and it is **more** checkable than the one it declines to borrow. Rechecking
a `Generated` claim means resolving a grammar and indexing into it; rechecking this means applying
one rule to one string. Both are `Reproducible::FromRepository`; only one of them is legible in a
diff.

## Two things it deliberately does not do

**A seed is never a candidate.** The seeds are the harvested standard list, the loaded database, and
whatever this run has already *proved* - never something proposed. A derivation claiming a proved
name was built from an unproved one is a lie the audit has no way to catch afterwards.

**One pass, not a fixed point.** `snprintf_s` becomes a seed on the next run, once it is in the
database, not on this one. Iterating multiplies the candidate count by the depth to buy names
nobody has evidence exist, and the whole reason this source is affordable is that it is cheap.

## The refusal that is easy to omit

An empty prefix and an empty suffix reproduce the seed. Both are in the file on purpose - without
them no one-ended variant is reachable - so `variants_of` refuses the identity explicitly, and
`derive_affixed` refuses a seed equal to the name. Without that, a record could say `snprintf` was
derived from `snprintf` and every check in the crate would accept it.

## Assumed

Two things nobody has agreed to: that a new `Method` variant is the right shape rather than
stretching an existing one, and the rule list itself. The second is data and cheap to extend, which
is the point; the first is a schema change and is in the review queue.
