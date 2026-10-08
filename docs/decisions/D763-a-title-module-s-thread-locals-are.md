# D763 - A title module's thread-locals are served by module id

**Status:** decided
**Date:** 2026-10-08
**known_by:** published (the ELF thread-local storage ABI's dynamic models)

The executable's thread-locals are module 1, whose block sits below the thread pointer, where its
`TPOFF64` relocations reach it. The modules a title ships have their own blocks. The title's
`index`th module is module `index + 2`. Its local `DTPMOD64` relocations write that id, and its
`DTPOFF64` relocations write their addend. A `TPOFF64` in one is deferred, because its block is not
below the thread pointer.

`__tls_get_addr(&{module, offset})` answers the calling thread's address of the variable. For
module 1 that is the thread pointer, less the executable's block size, plus the offset. For any
other module it is that thread's block for the module plus the offset. The block is reserved the
first time the thread asks, in its own arena at `0x6C00_0000_0000`. It starts with the module's
`.tdata` as relocation left it, and its `.tbss` reads as zero. An index naming no module answers
null.

**Why:** relocation answered module 1 for every module, so the title's own `libc.prx` (PPSA02664)
was told its variables were the executable's. Its `__tls_get_addr` calls answered the
placeholder, which it then dereferenced.

**Rejected:**
- Folding title modules into the executable's static block: their offsets are relative to their
  own block, and the static block's size is fixed when the main thread starts.
- Dense ids counted over the modules that declare thread-locals: an id only has to be unique, and
  counting needs state the link loop does not otherwise carry.
