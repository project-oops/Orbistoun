# D307 - Import kind is read from the symbol type

**Status:** decided
**Date:** 2026-08-26

Each import carries its kind from `st_info` - function, data or unspecified - through
`RawImport`, the survey, the wire record and the report. `orbistoun-proto` declares its own
`ImportKind` and converts at the service boundary.

**Why:** a data import given a function thunk reads instruction bytes as data and carries on
without faulting, so the loader must know what the guest wants in the slot. `STT_NOTYPE` is
common in vendor modules, and folding it into "function" would assert what the table never
said. The wire crate depends on serde only.

**Rejected:**
- Inferring the kind from the name: the table states it.
- Defaulting unspecified to function: invents a fact.
- Re-exporting the parser's type from the wire crate: couples the protocol to the parser.
