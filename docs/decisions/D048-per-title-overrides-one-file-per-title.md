# D048 - Per-title overrides: one file per title, three layers, merged per key

**decided** · 2026-08-19

Title-specific behaviour never touches the core. No `if title == …` anywhere. Instead
the core reads generic, named settings, and a per-title file declares what that title
needs.

**One file per title**, not one large database. Identified by **content hash**
initially, since metadata parsing is deferred (D040) and a hash is exact and immune to
renaming.

**Three layers, user wins:** global defaults → repo per-title → user per-title.
Repo-shipped overrides are our compatibility knowledge and live in `overrides/`,
versioned and reviewable - distinct from `titles/`, which is gitignored corpus
content. **Knowledge is tracked, content is not.** User files live in the data dir, so
they follow portable mode automatically (D038).

**Merge per key, never wholesale replacement.** A user file setting resolution must not
silently drop the repo's compatibility entries for that title. Whole-file replacement
produces bug reports that cannot be falsified, and it is a known failure of
config systems shaped like this.

**Two kinds of key share the file:**

- **Compatibility keys** describe a *deviation*, never a title -
  `tolerate_unaligned_direct_memory`, not `gta_alignment_fix`. This is what lets a
  second title needing the same behaviour reuse the key instead of adding a case.
- **Preference keys** are ordinary settings scoped per title - `resolution_scale`,
  `raytracing_enabled`. Only the first kind needs a stated reason when we ship it.

**Prefer typed overrides to boolean flags.** Booleans multiply
(`tolerate_unaligned_alloc`, `tolerate_tiny_alloc`, …) where a typed value generalises
(`direct_memory_alignment = 4096`). Fewer keys, reads as configuration rather than a
list of exceptions.

**Compatibility entries carry one of three labels**, because each resolves differently:

- **quirk** - the title genuinely does something out-of-spec that real hardware
  tolerates. Legitimate and permanent; nothing to fix.
- **workaround** - *our* implementation is wrong and this masks it. Temporary, a
  reason is required, and it is deleted when the bug is fixed.
- **unsupported** - a capability we have not built yet. Deleted when the feature
  ships, and aggregates into a feature-level work list: *"14 titles blocked on
  raytracing"* is the phase-1b prioritisation applied above function level.

Without that split the file becomes a place where every bug we did not fix acquires a
respectable name and turns permanent and invisible. A workaround should be
uncomfortable to look at, and answerable by a "what are we papering over" report.

**Worked example**, since the abstract rule is easy to misapply. A commercial title
fails on something we cannot support, and we know it proceeds if that thing is
skipped:

```toml
# overrides/<title-hash>.toml
[compat]
raytracing_enabled = { value = false, kind = "unsupported",
                       reason = "no RT pipeline yet; title proceeds without it" }
```

The core reads `raytracing_enabled` and skips the work. **The title is named nowhere
in the code** - only a clear toggle whose name describes the capability. A second
title needing the same thing adds a line to its own file and no code changes at all.

Note the key is `raytracing_enabled = false`, not `disable_raytracing = true`.
Negative booleans acquire double negatives the moment something must be forced on
(`disable_raytracing = false` reads terribly), and a positive-sense key extends
cleanly if the toggle later becomes an enum. Same reasoning as preferring typed
overrides above.

**Applied overrides are never silent.** Each emits a structured WARN and appears in the
run report with per-key provenance (D046) - otherwise behaviour is being diagnosed
against a configuration that cannot be seen, which is the D008 problem again.

Confirmed 2026-08-19: **file-only initially**, with GUI per-title properties arriving
at phase 2b rather than blocking it; and the corpus survey **records which overrides
each title needs**, so the dependency is answerable in both directions and can become
a regression assertion.

**Identity hashes the executable, not the directory** (amended 2026-08-19 after
inspecting real material). A title directory is around 96 GB; hashing that per run is
a non-starter. The executable is both small enough (tens of MB) and the right thing
semantically - it is what changes when a title is patched.

