# orbistoun-overrides

Per-title settings and compatibility entries, layered and merged. Title-specific behaviour
never reaches the core: there is no `if title == ...` anywhere.

It merges three layers (global, repository, user) per key, into typed values, and holds
compatibility entries labelled `quirk`, `workaround` or `unsupported`.
[orbistoun-report](../orbistoun-report/) records the resolved configuration in every run
report.

## Rules

- **Merging is per key, never wholesale.** A user file that sets a resolution does not drop
  the repository's compatibility entries for that title. Whole-file replacement produces bug
  reports that cannot be falsified; a test is named after that case.
- **Keys name the behaviour, never the title:** `raytracing_enabled`, not `title_x_rt_fix`. A
  second title needing the same thing then adds a line rather than a code path.
- **A compatibility entry carries a reason**, by construction. An entry without one is how a
  file becomes a graveyard of unexplained exceptions.
- **Every resolved value records the layer that set it**, so a run report shows effective
  configuration with provenance. An override applied invisibly is the same failure as a stub
  that lies about succeeding.
