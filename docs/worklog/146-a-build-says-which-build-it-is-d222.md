# 2026-08-24 - A build says which build it is (D222)


**Done.** The commit in the GUI sidebar and in `orbistoun-cli paths`, supplied by CI and by
a build script, falling back to the compile time where there is no commit.

A convention the user carries across projects, and applying it uncovered something.

### The field had never been populated

`ORBISTOUN_COMMIT` has been read by the reporting layer since that layer was written, and
**nothing has ever set it** - not CI, not the release workflow, not `orbistoun.sh`. So every
run report ever produced carries `binary_commit: "unknown"`, from a field whose only job is
naming the tree that produced a result.

Sharper than it sounds given the release model: there are no tagged versions and **the short
SHA is the version number**, which the CHANGELOG has claimed from the start while every
binary the workflow published said `unknown`.

Third mechanism this session that exists and does nothing, after the ceiling comparison that
was skipped and the `found_by` check that never looked at its field.

### Four choices worth keeping

- **CI supplies it, the build script falls back to git.** CI is authoritative even in a
  shallow checkout; the fallback means a plain clone stamps itself with no configuration.
- **The PR *head* SHA, not `github.sha`** - which on a pull request is the ephemeral merge
  commit and exists only inside the run, so nobody could check it out.
- **The workflows pass it whole and the build script shortens it.** Actions expressions
  cannot slice a string, so shortening there is a shell step per workflow doing one thing,
  kept in step by hand. Only plain hex is shortened; a tag passes through, because
  truncating one produces something that looks like an identifier and identifies nothing.
- **The compile time comes from the executable's own timestamp**, not a constant baked into
  a crate - which says when *that crate* was last built and is stale whenever a change above
  it triggered the link.

### Two smaller finds

The sidebar had `Ok(titles) if titles.is_empty()` **twice**, identical guard and identical
body, so the second was unreachable. Rust cannot prove two guards equivalent, so neither the
compiler nor clippy said anything.

And `rerun-if-changed` naming a path that does not exist makes cargo re-run a build script
on *every* build - so the `.git` watches are emitted only when there is a repository, or a
source tarball would pay a rebuild for a stamp it can never have.

### A collision worth recording

`crates/orbistoun-cli/src/main.rs` was being edited by another session **while this was in
progress** - `real_hardware` renamed to `is_target`, the file four kilobytes smaller in
fifty seconds, and mid-rename so it did not compile.

Whole-file rewrites (`perl -0pi`) against a file another session is holding will silently
revert its work. Mine survived; I stopped editing that file rather than find out the hard
way. Worth remembering: **in a shared tree, prefer edits that fail loudly on a mismatch over
rewrites that always succeed.**

