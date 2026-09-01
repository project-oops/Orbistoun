# 2026-08-20 - Attribution, the submit function, and honest word lists


D117-D120. 27 crates, 457 tests. Worked alongside the GPU thread; touched none of their
crates.

**Library attribution was fabricated and now is not.** Reading output caught it - a
graphics driver exporting `setsockopt` - and counting proved it: ids run to 54 against a
52-entry `DT_NEEDED`. The real table is a vendor dynamic tag holding exactly 55 entries.
Every POSIX call now resolves to `libScePosix`, which does not appear in `DT_NEEDED` at
all.

**The submit function has a name.** `sceAgcDriverSubmitDcb` and `sceAgcDriverSubmitAcb`,
which is what the GPU thread was blocked on. Reached by adding command-stream nouns to
the grammar - `Dcb`, `CommandBuffer`, `Flip`, `DrawIndex` - and searching 2.4 billion
candidates in 94 seconds.

**Supplied names stop lying about where they came from.** `--words-from observed|supplied`,
defaulting to the stricter one.

### Surprises

- **The two fixes compounded.** Fixing attribution shrank the graphics search from 1,410
  imports to 260 and made a hit distinguishable from a lucky collision elsewhere. Doing
  the vocabulary work first would have been much weaker.
- **The generator already had "Agc" and "Submit" and found nothing.** Generic breadth was
  not the shortage; domain nouns were. Worth remembering before adding more verbs.
- **A merge silently dropped a vocabulary list** written on one line, including the empty
  revision mark - which would have made every generated name carry a suffix. Caught only
  because the reported count went *down*.
- **The `Supplied` derivation had no code path that could produce it.** The category
  existed, the mechanism did not. Found by asking what obSCEne names would be labelled.
- **A search now costs 94 seconds per module rather than nine**, which turns the staleness
  check in `run` from a nicety into the thing that keeps the loop usable.

### Outstanding

The obSCEne name export needs a five-line change in *that* repo; prompt written to
`docs/design/obscene-name-export.md` rather than editing across the boundary.

