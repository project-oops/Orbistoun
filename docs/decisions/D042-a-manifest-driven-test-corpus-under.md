# D042 - A manifest-driven test corpus under `titles/`

**decided** · 2026-08-19

Tooling in-repo downloads a predefined list of homebrew into a **gitignored
`titles/`** directory. Users drop their own material in the same place.

Named `titles/` deliberately: `games/` is banned by our own provenance guard
(`(^|/)(firmware|dumps|games)/`) and would trip it the moment anything got tracked.

**`titles/` is NOT exempt from the provenance guard**, and that correction matters.
It was initially exempted on the reasoning that the corpus lives there so the guard
should ignore it. That is backwards: the guard inspects the *index*, and corpus
content is gitignored, so nothing under `titles/` should ever be tracked. Anything
that appears there is precisely the failure worth catching - and the exemption made
the second line of defence blind to the one directory it most needs to watch.

Verified by staging a guest binary with `git add -f`: with the exemption the guard
passed, without it the guard fails with exit 1. Only `tests/fixtures/synthetic/`
remains exempt, since those are deliberately committed and may legitimately carry
banned extensions.

Two independent layers, and they catch different things:

1. `.gitignore` (`/titles/*` with a `!/titles/README.md` exception) stops content
   being staged at all. Note the idiom - `/titles/` with a trailing slash makes git
   skip the directory outright, so the negation would never be reachable.
2. The guard fails CI and blocks a push if anything of that shape is tracked
   *anywhere*, catching a force-add that bypassed layer one.

The guard is the control; the ignore rule is the convenience.

**Two tiers, because they serve different purposes:**

- **Ours** - test apps whose source lives in the suite repo (D043). Small, targeted,
  one behaviour each, fully under our control.
- **Third-party homebrew** - breadth. Does anything real get further this week than
  last week.

**Reproducibility constraints, all learned-in-advance rather than the hard way:**

- Pin by commit or release-asset hash, **never a branch** - otherwise "reproducible
  on any dev machine" is false within a month.
- Prefer **prebuilt release assets over clone-and-compile**. Far more reproducible,
  and it means not every dev needs a cross-compiler installed.
- Record **licence per manifest entry**. Downloading is not redistributing, which
  keeps this simple, but the field should exist from the start.
- **CI does not fetch on every run.** Cache, or a small pinned subset. A red build
  caused by somebody else's repo moving is corrosive.

Lives in an `orbistoun-corpus` crate, not in CLI logic (D034).

**Dev-mode library ingestion is not a special case.** `titles/` is simply another
configured library path that dev builds add by default - same ingestion, different
config, no branch in the GUI to maintain.

