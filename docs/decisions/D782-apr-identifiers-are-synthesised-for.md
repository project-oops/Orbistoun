# D782 - APR identifiers are synthesised for app0 files outside a title's index

**Status:** decided
**Date:** 2026-10-09
**known_by:** assumed (what the console answers for a packaged file is not measurable on firmware
12.40: native homebrew has no `/app0`, so every APR probe resolved against nothing)

`sceKernelAprResolveFilepathsToIdsAndFileSizes` resolves a path its title's own index names as
before (the index's identifier and size). A path the index does not name, or any path of a title
that ships no index, resolves when it names an existing file under `/app0/`: it is given an
identifier of its own, from `0x4000_0000` upward in the order paths are first asked for, the same
one every later time it is asked, with the file's real size and status 0. Anything else still
gets the measured unresolved answer: identifier `0xffffffff`, size 0, and `-1`.

**Why:** a title resolves every file it reads through this call before reading it. PPSA25872 ships
no index, so its resolve of `/app0/Media/ScriptingAssemblies.json` was refused, Unity closed the
file unread, il2cpp found none of its core types, and the title crashed. The user approved
synthesising identifiers on 2026-10-09. A range of its own keeps a synthesised identifier from
ever meeting one an index assigns.
