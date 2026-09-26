# D292 - The knowledge merge rule lives in orbistoun-hle

**Status:** decided
**Date:** 2026-08-26

`Record` and `KnowledgeFile::merge` live in `orbistoun-hle::knowledge`: a field is replaced
only when given, lists append without duplicates, and a behaviour claim without a stated
source is refused. `merge` returns provenance faults as a list; shims only locate, read, write
and print.

**Why:** the crates are the emulator and a shim holding logic drifts from the other shims.
Both the `learn` command and the measurement loop record findings, and two copies of the merge
would soon disagree about the refusal that is its point. The caller decides whether a fault
refuses input or declines to record, so it is a list, not an error.

**Rejected:**
- The merge in `orbistoun-cli`: a second caller would copy it.
- A mirror of the provenance vocabulary in the dispatcher: two vocabularies for the one thing that must be checkable.
