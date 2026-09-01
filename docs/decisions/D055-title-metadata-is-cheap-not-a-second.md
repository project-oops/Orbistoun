# D055 - Title metadata is cheap, not a second container format

**decided** · 2026-08-19 · observed from real material

The backlog recorded display names and icons as expensive - "parsing a second
container format entirely". That is wrong for this generation.

`sce_sys/` contains **`param.json`** (plain JSON) and **`icon0.png`** (a plain PNG),
alongside `pic0`/`pic1`/`pic2`, trophy data, and a menu sound. Real names and icons in
the library view cost roughly a `serde_json` call and an image load - not a parser.

Consequence: D040's filename-first library, assumed on cost grounds, is worth
revisiting sooner than planned. The GUI shell at phase 2b can plausibly ship with real
titles and icons rather than filenames.

