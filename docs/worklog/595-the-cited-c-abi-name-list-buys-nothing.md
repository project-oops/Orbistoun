# 595. The cited C++ ABI name list buys nothing, measured three ways

**2026-09-15** - step 1 of the gap analysis, and the item BACKLOG 013 calls "the highest-value
item on this list". It is not, and the measurement says so rather than an argument.

## What the item asks for

The seventeen names recorded at the `call-trace` tier are `__cxa_*` and mangled C++ ABI symbols.
They are **published**, by a public specification, and are simply not in any list this repository
ships - so they record as runtime evidence and sit a tier below where they belong. Harvesting a
symbol map the way `standard.txt` was harvested from FreeBSD would move all seventeen.

The classification is right. All seventeen trace to one document, the Itanium C++ ABI:

| Group | Names | Where the spec puts them |
|---|--:|---|
| `__cxa_*` runtime support | 10 | §3, named individually |
| `_Unwind_Resume` | 1 | Level I base unwinder API |
| `__gxx_personality_v0` | 1 | the personality routine of the exception section |
| mangled `_Z*` | 5 | the spec's own mangling rules over entities it names |

## What it buys, measured

**Nothing, against three separate C++ vocabularies.** All three were run through
`orbistoun-cli names` - the project's own search, not a scratch script - against the whole corpus:
**2,932 distinct unnamed imports across 54 modules**.

| Vocabulary | Candidates | Named |
|---|--:|--:|
| GCC 15 `cxxabi.h`, every identifier in it | 659 | **0** |
| GNU `libstdc++.so.6` exports (6,000 of them mangled `_Z*`) | 6,046 | **0** |
| The same, transformed to libc++'s `std::__1` inline namespace | 3,048 | **0** |

The third is the one worth having tried. libc++ puts the standard library in inline namespace
`std::__1`, so its mangled names are systematically `_ZNSt3__1…` where GNU's are `_ZNSt…`; a GNU
list would miss a libc++ target almost by construction, and the target's toolchain is LLVM. Making
that substitution mechanically and re-running is cheap, and it still finds nothing.

**So the 2,932 unnamed imports are not C++ standard-library or C++ ABI symbols.** That is the
useful part of this result: a whole class is eliminated, which is worth more than the re-citation
the item was asking for.

## The negative is a real negative

A search that reports zero is worthless unless it can report non-zero, so the supplied-words path
was given a positive control: the same command, against the same corpus, with an **empty** symbol
database - so that names already known count as unnamed again - and three known C++ names
supplied.

```
supplied names: 3 tried, 1 named (Supplied)
```

It reports hits when hits exist. The three zeroes above are measurements, not a broken pipe.

(The first attempt at this control pointed at `sce_module/`, got `3 tried, 0 named`, and would
have "confirmed" the wrong thing - those directories hold the libraries that *export* these
symbols, not the executable that imports them. The control only became a control once it was
aimed at the importer.)

## And it cannot do the provenance job either

The route a supplied list takes is `--words … --words-from supplied`, and the flag's own
documentation is explicit: *"Anything from outside is `supplied`, never verifies, and is listed
separately by an audit"* (D119). That is a **lower** tier than `call-trace`, not a higher one - so
feeding a C++ ABI list in this way would move the seventeen down.

Doing it properly means what `standard.txt` did: a harvester reading a lawful source, producing a
shipped data file that a contributor can regenerate. That needs the source to be
**machine-readable**, and the authoritative source here - the Itanium C++ ABI specification - is
prose. The implementations that are machine-readable are a tier removed from it, and the only one
on this machine is GNU's, which is the wrong family for an LLVM target.

## What was refused

Building the harvester anyway. It would have cost an afternoon, cited GCC's header for nine of the
seventeen names, and added zero names - and principle 11 says take the path with the better end
state, not the one already written down.

Also refused: shipping any of these lists into the repository. They were used as candidate
vocabularies for a search that confirms by hash, which is the clean-room method this project
already relies on; none of them became data.

## The backlog entry is now wrong, and is corrected

BACKLOG 013 calls this "the highest-value item on this list" and PROJECT_STATUS calls it "the
cheapest". Cheap it is. Highest-value it is not, and the entry now carries this measurement so the
next reader does not spend the afternoon on the strength of the old claim.

## Surprise

**Two of my own measurements were wrong before they were right, and both failed silently.** A
`for` loop inside `wsl -- bash -lc '…'` had `$n` swallowed before WSL saw it, so
`grep "\b\b"` matched every line and reported all seventeen names present in a header that
contains nine. Then `nm` was handed a Linux path that MSYS rewrote into a Windows one. Neither
errored in a way that looked like an error - the first produced a confident wrong answer, which is
the worse failure of the two, and it was caught only by asking whether a mangled name could
plausibly appear in a header at all.

The lesson is the one CLAUDE.md already has, one level down: a measurement needs a control before
it is believed, and "all seventeen matched" deserved suspicion precisely because it was the answer
I wanted.
