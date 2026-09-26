# Symbol databases

Names Orbistoun has worked out, and the record of how.

Every name here is re-derivable from this repository alone - from the published-standard word
list in `crates/orbistoun-names/data/standard.txt`, or from the candidate grammar in
`crates/orbistoun-names/data/vendor.toml` - and each carries a derivation saying which (D242).

Re-derivable is not the same as rebuildable without a guest module. The generator produces far
more candidate names than exist, and a module's import table is what selects the real ones. The
names are generated here; the selection is the module's. The audit proves the first half, which
is the half a provenance question is about. See [docs/PROVENANCE.md](../docs/PROVENANCE.md).

Nothing here comes from a NID database, a disassembly or a vendor binary.

## The audit

```bash
orbistoun-cli audit symbols/generated.json
```

It re-runs each recorded derivation rather than trusting it, so a forged record fails as loudly
as a missing one. `./bin/orbistoun check` and CI run it over every file here; it needs no guest
module.

## Regenerating

These files are generated and never hand-edited: a hand-added name has no derivation behind it,
which is what the audit exists to catch.

```bash
./bin/orbistoun names
```

`names` sweeps every guest module in the title library and accumulates - each module contributes
the imports only it needs, and nothing already learned is dropped. A name is added by making the
generator produce it, by extending `crates/orbistoun-names/data/vendor.toml`. Re-run it after
extending the vocabulary or adding a module to the library. `./bin/orbistoun run` re-runs it
automatically when the names are stale. CI does not run it, because the library is not in the
repository.

## Files

| File | Holds |
|---|---|
| `generated.json` | the names worked out, each with the record of how it was derived |
| `wanted.txt` | unnamed import hashes, accumulated across every module searched; the naming work list |
| `exported-unnamed.txt` | unnamed platform export hashes - what the hardware's export table offers whether or not a title imports it |
| `proposed-learned.txt`, `proposed-tail.txt`, `proposed-verb.txt` | words a model proposed that the NID hash then confirmed |
| `unaccounted-ceiling.txt` | names this repository cannot re-derive, as a ceiling: `orbistoun-cli audit --ceiling` fails on an unaccounted name not listed, and on a listed name that has become accounted for |
