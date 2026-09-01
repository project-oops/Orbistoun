# D222 - A build says which build it is, and the field for it was never populated


**decided** · 2026-08-24 · a convention the user carries across projects

The commit a binary was built from is shown in the running application - a sidebar, a
footer, a menu. Where there is no commit, it shows when the binary was compiled instead.

### Why it is worth the footer

So a screenshot, a bug report or a run result can be tied to a tree somebody else can check
out. A result nobody can attribute to a build is a result nobody can reproduce, and this
project's entire argument is that its results are reproducible.

Locally the fallback answers a different and more immediate question: *am I looking at my
last change, or at a binary from an hour ago?*

### The gap it exposed

`ORBISTOUN_COMMIT` has been read by the reporting layer since that layer was written, and
**nothing has ever set it** - not CI, not the release workflow, not `orbistoun.sh`. So
every run report ever produced carries `binary_commit: "unknown"`, from a field whose only
job is to say which tree produced a result. Its own doc comment says it is there "for a
result contributed by somebody whose tree you cannot see".

Third mechanism this session that exists and does nothing, after the ceiling comparison
that was skipped and the `found_by` check that never looked at its field.

**Both halves, and the CI half is the one that matters.** `ci.yml` and `release.yml` set
`ORBISTOUN_COMMIT` from the workflow context, which is authoritative even in a shallow
checkout; the build script falls back to asking git, so a plain clone with no configuration
still stamps itself. An explicitly supplied value always wins.

On a pull request it is the **head** SHA rather than `github.sha` - which is the ephemeral
merge commit, existing only inside the run, so nobody could ever check it out.

The workflows pass the SHA **whole**. Actions expressions cannot slice a string, so
shortening there means a shell step per workflow whose only job is to cut seven characters
off - three copies of one rule, kept in step by hand. The build script shortens instead,
and only when the value is plain hex: a tag or a `git describe` string passes through, since
truncating one produces something that looks like an identifier and identifies nothing.

That matters more for `release.yml` than it reads. The release model is that there are no
tagged versions and **the short SHA is the version number** - a claim the CHANGELOG has
made from the beginning, while every binary the workflow published said `unknown`.

A modified tree gets `-dirty`. A binary built from uncommitted edits is not the commit it
would otherwise name, and a report pointing at a commit somebody can check out has to be
true or it is worse than saying nothing.

### Two details that are choices rather than defaults

**The time comes from the executable, not from a constant.** A value stamped into this
crate says when *this crate* was last compiled, which is older than the binary whenever a
change above it triggered the link. The file's own timestamp is when the thing being run
came into existence.

**It is UTC.** A build stamp is compared against another build stamp, and a local time that
shifts twice a year makes two of them incomparable for no benefit.

`orbistoun_nid::timestamp_of` formats it, extending the calendar arithmetic already there
for derivation dates rather than repeating it - `today` is now the same function underneath.

### And a duplicated match arm

The sidebar had `Ok(titles) if titles.is_empty()` **twice**, with identical guards and
identical bodies, so the second was unreachable. Rust cannot prove two guards equivalent,
so neither the compiler nor clippy said anything. Removed while adding the footer beneath
it.

