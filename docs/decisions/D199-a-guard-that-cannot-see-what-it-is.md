# D199 - A guard that cannot see what it is checking is worse than none


**decided** · 2026-08-24

The line-continuation guard failed twice, in the same direction, and reported success both
times.

**First:** it used `git grep`, which searches the index. It was written in a repository with
no commit, so it was checking a fraction of the tree and printing "none added" for the rest.
Fixed by searching the working tree.

**Second, and only visible once the first was fixed:** the output was piped through a `sed`
expression meant to normalise path separators, and the expression was malformed. `sed`
aborted, `offenders` came out empty, and the two comparisons that follow both degenerated -
nothing could ever be reported as *unlisted*, and every file on the ceiling was reported as
having *stopped offending*. The visible symptom was the second one, which reads as good news.

The normalisation was never needed: `grep -r crates/` echoes back the prefix it was given,
so its output is already `/`-separated everywhere. Removed rather than repaired.

The ceiling went **8 files to 21** as a result, which is the direction its own header
forbids. That paragraph stands - twenty-one is simply the first count taken with the guard
able to see, and it is the ceiling from here.

**The general form.** A check whose failure mode is a false pass is worse than no check,
because no check leaves the question open and a false pass closes it. Any guard added here
should be tested by making it fire once - a guard nobody has watched fail has not been
tested at all.

