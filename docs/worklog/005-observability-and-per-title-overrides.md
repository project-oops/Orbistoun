# 2026-08-19 - Observability and per-title overrides


Documentation only; no code touched. Final planning entry before implementation.

**Done.** D046-D048 recorded. Roadmap gained phase 0e (observability substrate) and
`orbistoun-overrides` as a fifth piece of phase 0c.

**The audit that prompted D046/D047.** Logging today is *one* call - the
`--suffix-hex` warning - and only `orbistoun-cli` depends on `tracing` at all. The
core crates cannot log, because the dependency prune removed `tracing` from every one
of them. Correct at the time (genuinely unused) and directly at odds with logging
being a first-class principle. It returns per-crate as each gains real calls.

**Surprises.**
- **The consumer is an agent reading cold**, and that single fact drives the design
  more than anything else. It is why the run report - not the log - is the contract:
  an agent grepping log prose turns messages into an unversioned API that breaks
  silently on a reword.
- **The report must be bounded to kilobytes.** A finite context cannot read a
  multi-gigabyte trace, so the report is an index with top-N and last-N and the trace
  is queried on demand. A report of "everything that happened" stalls the loop on the
  one artifact it depends on.
- **Ring-buffering the trace solves two problems at once** - the disk concern and the
  diagnostics. Successful runs write nothing; failures yield the tail, which is the
  part anyone wanted.
- **Per-title overrides and per-title user settings are the same system.** Initially
  scoped as a compatibility database sitting *alongside* user settings; collapsing
  them into one layered mechanism is strictly better. The trap is merge semantics -
  per key, never wholesale, or a user setting resolution silently drops the
  compatibility entry that made the title work.

**Next.** Planning complete. Consistency pass over the roadmap and decision log, then
phase 0 - with 0b, 0c, 0d, and 0e all available in parallel.

