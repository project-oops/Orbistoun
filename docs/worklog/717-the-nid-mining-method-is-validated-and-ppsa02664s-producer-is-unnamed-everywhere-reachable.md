# 717. The NID mining method is validated, and PPSA02664's producer stays unnamed everywhere reachable

**2026-09-19** — following worklog 716 (PPSA02664's wall is a null object whose producer is the unnamed
`libSceAgc::0x7d86501b8094ef57`), an attempt to name that NID clean-room by collision. The mining
method is proven; the name is not in any source reachable from here.

## The method, validated end to end

Naming a NID is a collision search: a name whose hash equals the target *is* the name, a fact
re-derivable in-tree (`orbistoun-cli nid <name>`), independent of where the candidate came from. The
full pipeline was exercised and verified at each step:

- **name → NID:** `orbistoun-cli nid sceAgcCreateShader` → `0xa644a024d860777f`, and the tool hashes
  any candidate list at once.
- **NID → encoded key:** the target `0x7d86501b8094ef57` encodes (little-endian bytes, base64 with
  `+`/`-` as the last two of the alphabet) to `V++UgBtQhn0`; verified against a known entry, whose
  encoded key the same procedure reproduces exactly.

So the machinery to name a NID by collision is sound: given a candidate that collides, or a database
keyed by the encoded NID, it resolves.

## But the datum is not reachable

`0x7d86501b8094ef57` is absent from every source available to this project:

- **The title's own files** carry no `libSceAgc` name strings at all - the engine build strips them,
  so there is nothing to harvest (the standard clean-room route, worklog 484's method).
- **Hardware** cannot supply it: the retail module's export symbol table is unmapped/protected, so
  enumerating its NID→name pairs came back not-possible (recorded against the sibling probe).
- **Cross-reference databases and other implementations** do not carry it: hashing the full set of
  known `sceAgc*` names finds no collision, and a lookup by the encoded key `V++UgBtQhn0` lands on the
  exact sort position where the entry would sit and finds it missing. It is a genuinely obscure
  `libSceAgc` function that the comprehensive lists do not name.

## Disposition

- **The method is proven and stays in the toolbox.** The moment `0x7d86501b8094ef57` appears in any
  source that names it - a firmware-derived export table, a future database, a measured probe - this
  same collision search names it with no new machinery.
- **PPSA02664's frontier remains blocked** on that one name (worklog 716): the null object in
  `CreateWorkload` is a structure that function should write, and until it is named and its write
  semantics are known, the wall stands. Inventing a name or a layout would be the plausible output
  principle 3 forbids; a collision-verified name is the only kind this project will record, and the
  candidate that collides has not been found.
- The encoded key `V++UgBtQhn0` is recorded here so a future search starts from it rather than
  recomputing it.

## Gate state

No tree change beyond this worklog - a search and a validation, no code. Identity scan clean.
