# 485. Three roots

**2026-09-09** - directed, continuing 484

A payload mirror had been downloading into `titles/`, so twenty-five one-file homebrew ELFs sat
beside installed titles and the compatibility table listed them as titles. They are not: a title
is a directory with a `param.json` and its own filesystem, a payload is one executable, a package
is something not installed yet.

## What landed

`orbistoun-paths` gains `payloads/` and `packages/` beside `titles/`. Two constants, two
accessors, two lines in `named_dirs` - and because that is the single registration point, they
were picked up by `all_dirs`, `ensure_dirs`, the containment test and `orbistoun-cli paths` with
no other edits. **Portable mode needed nothing**, which is what the `data_root` split is for.

A corpus source carries `target = "titles" | "payloads" | "packages"`, defaulting to `titles`.
`sync` routes each source to its own root and says where it went:

```text
payloads-mirror (github-release) -> payloads
obscene (local) -> titles
```

The three roots derive from the titles root rather than resolving independently, so `--titles`
still means what it says - point it at a scratch directory and all three follow. One mapping
function shared by `sync` and `run`, so a guest cannot be sought where it was never put (D661).

## Surprises

- **The doc-comment mistake, sixth time**, and this one also left a stray brace that only showed
  up as "unclosed delimiter" 4,000 lines later. Insert after the closing brace, never before a
  comment - and check the cut range, not just the paste.
- **PPSA99980 jitters like PPSA25872** - 10,600 then 10,748 calls. Both now run enough concurrent
  work that the count is not fixed; the frontier records one number and this is the second title
  to make that awkward.

## Deliberately not done

Asked for and staged rather than half-built: per-item **source lists with fallback** (try a local
build, then a release URL, then log and continue); the **Prospero / Trinity / Neo** vocabulary,
which is a three-way split where the code has a two-way `Generation` enum and so is a design
change rather than a rename; and the two **shell surfaces** - a package manager listing
`packages/` and simulating installation, and a payload lister that can execute one.

## Next

- The source-list fallback, which the manifest shape now has room for.
- The vocabulary, with a decision record: codenames are the oracle, and they are not trademarks,
  which is why §2 is satisfied by them rather than violated.
