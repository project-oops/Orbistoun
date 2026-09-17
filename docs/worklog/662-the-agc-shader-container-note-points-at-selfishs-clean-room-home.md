# 662. The AGC shader container note points at SELFish's clean-room home

**2026-09-17** - `libSceAgc.toml`'s `sceAgcCreateShader` layout deferral now names SELFish's
hardware-confirmed container table instead of obSCEne's disassembly, and the `0x18` header field is
corrected to `version`; the collection-wide dedup SELFish asked for (inbox `-3e88`), unblocked by
obSCEne `9f4c` landing

## Why now

`-3e88` was accepted in September and held: orbistoun agreed to point its `sceAgcCreateShader` layout
notes at SELFish's `data/agc-shader-format.tsv` once that table was hardware-confirmed, rather than at
obSCEne's disassembled copy which is being retired. The confirmation is obSCEne `9f4c`, a differential
AGC-shader probe that verified the container layout from clean bytecode; SELFish consumed its verdicts
into the table. So the deferred edit is now the honest one.

## What changed

`crates/orbistoun-hle/data/knowledge/libSceAgc.toml`, `sceAgcCreateShader`:

- The layout deferral - "the exact argument layout ... should be read from obSCEne's 166-agc source" -
  now names **SELFish's `data/agc-shader-format.tsv` (SELFish D103)**, the collection's clean-room home
  for the AGC shader container (derived from five open-source emulators, hardware-confirmed by `9f4c`),
  "rather than obSCEne's disassembly (which is being retired)".
- The `0x18` after the magic, previously "taken to be the header's own size by analogy ... and is
  unverified" (D083), is corrected to the header's **`version` at +0x04** - what SELFish's table
  establishes it is.
- Orbistoun's own **measured** fault-observations stay untouched: the `+0x30`/`+0x50` reads and the
  object model are orbistoun's, and they are measured, so they keep their place.

This is a provenance move (principle 1): the pointer goes from a dirty source (a vendor-binary
disassembly) to a clean one (emulator-derived, hardware-confirmed), which is exactly the direction the
boundary wants.

## Gate state

`cargo test -p orbistoun-hle` green (the knowledge parses, `the_shipped_files_parse_and_carry_real_content`
passes); identity scan exit 0 - the `selfish/data/...` references are collection-relative, no machine
path. No commit.
