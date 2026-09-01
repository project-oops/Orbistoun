# 2026-08-19 - Test corpus and the accuracy suite


Documentation only; no code touched.

**Done.** D042-D045 recorded. Corpus tooling (`titles/`, manifest-driven, pinned by
hash) and the accuracy suite decided as a **separate repository, `obSCEne`**, with its
result model and coverage strategy specified. Roadmap gained phases 0d (corpus
tooling) and 1b (corpus-wide survey report). `titles/` added to `.gitignore` and to
the provenance guard's exemptions in all three places it is enforced.

**Surprises.**
- **No accuracy suite exists for this target.** Checked rather than assumed - nothing
  comparable to `pspautotests`, blargg's ROMs, or `dolphin-emu/hwtests`. shadPS4 has
  host-side unit tests, which is a different thing entirely. That is why we are
  building one.
- **A test app we compile ourselves is a real container with clean provenance**, which
  partly dissolves phase 0's problem: genuine vendor-format input for the parser
  without anything of anyone else's in the repo. Synthetic fixtures are still needed
  for malformed cases a compiler will never emit.
- **The open toolchain is a lawful source for interface facts** - names and arities -
  because it is open source. That converts two "someday" items (D025's name list, the
  37 provisional arities) into tractable ones. The line is D044: interface facts yes,
  implementations never.
- **The reporting channel is itself under test.** A suite reporting via stdout says
  nothing when stdout is unimplemented. Resolved by trace-as-report: orbistoun already
  records every call, so calling interfaces in a known order *is* the result, with
  beacon calls (sentinel argument values) as trace delimiters. Works with zero I/O.

**Next.** Unchanged - phase 0, with 0b, 0c, and 0d available in parallel. `obSCEne`
awaits scaffolding.

