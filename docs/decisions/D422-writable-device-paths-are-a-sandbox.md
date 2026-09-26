# D422 - A title's writable device paths are modeled as a per-title sandbox, not a special case

**Status:** decided
**Date:** 2026-08-31

A guest's writable device paths (its per-title save area and mounted removable-storage
stand-ins) are layers in the existing overlay filesystem, not a new subsystem; guest writes to
them persist across runs by default.

**Why:** The overlay filesystem already builds a read-only base tree with a writable per-title
layer over it, so the console's sandboxed device paths are additional writable mount entries in
that same mechanism. Persisting a title's writes by default keeps the saves and reports a run
produces available to the next one; a caller that wants a clean slate selects ephemeral
retention instead.

**Rejected:** special-casing the one call site that first hit this - the same shape recurs at
every writable path a guest opens.
