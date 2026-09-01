# Previous-generation container support

The loader accepting previous-generation binaries is a test-material decision, not a
product direction (see [SCOPE.md](../SCOPE.md)) - but it needs doing, because homebrew
for the current target is scarce and a loader with nothing to load cannot be
debugged.

## Housekeeping

Small, known, and deliberately not urgent. Recorded so they are choices rather than
oversights.

- **`memmap2` is declared at workspace level and used by no crate.** The other three
  this entry used to name - `serde_json`, `ash`, `directories` - all found consumers.
  `cargo-machete` does not flag workspace-level entries, so this one will not trip the
  gate on its own.
- **Public API with no consumer.** `HandleAllocator`, `GuestResult`, `Sink`,
  `CountingSink`, `StubReturn::as_raw`, `SymbolDbFile`, `Registry::len`,
  `SymbolDb::len`, `Protection`, and `Region` are declared shape awaiting their
  phase. Listed so the count can be watched going down rather than up. Two entries once on
  it, `Sink` and `CountingSink`, were the whole of a crate nothing depended on, and went
  with it (D211).
- **The provenance guard still excludes `tests/fixtures/synthetic/`**, a path that does
  not exist and will not until roadmap phase 0. Harmless, and worth deleting rather than
  carrying if phase 0 slips much further - an exclusion for a directory nobody has seen
  is indistinguishable from a mistake.
- **CI has never executed.** The workflow YAML is unverified as *CI*, though every gate
  it runs is now a `./bin/orbistoun` verb that has been run locally, which is most of the
  risk. `pages.yml` is the exception in the other direction: `./bin/orbistoun site`
  reproduces its steps exactly.
- **The macOS release artifact cannot run a guest.** `release.yml` builds
  `aarch64-apple-darwin`, where `enter_process` is `unimplemented!()` - guest x86-64 code
  cannot execute natively on ARM, and principle 12 rules out an execution-backend
  abstraction on purpose. The analysis commands (`symbols`, `imports`, `knows`,
  `questions`, `worklist`) are genuinely useful there, so the build is worth keeping -
  but it is offered on the site with no caveat, and `run` panics rather than refusing.

