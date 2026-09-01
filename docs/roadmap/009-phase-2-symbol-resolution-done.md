# Phase 2 - Symbol resolution *(DONE)*


`orbistoun-nid`. Two halves:

- **Load a database** (`SymbolDbFile` - suffix plus names) from disk, retiring
  `--suffix-hex` as the only way to supply a suffix.
- **Generate candidate names** per D025. The naming convention is regular, so a
  generator hashed against the NIDs a real module actually imports gives proof by
  collision - self-verifying, and requiring no vendor binary be read. This is what
  removes the dependency on anyone else's database.

**Observable result:** most imports stop printing `<unknown>`, and the unresolved
count becomes a number worth driving down rather than an artefact of having no name
table.

