# 2026-09-02 - (/loop) Exports exist now, and the title's own module answers the wall

```
tests   1957  ->  1960
```

`orbistoun-elf` could read what a module *needs* and had no notion of what it *provides*. That
is now `RawExport`, `exports_from_symbols`, `Container::raw_exports`, `Service::exports_path`
and an `orbistoun-cli exports` verb - and pointing it at PPSA02664's own module settled the
question the loader work exists to answer.

## The measurement that matters

`Il2CppUserAssemblies.prx` exports **247 symbols**. The eboot imports four NIDs from that
library. All four are in the export table:

```text
0x9162adc3c86893df  FOUND
0x9fdbed4fe0989d70  FOUND
0x6f8b9da539afc9af  FOUND      <- the NID PPSA02664 is stuck on
0x00e9dafedfe399fe  FOUND
```

**`0x6f8b9da539afc9af` is the wall.** It is answerable by a file that has been sitting in
`titles/PPSA02664-app0/Media/Modules/` the whole time. Nothing about it was unknown; nothing in
the tree could read it.

## What the reader is, and the one judgement in it

Imports and exports are the same dynamic symbol table read from opposite sides of `SHN_UNDEF`,
so `symbol_tables` locates it once and both walks share it. Duplicating thirty lines of table
location is how the two would eventually read different tables.

An export carries what an import has no room for: **where it lives, as an offset from the
module's own base**. Not an address - nothing is placed when this is read, and the loader adds
the base.

**Binding and visibility are deliberately not consulted**, and that is the judgement. A local
or hidden symbol is unreachable in a real linker, and a general ELF reader should filter on it.
This is not reading generally: the guest's imports name NIDs, and the only question is whether
this module answers one. A spare row costs nothing; a symbol wrongly filtered out is an import
that never binds and a guest that stops - which cannot be diagnosed from outside. It errs the
other way and the code says so.

## The encoding, which the tests pin

All 247 are vendor-encoded (`CRJcH8CnPSI#I#A`), not names. So the NID has to be **decoded from
the spelling**, not hashed from it - hashing would produce 247 NIDs no importer asks for. The
import path already had that logic, so exports reuse `decode_symbol_name` rather than growing a
second decoder, and a test asserts the two answers differ.

Three unit tests, all running without any title data: imports and exports are complementary
across one table, an export carries its offset, and an encoded name publishes the NID it
contains rather than a hash of its spelling.

## State

`cargo test --workspace` green - **117 suites, 1960 tests**, 0 failures. clippy `--tests`
clean, fmt clean, identity scan clean.

Nothing committed. The day holds worklogs 292-327 and D466-D481.

**Next is D482, the load order**, and it now has a concrete shape rather than an open question:
the title's modules must be placed and relocated **before** the main executable, and their
exports fed to the resolver. The decisions inside it are which module places first when one
title module imports from another, how the case-variant library name
(`Il2cppUserAssemblies`, four more imports) matches, and where modules are discovered -
`Media/Modules/` is the title's own choice, not a platform path, so it cannot be hardcoded.
