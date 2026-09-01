# orbistoun-overrides

Per-title settings and compatibility entries, layered and merged. Title-specific
behaviour never reaches the core - there is no `if title == …` anywhere.

**Models:** three layers (global → repo → user) merged **per key**, typed values, and
compatibility entries labelled `quirk` / `workaround` / `unsupported`.

**Deliberately fakes:** nothing.

**Design note.** Merging is per key and never wholesale. A user file that sets a
resolution must not silently drop the repo's compatibility entries for that title -
whole-file replacement produces bug reports that cannot be falsified, and is a known
failure of config systems shaped like this. There is a test named after exactly that.

Keys name the *behaviour*, never the title: `raytracing_enabled`, not `gta_rt_fix`.
That is what lets a second title needing the same thing add a line rather than a code
path. A reason is mandatory on compatibility entries by construction - an entry
without one is how a file becomes a graveyard of unexplained exceptions.

Every resolved value records the layer that set it, so a run report can show effective
configuration with provenance. An override applied invisibly is the same failure mode
as a stub that lies about succeeding.

**Status:** complete.
