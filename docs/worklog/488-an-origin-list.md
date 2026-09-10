# 488. An origin list

**2026-09-10** - directed, continuing 487

The fallback the manifest had room for, built test-first.

## What landed

A corpus source carries `sources` - origins tried in order, first that answers wins, every failure
kept and named. Origins are bare strings classified here rather than declared, so a release URL
sits beside a relative path without a keyword for each. A drive-letter path is a path and not a
scheme, which is why classification is a tested function and the test is `://` rather than `:`
(D664).

The four obSCEne entries are in, routed by target:

```text
payloads-mirror                 (github-release) -> payloads
obscene                         (local)          -> titles
obscene-probe-prospero-native   (local)          -> titles
obscene-probe-prospero-payload  (local)          -> payloads
obscene-probe-neo-native        (local)          -> titles
obscene-probe-neo-pkg           (local)          -> packages
```

## Surprises

- **An insert landed between `#[cfg(test)]` and its module**, making a public enum test-only and
  the test module unconditional. Same family as the doc-comment mistake and now the seventh of
  them: an attribute binds to what follows it, so inserting *after* an attribute is inserting
  *into* an item.
- **A sibling repository changed underneath me.** The harness reported `writing.md` modified in
  SELFish seconds after I read it. Not this session - the SELFish agent working its own repo
  through the mesh. Worth knowing before anyone reads a cross-repo mtime as their own doing.

## Deliberately not wired

The fetch path still uses `repo`/`tag`/`path`. The decision layer is complete and tested and the
manifest carries the field, but walking an origin list at fetch time is the next step - and a
manifest that describes something the code does not do is worse than one that describes less.

## Next

- The fetch path, walking the list.
- The two shell surfaces - packages and payloads - still unbuilt.
