# 487. Prospero, Trinity, Orbis, Neo

**2026-09-10** - directed, continuing 486

The rename, test-first.

## The oracle settled the fourth name

obSCEne's own docs say *"PlayStation OS (Prospero / Orbis)"* and use `neo-mode`; Trinity was given
directly. So the four are not a guess:

| generation | revision | codename |
|---|---|---|
| Orbis | base | `orbis` |
| Orbis | pro | `neo` |
| Prospero | base | `prospero` |
| Prospero | pro | `trinity` |

## Test-first, and both failed for the right reasons

```text
error[E0599]: no variant named `Prospero` found for enum `Generation`
error[E0432]: unresolved import `super::Platform`
error[E0599]: no method named `platform` found for struct `Machine`
```

The first test walks all four pairs, collects the names, deduplicates and asserts four remain -
the failure it guards is two combinations collapsing onto one label, which would have a report say
`prospero` for a machine that is not one. The second asserts the **absence**: `describe()` contains
neither `ps5` nor `ps4`. A positive assertion alone would pass `prospero-ps5/cex/base`, and the
absence is the point (D663).

```text
before:  presenting a ps5/cex/base machine
after:   presenting a prospero/cex/base machine
```

## Surprises

- **The enum variants were a data format, and the tests said so.** `Generation` derives
  `Deserialize`, so shipped machine profiles carried `generation = "ps5"` and the rename broke
  them: `unknown variant `ps5`, expected `orbis` or `prospero``. Principle 10 says no
  compatibility shims, so the data was renamed too - `prospero-cex-12.40` - rather than aliased.
  A serde alias would have been the easy fix and would have kept the trademark alive in a file.
- **My earlier warning was wrong, and the question it prompted was wasted.** I said this was a
  three-way split against a two-way enum and so a design change. `Generation x Revision` already
  picks one of four; `Platform` is derived, not a third field. Recorded because the wrong warning
  cost a round-trip.
- **Two existing tests failed, and should have.** They pinned `ps5/cex/base` and `ps5/dex/pro` -
  the behaviour they guard changed on purpose. Updated in place, same properties, new vocabulary.
- **Nothing else spoke the old words.** A workspace build was clean and a search outside the
  machine module found no `"ps4"`/`"ps5"` - the trademark had exactly one home.

## Next

- The corpus manifest's source-list fallback, still unbuilt.
- The two shell surfaces - packages and payloads - still unbuilt.
- A non-empty eboot sweep, so the conformance differential can pair like with like.
