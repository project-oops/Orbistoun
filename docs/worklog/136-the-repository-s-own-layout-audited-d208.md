# 2026-08-24 - The repository's own layout, audited (D208)


**Done.** A structural pass over everything that is not Rust source: `.gitattributes`, the
three provenance guards, the pre-push hook, two CI jobs that would have been red from the
first push, the workspace manifest, and the one directory that is a different language.

The finding that ties them together: **every defect was invisible while the repository
stayed exactly as it was, and every one fires the moment somebody clones or pushes it.**
A greenfield repo with no commits and no remote is a set of assumptions nobody has tested.

### The one that would have corrupted data

`.gitattributes` reserved `tests/fixtures/**`. That pattern contains a slash, so git anchors
it to the repository root, and **there is no `tests/` at the root and never has been** -
every fixture lives under `crates/<crate>/tests/`. `git check-attr` had been answering
`text: unspecified` to anyone who asked.

The dangerous half: ten `.gcn` files are guest shader bytecode and the extension was in no
binary list. A Windows clone with default `core.autocrlf` turns every `0x0A` inside them
into `0x0D 0x0A`, corrupting the material the whole differential suite rests on - and it
presents as a decoder bug on one person's machine.

Nothing was damaged; the index bytes matched the working tree. It was caught before it bit,
which is the only reason this is a paragraph rather than a week.

Fixing it took two attempts, and the second bug is the more instructive: `*.txt text
eol=lf` was added *after* the fixture rules, and **the last matching line wins**, so the
protection was silently removed again. The file now says so at the top, and 146 files were
normalised to LF so the working tree matches the declared policy.

### Guards that could not see

`git ls-files` reads the index. Sixty-four files were not in it, including two whole crates,
so the provenance guard checked 252 of 311 and said OK. The local copy had also drifted to
one of CI's three checks - so passing locally said little about passing in CI, on the one
gate where a surprise is expensive.

All three checks now live in `./orbistoun.sh provenance`; CI and the hook call the verb.
The file set is `--cached --others --exclude-standard`: exactly what `git add -A` picks up.
Tested by making it fire on a force-added `.prx` and on a large disguised file, and by
confirming it stays quiet for ignored guest material under `titles/`, which is the
arrangement working.

**The pre-push hook skipped every check on the push that carries everything.** It chose
whether to run the heavy checks by diffing `@{push}..HEAD` then `HEAD~1..HEAD`; both fail
with no upstream and no commits, and the default was `false`. Unknown now means run them.

Twice in one session a `[ ... ] && echo` as the last statement of a block ended the script
under `set -e` while reporting nothing - once in the new size check, once in the hook's own
`grep -q ... && VAR=true`. Both are `if`s now, and it is worth naming as a shape rather
than two bugs.

### Two CI jobs would have been red forever

`cargo deny check` rejected three transitive GUI dependencies. BSL-1.0 twice - Boost,
plainly permissive, simply missing from the list - and the font licences on
`epaint_default_fonts`, which are a different question from code licences and had not been
considered. BSL-1.0 allowed; the fonts scoped to that one crate, because the reasoning is
about typefaces and would not transfer to code arriving under the same identifier.

`orbistoun-cli audit` exited 1 on 202 vendor names the grammar cannot respell. Every one is
*proved correct* by hash collision; what is missing is the other half of the provenance
claim. Failing every run on a known, shrinking fact makes a job permanently red, and a
permanently red job is one nobody reads - at which point the case it exists for, a new name
with no explanation, goes past unnoticed.

`--ceiling symbols/unaccounted-ceiling.txt` is the third use of a pattern already here
twice. Tested in both directions before being trusted: a name unaccounted and unlisted
fails, a name listed that is no longer unaccounted also fails.

Both jobs now call `./orbistoun.sh` verbs, and `check` runs the symbol audit - which
WORKFLOW.md had claimed for some time before it was true.

### The macOS artifact cannot do the thing the project is for

`release.yml` publishes `aarch64-apple-darwin` and the site offered it with no caveat.
Guest code is x86-64 and runs natively - that *is* the architecture - so `enter_process` is
`unimplemented!()` there. Kept, because the analysis commands are most of the tool, but
`run` refuses with a sentence instead of panicking. A panic is not an honest failure.

### Smaller

`orbistoun-gui` declared `orbistoun-overrides` and `orbistoun-worker` declared `serde`,
neither used - `cargo machete` would have failed CI. Workspace members were in no order at
all under documentation claiming a dependency spine. Four `.pyc` files were staged,
`docs/features/` was an empty directory described in the docs hub, and a one-off diagnostic
script sat at the root where `git add -A` would have swept it into the first commit.

### And one own goal

D200 was taken. The number was checked earlier in the session, the parallel session's
entries at 200-207 were noted, and the entry was written as D200 anyway - caught by the
duplicate-number guard, which is exactly the structural collision it was built for (D201).
Renumbered to D208, along with eight citations.

