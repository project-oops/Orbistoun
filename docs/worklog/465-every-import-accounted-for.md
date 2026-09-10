# 465. Every import accounted for

**2026-09-09** - directed

Two instructions: take the title-module wall, and **never let a run fail to say why**. The second
turned out to be how the first got solved.

## The account came first, and it did the work

The run printed nothing about binding - not a count, not a reason, not that the question had been
asked. So before changing anything, every import now gets a fate and a reason, grouped by reason
**and** library (D640):

```text
orbistoun: 141 of 1123 import(s) bound to a module this title ships
orbistoun:   from libc 353, PS5Util 6, Il2cppUserAssemblies 5, libSceAmpr 5
orbistoun:   353 unbound from libc - orbistoun implements it, so this project answers instead
orbistoun:   95 unbound from libkernel - no module this title ships exports under that library name
```

Five reasons, each a different fix. Grouped by library because one example out of four hundred
cannot answer *is my module in this bucket* - PS5Util among the ones orbistoun claims to implement
would be alarming, libc there is correct, and one example line said neither.

## Then three steps, each one a printed contradiction

1. The account said `PS5Util 2` resolved and not kept, so both were bound - while the run showed
   19.7 million calls on a PS5Util stub. **Two facts contradicting each other, printed for the
   first time.**
2. Two new checks settled which was wrong. `bound_yet_called` fires when a bound import has stub
   calls, which cannot happen; `tables_disagree` fires when the label and counter tables are
   different lengths. Both silent - so the attribution was sound and the calling index was simply
   never bound.
3. `bind_to_title_modules` read **`owned.first()`** - the executable alone. A module importing from
   a *sibling* module was never a candidate, and that is the wall: `PS5Util.prx` is placed,
   started, exports what the guest calls, and the caller is another of the title's own modules.

Every module's imports are considered now. `PS5Util 2` became **`PS5Util 6`**; bound went 64 to
141.

## The run is unchanged, and that is the honest half

Same 141 imports, same 20,000,000 calls, same 19,689,023 on stubs. The bindings are computed and
not **applied**: modules relocate with a resolver that never consults `bound`, and they relocate
before it is computed.

Composing the two resolvers in the right order needs `crates/orbistoun-loader/src/relocate.rs`,
which the tooling refuses to open in this session. Stated rather than guessed; the change stops
there. Three other titles and the payload were re-run and are byte-identical, which is what a
computed-but-unapplied binding should look like.

## Surprises

- **The reporting was the debugging.** Every step came from printing something the run already knew
  and had never said. Nothing was traced, disassembled or bisected.
- **`bound_yet_called` is a permanent guard for a class**, not a one-off: it says when two things
  the run believes cannot both be true. It was silent here, which was itself the evidence.
- **`owned.first()`** - the whole wall, one method call, in a function whose doc comment describes
  binding "the executable's imports" and is therefore accurate about what it does and silent about
  what it omits.

## Next

- Apply the binding to module relocations: move the computation above the relocation loop and
  compose `TitleResolver` with `OffsetResolver`. Needs `relocate.rs` readable.
- The remaining blocked list from 461.
