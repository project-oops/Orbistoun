# 479. The symbol level

**2026-09-09** - directed, continuing 478

Built the second half of the differential: symbol counts, import triples, relocation census.

```text
differential: 29 module(s), 0 with a disagreement, 28 differing only in units
```

## What it compares

- **The dynamic symbol count, from two independent derivations** - orbistoun from `DT_HASH`'s
  `nchain`, SELFish from `symtabsz / syment`. Agreeing is only worth something because neither
  read the other's number.
- **Every encoded import, joined on symbol index**, comparing hash and *resolved* library and
  module names. The index is the join because a relocation names its symbol by index and nothing
  else.
- **The relocation census by type**, for `rela` and `jmprel`.

All three agree, on all 29 modules.

## Three meanings read out of the source first

478's lesson applied rather than repeated: SELFish's `imports` returns encoded names only, so
plain undefined symbols are counted apart; SELFish resolves library and module to names where
orbistoun carries raw ids, so the names are what gets compared; and the NID byte order.

**The byte order is measured, not assumed**, and it had to be: SELFish's answer to
`REQ-20260909T1250Z-1f74` says orbistoun prints these reversed, while orbistoun's own
`NidHasher::hash` documents a little-endian read that sounds like agreement. Two documents,
opposite implications. The test tries both and counts which matched: every encoded import in every
module matches only reversed, no module mixing the two. A module that mixed them is a separate
reported field, not a silent majority vote (D655).

## Surprises

- **Reporting the units as disagreements said the opposite of the truth.** 28 modules "differing"
  when they agree about all of them. `Disagreement` now carries a `units` flag and the headline
  counts them apart - the line still prints, because a convention nobody wrote down is how three
  of 478's four errors happened.
- **Two vacuous passes were possible and both are now loud.** `symbol_count` returns `None` when
  `symtabsz` is zero, which would skip the comparison; an empty relocation census compares equal
  to an empty one. Neither fired on this corpus. Both were written before knowing that.
- **The doc-comment mistake again** - inserted a function above an existing one and split its
  doc comment off its item. Fourth time. Insert after the closing brace, never before a comment.

## Next

- Reported back to SELFish as `REQ-20260909T1910Z-3d05`, asking them to write the byte-order
  convention down on their side.
- The exception context at +0xf8, still open, still the wall for the furthest-reaching title.
