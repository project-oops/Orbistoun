# D267 - A second guess at where data lives put a downloaded runtime in the repository


**decided** · 2026-08-25 · the provenance guard caught it on the first run

`orbistoun-suggest` resolved its own data root: read the data-directory variable, and fall
back to `.orbistoun` in the working directory. Run from the repository - which is where a
developer runs it - that fallback *is* the repository, so the first invocation downloaded a
model runtime into the tree and the provenance guard failed on four DLLs.

`orbistoun_paths::Paths::resolve` already exists, already honours portable mode and the
data-directory setting, and is what every other entry point calls. The fallback was a second
definition of a decision one crate owns, written in four lines because it looked too small
to be worth a dependency.

**The guard is the point of the story.** Principle 1 exists for firmware and disassembly,
and what it caught was a build artefact - a category nobody was thinking about when it was
written. A guard that only ever catches what its author imagined is a guard that has not
been tested; this one earned its place on something else entirely.

`orbistoun-propose` depends on `orbistoun-paths` now, and on nothing for the data root but
that.

