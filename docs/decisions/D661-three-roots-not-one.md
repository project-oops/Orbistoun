# D661 - Three roots, not one

**Status:** measured
**Date:** 2026-09-09

## One root was doing three jobs

Everything the corpus fetched landed under `titles/`, because that was the only root there was.
So `titles/ps5-payloads-mirror/BackPork_0.1/BackPork_0.1.elf` sat beside `titles/PPSA28061-app0/`,
and the compatibility table listed twenty-five one-file homebrew ELFs as though they were
installed titles. A shell drawing the library would have shown them the same way.

They are three different things:

| kind | what it is |
|---|---|
| **title** | a directory with a `param.json`, an `eboot.bin` and its own filesystem |
| **payload** | one executable somebody runs directly |
| **package** | something not installed yet - the input side of installation |

## The path model made this cheap, and that is the point of it

`orbistoun-paths` hangs every location off `data_root` and resolves that root once. Adding
`payloads/` and `packages/` was two constants, two accessors and two lines in `named_dirs` - and
because `named_dirs` is the single registration point (its own doc says so), they were picked up
by `all_dirs`, by `ensure_dirs`, by the containment test and by `orbistoun-cli paths` without
being mentioned anywhere else.

**Portable mode needed no work at all**, which is the property that split was built for: portable
resolves a different `data_root` and everything follows. A directory resolved separately would
have had to be taught about portable mode, and would have been the one that escaped its root.

## The manifest says where, and `--titles` still means what it says

A corpus source now carries `target = "titles" | "payloads" | "packages"`, defaulting to `titles`
- what every source was before there was anywhere else to put one. `sync` routes each source to
its own root and prints where it went.

The three roots are derived from the titles root rather than resolved independently, so pointing
`--titles` at a scratch directory takes payloads and packages with it. Resolved separately, two of
the three would follow the override and one would quietly keep writing to the real data directory
- which is the shape of bug that takes a day to find.

One mapping function, shared by `sync` and `run`, so a guest cannot be looked for somewhere it was
never put.

## What this does not do

The manifest still has one origin per source. Per-item **source lists with fallback** - try a
local build, then a release URL, then give up and carry on to the next item - is asked for and not
built. So is the vocabulary change (Prospero / Trinity / Neo), and the two shell surfaces that
would list packages and payloads. Each is recorded as wanted rather than half-done.
