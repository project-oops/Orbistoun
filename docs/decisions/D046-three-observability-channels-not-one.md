# D046 - Three observability channels, not one

**decided** · 2026-08-19

Conflating these is the classic mistake, so they are named separately:

1. **Developer log** (`tracing`) - what the emulator is doing. Human-facing, moderate
   volume, console plus rolling file.
2. **Guest call trace** - every guest call. Enormous volume,
   binary, allocation-free (D018). **Never `tracing`** - a log call per guest call
   would dominate the profile and change what it measures.
3. **Run report** - a structured, versioned, machine-readable artifact per run. This
   one did not exist and is the one the iterative loop actually needs.

**Do not make an agent parse logs.** If the consumer greps for "unresolved import",
rewording that message silently breaks it - log prose becomes an unversioned API. Logs
stay human-facing; **the run report is the contract.**

**The report's consumer is an agent reading cold**, with no memory of the session that
produced the code. That single fact drives its design:

- **The diff against the previous run of the same title** is the most important field.
  One run says what happened; the delta says whether the last change helped. Without
  it every session begins by re-deriving state it should have been handed. This is the
  one-bit oracle from TESTING.md, made structured and automatic.
- **First-touch ordering as well as frequency.** The *first* unmet need is usually the
  cause; everything after is cascade.
- **Absent versus wrong**, mirroring D045's grey/red split - different fixes.
- **The failure tail**: last N calls with arguments, call sites, thread ids, sequence
  numbers, and what each stub returned.
- **Effective configuration with per-key provenance** (D048), so behaviour that came
  from an override is visible rather than mysterious.
- **Its own inputs embedded** - title hash, stub-policy hash, override set, binary
  version and commit. Otherwise a difference between runs cannot be attributed to the
  change rather than to config drift, and the loop chases ghosts. This makes each
  report a reproducible experiment record.

**The report must be bounded - kilobytes, not gigabytes.** A finite context cannot
consume a multi-gigabyte trace. The report is an *index*: top-N, last-N, with the
trace queryable on demand for specific ranges. A report of "everything that happened"
stalls the loop on the one artifact it depends on.

