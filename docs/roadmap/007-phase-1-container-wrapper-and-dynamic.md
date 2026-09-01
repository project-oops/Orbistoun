# Phase 1 - Container wrapper and dynamic segment *(DONE)*


`orbistoun-elf`. Two pieces, the first of which was discovered by inspecting real
material rather than anticipated (D049):

- **The outer wrapper.** Real containers are not plain ELFs. Magic `54 14 f5 ee`, with
  the inner ELF beginning at a header-stated offset - observed at 416 across every
  file checked, and **derived from the header rather than hardcoded**, per D010. The
  existing ELF64 parsing then runs against the inner image.
- **The vendor dynamic-link data**: import table, library list, relocation entries.

Start with `sce_module/libSceJobManager.prx` at **76 KB** rather than a 68 MB
executable - same format, same wrapper, trivially inspectable.

**Delivered.** `orbistoun-cli imports <file>` prints a real import list with an
unresolved count, and `orbistoun-cli inspect <file>` reports container structure. On
real material: 159 imports from a 76 KB module and **1,410 from a commercial
executable**, each attributed to a library.

The wrapper decode is D049 and D052; the import path is D053 - which turned out to be
standard ELF machinery with vendor-encoded *names*, far less bespoke than the vendor
`DT_` tags suggested.

