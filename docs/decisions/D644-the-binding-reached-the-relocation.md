# D644 - The binding reached the relocation

**Status:** measured
**Date:** 2026-09-09

## Two things were wrong, and only the second needed the loader

D640 found that `bind_to_title_modules` read `owned.first()` - the executable alone - so a module
importing from a **sibling** module was never a candidate. Fixing that made the binding correct and
changed nothing, because the bindings were computed and never applied. Two faults, one behind the
other:

**The order.** The binding was computed *after* the module relocation loop had already run. A
relocation that has finished cannot be told which slot should have pointed into a placed module.
It is computed before the loop now, from the same `placed`, `modules` and `slots` that were already
in scope there.

**The composition.** Modules relocated through an `OffsetResolver`, which shifts a per-module symbol
index into the shared stub table, and never through the `TitleResolver` that consults the binding.
Wrapping one in the other raises a question - which of them sees the shifted index? - and
`crates/orbistoun-loader/` is denied to this session, so the answer could not be read.

## Sidestepped rather than guessed

The map is now **one per module, keyed by that module's own symbol index**, and the composition puts
the binding *outside* the offsetting:

```rust
let shifted = OffsetResolver { offset: slot.offset, inner: &shared };
let resolver = TitleResolver { bound: per_module.get(index + 1)…, inner: &shifted };
```

`TitleResolver` looks a symbol index up in the map it holds; because that map is keyed by the same
module's own indices, it **never sees a shifted index**. `OffsetResolver` shifts only what falls
through to the shared stub table. Neither has to know what the other does with the number, so the
ordering question does not arise.

That the executable's path has worked this way since it was written - a map keyed by symbol index,
which for module zero is also the slot - is what made the shape safe to assume without reading it.

## What it moved

```text
PPSA25872-app0   before:  141 imports  20000000 calls    2% standing
PPSA25872-app0   after:   141 imports    310987 calls  100% standing
```

**Nineteen million six hundred and eighty-nine thousand calls, gone.** They were the guest asking
`PS5Util::0xf948d02a4f9f5ace` the same question until its budget ran out; it now calls that function
*inside the module the title ships*, so orbistoun never sees it at all. Eight calls land on stubs,
out of three hundred and ten thousand.

Two other titles gained an import each - PPSA02664 to 199, PPSA03416 to 198 - and nothing lost one.

## The guard that would have caught it staying wrong

`bound_yet_called` (D640) fires when an import the binding claims is nevertheless called through a
stub, which cannot happen if the binding is applied. It was silent before this change because
nothing was bound at those indices, and it is silent after because the bindings now take effect.
The one state it would catch - bound and still stubbed - is exactly the failure this change could
have introduced by composing the resolvers the wrong way round.

`tables_disagree` is silent too, so the label and counter tables still agree and the attribution
behind those numbers is sound.
