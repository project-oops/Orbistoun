# 2026-08-27 - The harvest became a command, and writing it twice found a bug


D352's 911 constants came from a throwaway script that existed nowhere afterwards, failing
the standard `docs/REFERENCES.md` sets: *"anyone should be able to fetch the same material
and follow the same reasoning to the same table."*

`orbistoun-gen constants <checkout> --revision <commit>` (D353). Not a new shape -
`orbistoun-gen` is already one module per generated table. The source path becomes an
argument, which mattered within the hour: the script hardcoded `<clones>/freebsd-src`, and
everything moved under `OOPS/` shortly after.

### The bug

The command found **911** where the script found **903**. Eight definitions with comments
running onto the next line - the script's regex needed the comment to close on the same
line, so it rejected the whole definition rather than the comment. `AT_EACCESS = 0x0100`
and `IP_MULTICAST_IF = 9` were simply missing, with nothing saying so.

A harvest that silently drops what it cannot parse produces a table that **looks complete**,
and no one can tell a constant absent because it does not exist from one absent because the
extractor tripped. Found only because two implementations disagreed.

Then the fix had the same shape one level down: those eight comments end mid-sentence, and
`AT_EACCESS` read as *"Check access using effective user"* - a fragment shaped like a
sentence. They end in `...` now.

### Kept

Writing a generator twice and diffing the output is cheap and found something nothing else
would have. Worth doing wherever a table is harvested rather than derived.


