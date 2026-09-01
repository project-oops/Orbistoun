# An implemented function's name has no auditable derivation


The name search records a derivation for every name it works out - and it only ever *sees*
imports nothing can already name. Once a function is implemented, the registry resolves it,
so it leaves the unnamed set and no search records anything about it again.

The consequence is quiet and worth stating: **57 of the 95 declared functions have no
provenance record at all.** The other 38 have one only by luck of ordering - a search
confirmed them *before* anything implemented them, and `write_symbol_db` never removes a
record once written. The 57 arrived the other way round: somebody worked out the name and
declared it in the same step, so the search never targeted it and nothing recorded anything.

So `orbistoun-cli audit` covers the names we have not yet acted on, and covers the ones we
have only where the order happened to favour it. The knowledge base has been papering over
that with a hand-written `found_by`, which is why the field drifted into eleven
contradictions before anything checked it (D213).

**The cheapest fix is probably not a record at all.** PROVENANCE.md is explicit that the
audit "proves derivability, not history" - so for a declared name, hashing it and sweeping
the space answers exactly the claim the audit already makes, with no reconstruction and no
pretence of a contemporaneous record. One sweep covers all 95 at once, the same way
`--repair` does, because a wider target set costs a sweep nothing. Roughly ninety seconds
for the whole declared set, which is a `names`-time check rather than a per-commit one.

Two ways out, and the first is probably right:

- **Record a derivation when a name is first confirmed, and keep it** even after the
  function is implemented. Costs nothing at search time; the difficulty is that the names
  currently implemented were confirmed before anything was keeping the record, so a
  one-off `--deep` walk would be needed to reconstruct them - which is exactly the "a
  provenance record assembled afterwards is a reconstruction" problem, and should be
  recorded as such rather than dressed up.
- **Have the knowledge base stop carrying `found_by`** and read it from the symbol database,
  accepting that implemented functions show nothing. Simpler, and loses the only record
  those names have.

