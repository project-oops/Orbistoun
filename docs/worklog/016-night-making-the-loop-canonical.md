# 2026-08-19 (night) - Making the loop canonical


The question was whether a fresh agent on a fresh machine could be handed one command
and get on with it. Nearly - and the gaps were worth finding. D079-D081; 273 tests.

- `./orbistoun.sh run <title>` is now the single command: resolve a title id, rebuild,
  refresh names only if stale, run, report, and say whether it went **further than last
  time**.
- `./orbistoun.sh doctor` says whether a machine can do the work, with install commands.
  `run` and `names` invoke a silent version of it first.
- `CLAUDE.md` opens with three commands instead of pointing at eighty decisions.

### Surprises

- **The loop had no objective function, and I had not noticed.** It said what a guest
  wanted and nothing about whether a change helped. That is the difference between
  iteration and repetition, and no amount of documentation would have fixed it - it
  needed a number. The faulting instruction pointer is that number.
- **`run` was unusable as a verb.** It was a raw passthrough, so debugging a title meant
  `./orbistoun.sh run run titles/PPSA04263-app0/eboot.bin`. Nobody had typed it because
  nobody had tried to use it the way a person would.
- **A grep-based count in `doctor` was silently double-counting**, reporting 528 names
  for a 264-name database, because derivations sit at the same indentation as names.
  Dropped rather than fixed: a number that is quietly wrong is worse than no number.
- **Always-rebuild would have been the wrong default.** Ten seconds per debug run on an
  unchanged tree is enough that people route around it, and then they debug against
  stale names - worse than either option the staleness check was choosing between.

### Outstanding

Implementing a function is still work, not a command. The loop identifies the wall,
measures whether it moved, and keeps the record; it does not write the implementation,
and nothing here should pretend it will.

