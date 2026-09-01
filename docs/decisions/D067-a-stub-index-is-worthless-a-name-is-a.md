# D067 - A stub index is worthless; a name is a work list

**decided** · 2026-08-19

`RawImport` now carries its **dynamic symbol index**, because that number is what links
a name to everything numeric: relocations index the symbol table, and so does the stub
table. Without it a trace can only say "import 260", which is a fact about the loader
rather than about the guest.

Labels are built per symbol index, and symbols that are not imports get an **empty
label rather than being omitted** - omitting them would shift every index after the gap,
which is exactly the off-by-many that makes a trace confidently wrong.

An unnamed import is reported as `library::0x7dd1e10c2d2e7a04` rather than as a bare
index. The hash is stable across builds and searchable, and the library says which
subsystem to look in - so even with no symbol database at all, the trace names something
actionable.

