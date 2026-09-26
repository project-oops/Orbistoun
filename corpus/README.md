# corpus/

Where the test guests come from. `sources.toml` is a manifest of sources - metadata only. The
guests themselves are never in the repository: `corpus sync` fetches them into the title library
(`orbistoun-cli paths` prints where it is), and the provenance guard fails CI if guest bytes are
committed anywhere (D042).

The manifest records where a guest came from, under what licence and at which hash. Downloading
is not redistributing, and a hash pin under a fixed tag keeps a run reproducible on any machine.

## The manifest

```toml
[[source]]
name    = "payloads-mirror"          # the directory the source lands in
target  = "payloads"                 # titles (default), payloads, packages or staged
kind    = "github-release"           # or "local"
repo    = "<owner>/<repository>"
tag     = "<release-tag>"            # a tag, never a branch
licence = "per-project; each payload keeps its upstream author's licence"
cite    = "<release URL>"

  [[source.asset]]
  file   = "<asset>.elf"
  sha256 = "..."                     # filled by the first `corpus sync` (pin on fetch)
```

`target` chooses the library root:

| `target` | Holds |
|---|---|
| `titles` | installed titles: a directory each, with `param.json`, `eboot.bin` and their own files |
| `payloads` | raw executables, run directly rather than installed |
| `packages` | installable packages, before anything installs them |
| `staged` | titles staged on the user partition (the library's `data/homebrew` tree), run with a writable `/app0` |

A `local` source has no `repo` or `tag`. It has a `path` relative to the repository root, usually
a sibling project's build output, and an optional `todo`:

```toml
[[source]]
name   = "obscene"
target = "titles"
kind   = "local"
path   = "../obscene/build"
todo   = "migrate to a github-release once obSCEne publishes one"
```

A `local` source is for a collection project with no published release. Its bytes are a
development snapshot, re-hashed on every sync rather than verified against a pin. A
`github-release` asset is verified against its pin on every fetch, and a changed pin under a
fixed tag stops the sync.

A source may instead list `sources`, origins tried in order (each a local path or a URL, the
first that answers wins), with every failed origin reported. With `archive = true`, each origin
names a `.zip` of a whole title directory, unpacked into `<root>/<name>/`.

## The verbs

```
orbistoun-cli corpus list                                   # every source and whether each asset is pinned
orbistoun-cli corpus sync [--source <name>]                 # fetch, then pin or verify, into the library
orbistoun-cli corpus run --profile prospero-cex-12.40       # sync, run each guest, record to compat/
```

`corpus run` runs the ordinary `run` over every guest, so each records `compat/<name>.toml`
exactly as a hand run would, keyed by its own name. `--limit` and `--calls` bound each guest's
run time and call budget. A recorded run is always an unaided default entry: a run with a
diagnostic intervention measures the intervention, not the guest.

## Input scripts

`input/` holds pad scripts that take a title past its menus (D721). Each is captured with the
GUI's "capture input", timed in the guest's own flips, and signed off by watching it played back
with `orbistoun-gui --title <id> --playback <file>`. A run plays one with:

```
./bin/orbistoun run <id> -- --input corpus/input/<file>
```

so a headless test reaches the part of a title a person would otherwise walk it to.

| Script | Takes the title |
|---|---|
| `NVRB00001-into-game.toml` | from its title menu into the first level |
