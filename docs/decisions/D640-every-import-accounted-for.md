# D640 - Every import accounted for, with a reason

**Status:** measured
**Date:** 2026-09-09

## The run said nothing, and the wall read as a missing implementation

D630 established that six of PPSA25872's imports are exported by modules the title ships, that all
six are answered with a placeholder, and that one of them is then called **19,689,015 times** -
87.6% of every call this project has recorded.

What it could not establish was *why*, because the run printed **nothing at all** about binding:
not how many imports were bound, not how many were kept, not that the question had been asked. Four
functions across three crates had to be read to find out that the machinery existed, and reading
them did not answer it either.

So the first change is not a fix. It is an account.

## Every import, bound or not, grouped by reason and by library

```text
orbistoun: 141 of 1123 import(s) bound to a module this title ships
orbistoun:   from libc 353, PS5Util 6, Il2cppUserAssemblies 5, libSceAmpr 5
orbistoun:   353 unbound from libc - orbistoun implements it, so this project answers instead
orbistoun:   95 unbound from libkernel - no module this title ships exports under that library name
orbistoun:   68 unbound from libSceAgc - no module this title ships exports under that library name
```

Five reasons, each a **different fix**: a library id the module's own table does not list is a
parsing question; a library nothing exports under is a placement question; a hash a placed module
does not export is a question about that module; an ambiguous unattributed name is D483's; and
`KeptByOrbistoun` is not a failure at all.

**Grouped by reason *and* library**, because grouping on the reason alone gives one example out of
four hundred and cannot answer the question a reader actually has - *is my module in this bucket?*
PS5Util among the imports orbistoun claims to implement would be alarming; libc there is correct;
and one example line said neither.

Printed **whether or not anything went wrong**: "141 of 1123 bound" and silence are different
statements and only one is evidence.

## What the account found, in three steps

**One.** `by_library` said `PS5Util 2` resolved and the kept list showed only libc - so both PS5Util
imports were bound. The run still showed 19.7 million calls on a stub labelled PS5Util. Two printed
facts contradicting each other, which is the first time either was printed.

**Two.** A bound import is called *inside* the module; orbistoun never sees it. So a bound import
with stub calls is impossible, and `bound_yet_called` now says so when it happens. It was silent -
so the calling index was never bound. `tables_disagree` was silent too, so the label and counter
tables agree and the attribution is sound.

**Three.** `bind_to_title_modules` read `owned.first()` - **the executable, and nothing else**. A
module importing from a *sibling* module was never a candidate, and that is the whole wall:
`PS5Util.prx` is placed, started, and exports what the guest calls, but the caller is another of
the title's own modules.

Every module's imports are considered now, keyed by shared slot rather than per-module index.
`PS5Util 2` became **`PS5Util 6`**, and bound went 64 to 141.

## And the run is unchanged, which is the honest half

Same 141 imports, same 20,000,000 calls, same 19,689,023 on stubs. The bindings are computed and
**not applied**: modules are relocated with an `OffsetResolver` that never consults `bound`, and
they are relocated *before* the binding is computed.

Fixing that means composing `TitleResolver` with `OffsetResolver` in the right order and moving the
computation above the relocation loop. The composition semantics live in
`crates/orbistoun-loader/src/relocate.rs`, which this session cannot read - the tooling refuses it -
so the ordering is stated rather than guessed, and the change stops here.

**Recorded as half a fix**, deliberately. The account is the durable half: it is what turned "a
missing implementation" into "the caller is a sibling module and the resolver never sees the
binding", and it will say the same thing to whoever finishes it.
