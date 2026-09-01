# Title metadata for the library view

**Cheaper than recorded** - see D055. `sce_sys/param.json` is plain JSON and
`sce_sys/icon0.png` a plain PNG, so real names and icons cost a `serde_json` call and
an image load, not a parser. D040 assumed filenames-first on cost grounds that turned
out not to apply, so this is worth pulling into the phase 2b GUI shell rather than
deferring.

