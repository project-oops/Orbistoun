# D039 - Configuration formats: TOML and JSON; no database

**decided** · 2026-08-19

The SQLite in another project of mine is an edge case of that project, not a
pattern to copy. Nothing
here wants a database.

- **TOML** for anything a human edits - stub policy (D008 makes hand-editing the
  working loop, so readability is a requirement) and settings.
- **JSON** for the symbol database, where the input is a large flat list and is what
  any third-party source arrives as.
- **Binary** for traces (D018). A recording format, not config, and the one thing
  that must never be text.

The only place a database could later earn its keep is querying large traces. The
plan there is append-only binary plus a sidecar index, which keeps recording
allocation-free; SQLite is the fallback if querying outgrows it, not a starting
assumption.

