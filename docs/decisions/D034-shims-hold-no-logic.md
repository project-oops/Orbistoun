# D034 - Shims hold no logic

**Status:** decided
**Date:** 2026-09-26

The CLI, the GUI and the worker are interaction shims; logic lives in the crates, with
`orbistoun-service` for shared orchestration. Run comparison, ranking and report rendering live
in library crates, and a shim prints what they return.

**Why:** two shims computing one answer separately come to disagree about it. A seam with one
consumer is a claim; a second shim is how a leak of logic into the first is found.

**Rejected:**
- Orchestration in the CLI: the GUI would reimplement it.
- Rendering per shim: one measurement described two ways.
