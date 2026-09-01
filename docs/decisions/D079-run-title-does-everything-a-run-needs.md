# D079 - `run <title>` does everything a run needs

**decided** · 2026-08-19 · at the user's direction

`run` was a raw passthrough to the binary, so debugging a title actually meant
`./orbistoun.sh run run titles/PPSA04263-app0/eboot.bin` - a doubled verb and a path
nobody should have to know. It is now the one command a person needs while developing:

```bash
./orbistoun.sh run PPSA04263
```

Resolve the id to a module, rebuild, refresh names if stale, run under a time limit,
report what it asked for. **Everything static a run depends on happens here** rather
than in a checklist somebody has to remember, because the step people forget is always
the one that makes the output quietly wrong rather than absent.

Takes a title id because that is what a person has. They know which title they are
chasing, not where its executable sits in the layout. With no argument it lists what is
available; a full path also works.

**Names are refreshed only when stale** - when the grammar, the word list, or the module
is newer than the database. Searching 251 million candidates takes ten seconds per
module, and paying that on every debug run of an unchanged tree would make the fast path
slow enough that people start skipping it. Then they debug against stale names, which is
worse than either outcome the check was choosing between.

The raw passthrough moved to `cli`, which is what it always was.

