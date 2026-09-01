# Symbol databases

Names orbistoun has worked out, and the record of how.

Every name in every file here is **re-derivable from this repository alone** - from the
published-standard word list in `crates/orbistoun-names/data/standard.txt`, or from the
candidate grammar in `crates/orbistoun-names/data/vendor.toml`. Each carries a
derivation saying which, and where.

**Which is not the same as saying this file could be rebuilt with no guest module.** The
generator produces 251 million candidate names and cannot know which of them name a
function that exists; a module's import table is what supplies that. The names are ours,
the selection is the module's, and the audit proves the first half - the half a
provenance question is about. See [docs/PROVENANCE.md](../docs/PROVENANCE.md).

That claim is checked mechanically, not asserted:

```bash
orbistoun-cli audit symbols/generated.json
```

It runs in `./bin/orbistoun check` and in CI over every file here, and it re-runs each
recorded derivation rather than trusting it - so a forged record fails exactly as
loudly as a missing one.

**Nothing here came from a NID database, a disassembly, or a vendor binary.** The full
argument, including its honest limits, is in [docs/PROVENANCE.md](../docs/PROVENANCE.md).

## These files are generated

Never hand-edited. Editing one would put a name in the tree with no derivation behind
it, which is exactly what the audit exists to catch.

```bash
./bin/orbistoun names
```

Sweeps every guest module under `titles/`, accumulating - each module contributes the
imports only it needs, and nothing already learned is ever dropped. The right way to add
a name is to make the generator produce it, by extending
`crates/orbistoun-names/data/vendor.toml`.

There is no schedule. Re-run it after extending the vocabulary, or after adding a module
to `titles/`. CI does not run it - `titles/` is gitignored, and it does not need to: what
CI checks is the audit, which needs no module at all.

## Files

- `generated.json` - names worked out so far, with the record of how each was arrived at.
- `wanted.txt` - hashes still unnamed, accumulated across every module ever searched.
  The work list: extending the grammar is aimed at these, and it needs no rebuild.
