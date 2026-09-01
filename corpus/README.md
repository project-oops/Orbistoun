# corpus/

**Where the test guests come from.** This directory holds one tracked file, `sources.toml` -
a manifest of *sources*, metadata only. The guests themselves are never here and never tracked:
`corpus sync` downloads them into gitignored `titles/`, and the provenance guard fails CI if any
guest bytes are committed anywhere (D042).

That split is the whole point. The manifest says *where a guest came from, under what licence, and
at which hash*; the bytes stay off the repo. Downloading is not redistributing, and a hash pin is
what keeps "reproducible on any machine" true past a month - a moving branch is not (D042).

## The manifest

```toml
[[source]]
name    = "ps5-payloads-mirror"      # the directory under titles/ this source lands in
kind    = "github-release"           # or "local"
repo    = "itsPLK/ps5-payloads-mirror"
tag     = "payloads-mirror"          # a tag, never a branch
licence = "per-project (mirror); each payload keeps its upstream author's licence"
cite    = "https://github.com/itsPLK/ps5-payloads-mirror/releases/tag/payloads-mirror"

  [[source.asset]]
  file   = "elfldr_v0.26.elf"
  sha256 = "…"                        # filled by the first `corpus sync` (pin-on-fetch)
```

A **`local`** source has no `repo`/`tag`; it has a `path` relative to the repo root and a `todo`:

```toml
[[source]]
name = "obscene"
kind = "local"
path = "../obscene/build"            # a sibling checkout's build output
todo = "migrate to a github-release once obSCEne publishes one"
```

`local` is for a project of ours that has no published release yet (D043). Its bytes are a dev
snapshot, re-hashed every sync rather than verified against a pin, because they change on rebuild.
A `github-release` asset is **verified** against its pin every fetch; a changed pin under a fixed
tag stops the sync.

## The verbs

```
orbistoun-cli corpus list                    # every source and whether each asset is pinned
orbistoun-cli corpus sync                     # fetch + pin/verify into titles/
orbistoun-cli corpus run --profile ps5-cex-12.40   # sync, run each guest, record to compat/
```

`corpus run` turns the ordinary `run` loop over every guest, so each records a `compat/<name>.toml`
exactly as a hand-run would - one report per guest, keyed by its own name (each guest is fetched
into its own directory so the record does not collide). This is the breadth signal D042 names:
*does anything real get further this week than last week*, regenerated on demand.

## A note on what the records currently say

`corpus run` records the **honest default-entry baseline**: no handoff diagnostic, because an
intervened run is a fact about the intervention rather than the title (D227), and only a
non-intervened run is recorded. Today the elfldr-style payloads therefore record `outcome = "0x1"`
- they are entered with argc, the way any program is, and an elfldr payload wants a handoff block
instead. When that handoff becomes the *default* entry for a payload-shaped guest under a console
profile (not a diagnostic), these records will differentiate on their own. See D411.

The bytes live in `titles/`; the knowledge lives here and in `compat/`. That is what lets a
finding travel when the guest cannot.
