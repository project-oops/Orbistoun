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

The files are read through the same identifiers. `sceAmprAprCommandBufferReadFile` adds a read -
identifier, destination, length and file offset - to the command buffer it is handed, held beside
it rather than encoded into it, and answers 0. `sceKernelAprSubmitCommandBufferAndGetResult`
carries out every read the buffer holds, in order, before it answers 0, and
`sceKernelAprWaitCommandBuffer` then has nothing to wait for and answers 0. A buffer holding no
reads, which a title's own code encoded, is still refused: its encoding is not established.
