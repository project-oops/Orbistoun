# D053 - Imports use standard ELF machinery; only the names are vendor-encoded

**decided** · 2026-08-19 · implemented and verified against real material

The vendor `DT_` tags suggested a bespoke import format. They are a red herring: the
import path is **entirely standard ELF** - `PT_DYNAMIC`, `DT_STRTAB`, `DT_SYMTAB`,
`DT_HASH`, and an ordinary `Elf64_Sym` table.

What is vendor-specific is only the **encoding of symbol names**. A dynamic symbol is
called something like `H2e8t5ScQGc#B#C`: eleven base64 characters carrying the NID,
then a library id, then a module id. `DT_NEEDED` names the libraries in the ordinary
way (`libkernel.prx`, `libSceLibcInternal.prx`, and so on).

Three consequences:

- **The NID is in the name, so no hash suffix is needed to read imports.** D006 said
  NIDs need a runtime suffix - that is true for hashing *our own declarations* into
  NIDs, but an import's NID is decoded straight out of the symbol name. Import
  surveying works with no symbol database at all.
- **Vendor `DT_` tags are skipped, not rejected.** They carry information this parser
  does not need, and failing on an unknown tag would reject every real module.
- **The symbol count comes from `DT_HASH`'s `nchain`.** There is no `DT_SYMSZ`, and
  inferring the size from table adjacency would be wrong: the string table sits
  *before* the symbol table in real material.

**Verified.** 159 imports from a 76 KB module, 117 and 95 from two others, and
**1,410 from the commercial executable**, each attributed to a library. The NID
decoder is cross-checked against an independent implementation of the same rule so a
transcription slip shows up rather than being self-consistent.

**Two honest limitations, recorded rather than papered over:**

- The **byte order** of the decoded NID is little-endian, consistent with how NIDs are
  read elsewhere, but **not independently verified** - no known (name, hash) pair has
  been checked against it. Everything is self-consistent either way, so this would not
  surface until a symbol database is loaded.
- **70 of the executable's 1,410 imports have no library attributed.** Their library
  id does not index the `DT_NEEDED` list, so the id is probably relative to a separate
  library table carried in the vendor `DT_` entries rather than to `DT_NEEDED` order.
  Reported as `?` rather than guessed at.

