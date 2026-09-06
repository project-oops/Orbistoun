# 2026-09-02 - (/loop) Three firmware directories, and a test that cannot tell them apart

```
tests   1960  ->  1961
```

A platform library survey arrived in the sibling repository - 537 modules across three
directories, with the privilege tier of each. Folded in what applies, and the folding turned
up something worth more than the fix.

## The prefix covered 274 modules and missed 234

`sceKernelLoadStartModule` refused firmware modules with `path.starts_with("/system/")`. The
survey gives three directories:

| directory | modules |
|---|---|
| `/system/common/lib/` | 274 |
| `/system_ex/common_ex/lib/` | 234 |
| `/system/priv/lib/` | 29 |

**`/system_ex` is not under `/system`.** The prefix was generalised from the only directory the
probe ever asked about, and it silently excluded the larger half of the second-biggest tier.
`/system/priv/lib/` was covered by accident, sharing the prefix.

Now a table, with the survey cited.

## And the test I wrote for it cannot detect the bug

Written, then checked by reverting to the single prefix - **and it still passed.**

An unmatched path falls through to the unrecognised-path refusal, and the console answers
`0x8002_0002` for a path that does not exist just as it does for a firmware module
(`060-module/load-rejects-missing`, in both captures). Both routes give the same code. From
outside there is nothing to see.

So the test pins **the answer, not the reasoning**, and its doc comment now says so. The first
version claimed the opposite - *"a right answer reached the wrong way survives every test that
only reads the answer"* - written about a test that only reads the answer. Caught by trying it,
which is the only reason it is not still there.

The table still earns its place, and the reason is dated rather than immediate: **when the
loader stops refusing, the two branches stop agreeing.** `/app0` will load and a firmware path
must not. A distinction that is invisible today is the one that breaks silently tomorrow.

## What the survey is worth, and what it is not (REFERENCES.md)

Recorded as its own reference rather than folded into the conformance-run entry, because it is
a weaker claim and the difference is exactly the sort this project exists to keep visible.

The conformance run's header names the artefact and the console state it ran under. This
manifest's header says "measured on hardware via obSCEne probe & live filesystem survey" - and
**no such survey appears in any capture this project has read.** The 2026-08-30 runs contain no
directory listing.

So: the two `/system` refusals are measured, because `110-modules/load` asked for both. The
extension to `/system_ex` rests on the document alone. Written down that way, because a
directory list that turned out to be inferred would otherwise be indistinguishable from one a
machine printed. **One probe asking for a `/system_ex` path settles it.**

The privilege tiers and credential values were read and deliberately not taken: orbistoun does
not sandbox, so a tier is a fact about the platform that nothing here would act on.

## What it changes for the loader

The survey separates **platform** paths from **title** paths cleanly, which is the discovery
question D482 has to answer. The platform's are three fixed directories. A title's are its own:
`/app0/sce_module/` is a location the platform maps, and PPSA02664 puts
`Il2CppUserAssemblies.prx` in `Media/Modules/`, which is the title's invention.

That leaves a question the survey sharpens rather than answers: those four imports are
load-time import-table entries, but `Media/Modules/` is not a platform search path, so
**something has to tell the loader where to look.** D482 has to answer it rather than assume.

## State

`cargo test --workspace` green - **117 suites, 1961 tests**, 0 failures. clippy `--tests`
clean, fmt clean, identity scan clean.

Nothing committed. The day holds worklogs 292-328 and D466-D481.

**Next**: D482 and the load order, now with the platform/title split settled and the discovery
question sharpened.

## Passed back to the sibling repository

`docs/PLATFORM_LIBRARIES.md` carries two `file:///C:/...` absolute paths (lines 9 and 145),
which are the blocking tier of the identity guard. They were not caught by a working-tree scan
because it runs `git grep` without `--untracked` and both files are new - `--cached` would stop
the commit. Reported rather than edited: it is the other repository's file and the guard is not
mine to change.
