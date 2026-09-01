# D075 - Standard-library names are harvested from FreeBSD, not remembered

**decided** · 2026-08-19 · prompted by the user

The published-standard word list was ~470 names written out from knowledge of ISO C and
POSIX. Correct as far as it went, and the weakest link in the provenance argument:
"somebody wrote these down from the standards" cannot be audited, and it is bounded by
what one person thought of rather than by what exists.

FreeBSD publishes the answer. Every `Symbol.map` in its source tree lists what one
library exports, grouped by version - authoritative, permissively licensed, and citable
to a revision. `orbistoun-cli harvest <freebsd-src> --revision <tag>` reads them and
writes a word list whose header names the source it came from.

A sparse checkout is enough, since only the maps are read:

```bash
git sparse-checkout set lib/libc lib/libthr lib/msun lib/libutil
```

**What is skipped, and why.** Private version blocks (`FBSDprivate_*`) are implementation
detail rather than interface. Reserved names - anything leading with an underscore -
belong to the implementation's own namespace. Linker-script syntax (`local:`, `*;`) is
not a symbol, and treating it as one would put nonsense in the candidate list.

**A malformed map costs its own symbols, not the run.** A harvest that fails wholesale
because one file uses an unfamiliar construct is worse than a partial one that reports
how many maps it read.

The hand-written list stays until somebody runs the harvest, and now says so in its own
header. It is correct; it is simply smaller than it should be, and cited to the wrong
kind of source.

