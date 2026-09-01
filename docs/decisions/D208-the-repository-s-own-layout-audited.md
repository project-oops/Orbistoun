# D208 - The repository's own layout, audited once the tree stopped being small


**decided** · 2026-08-24

A greenfield repository with no commits, no remote, and thirty-one crates had accumulated
a set of structural defects that share one property: **every one of them was invisible
while the repository stayed as it was, and every one fires the moment somebody clones it
or pushes it.** They are recorded together because that shared property is the finding.

### `.gitattributes` protected a directory that has never existed

It reserved `tests/fixtures/**`. That pattern contains a slash, so git anchors it to the
repository root - and there is no `tests/` at the root. Every fixture lives under
`crates/<crate>/tests/`, so the rule matched nothing, and `git check-attr` said so to
anyone who thought to ask.

Worse than a no-op. Ten `.gcn` files are **guest shader bytecode**, and the extension was
in no binary list. A Windows clone with the default `core.autocrlf` would have turned every
`0x0A` inside them into `0x0D 0x0A` - corrupting the material the entire differential
suite is built on, in a way that presents as a decoder bug on one contributor's machine.

Patterns are unanchored now, `.gcn` is binary, source is pinned to LF in both the index and
the working copy, and the fixture rules come **last** because the last matching line wins -
which is how the first attempt at this fix silently un-protected them again.

### Three guards read the index, in a repository with no commits

`git ls-files` reports what is staged. Sixty-four files were not, including two entire
crates. So the provenance guard checked 252 of 311 files and said "OK".

The local guard had also drifted to a third of the CI job's coverage - one check where CI
ran three - which means passing locally said very little about passing in CI, on the one
gate where a surprise is expensive. All three checks now live in `./orbistoun.sh
provenance`, CI and the pre-push hook call that verb, and the file set is
`--cached --others --exclude-standard`: exactly what `git add -A` would pick up.

Tested by making it fire, in both threat models - a force-added `.prx`, and a large file
with a disguised extension.

### The pre-push hook skipped everything on the push that carried everything

It decided whether to run the heavy checks by diffing against `@{push}` and then `HEAD~1`.
Both fail when there is no upstream and no previous commit - the exact state of a
repository about to receive its first push. `RUST_CHANGED` defaulted to `false`, so the
hook skipped fmt, clippy, machete and audit on the one push containing the whole codebase.

Unknown now means "run them". A default that fails open is a default that has decided the
uncertain case is the safe one, and here it was the opposite.

### Two CI jobs would have been red on the first push, forever

`cargo deny check` rejected three transitive GUI dependencies: `BSL-1.0` twice, which is
Boost and simply missing from a permissive allow-list, and the font licences on
`epaint_default_fonts`, which are a different question from code licences and had not been
considered. BSL-1.0 is now allowed; the fonts are a scoped per-crate exception, because the
reasoning is about typefaces and would not transfer to a code dependency arriving under the
same identifier.

`orbistoun-cli audit` exited non-zero on 202 vendor names the grammar cannot respell. Each
is *proved correct* by hash collision against a real import table; what is missing is the
other half of the provenance claim. Failing every run on a known, shrinking fact makes the
job permanently red - and a permanently red job is one nobody reads, at which point the
case it exists for, a new name arriving with no explanation, goes past unnoticed.

`--ceiling symbols/unaccounted-ceiling.txt` is the third instance of a pattern this project
already had twice: fail on anything not listed, and fail again on anything listed that has
stopped applying, so the list can only shrink and cannot become permission. Tested in both
directions before being trusted.

### The macOS artifact cannot do the thing the project is for

`release.yml` publishes `aarch64-apple-darwin`, and the site offered it as a download with
no caveat. Guest code is x86-64 and runs natively - that *is* the architecture, and
principle 12 rules out an execution backend on purpose - so on ARM `enter_process` is
`unimplemented!()`.

Kept, because `symbols`, `imports`, `knows`, `questions` and `worklist` are most of the
tool and work fine. But the run path now refuses with a sentence explaining why, instead of
panicking: a panic is not an honest failure, it is a crash (principle 3). The download is
labelled.

### Smaller

- `orbistoun-gui` declared `orbistoun-overrides` and `orbistoun-worker` declared `serde`,
  neither used. `cargo machete` would have failed CI.
- Workspace members were listed in no order at all, under documentation claiming a
  dependency spine. Grouped to match what the docs say.
- Four `.pyc` files were staged; `docs/features/` was an empty directory described in the
  documentation hub; a one-off diagnostic script sat at the repository root where
  `git add -A` would have swept it into the first commit.

