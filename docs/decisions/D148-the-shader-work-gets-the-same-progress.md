# D148 - The shader work gets the same progress loop as the imports


**Status:** decided

The import side ends every run with `FURTHER`, `same` or `BACK` and a delta, and that -
more than any single feature - is what makes it iterable. A change either moved something
or it did not, and nobody carries two numbers between runs in their head.

The shader side has had the same loop all along: rank what blocks, implement the top
entry, run again. What it lacked was the verdict. Progress was two figures read off
consecutive screens and compared by eye, which is exactly how a regression goes unnoticed
for three changes.

`orbistoun-cli shaders` now records each run and reports movement against the last, in the
**same vocabulary**. They are one loop pointed at different material and giving them
different words would suggest otherwise.

**Completeness leads, instructions follow.** A run translating one more whole shader has
moved further than one translating three more instructions across shaders that still do
not run - a shader is the unit that can be checked against hardware and an instruction is
not. But more instructions alone is still `FURTHER`, because calling it `same` would make
a real change look like a wasted one.

**Cleared and uncovered blockers are reported apart.** Implementing one blocker routinely
reveals the next instruction in a shader that could not be reached past it. The count is
unchanged and the work moved; a count alone reads as nothing having happened.

**It does not wait for a submission.** The corpus is a directory of shader binaries, which
today is the fixture set and tomorrow is captures - the loop is identical either way. That
is why this was worth building before a guest gets far enough to submit anything: the same
argument as the rest of phase 6, but for once it is exercisable today rather than only
argued for.

History is keyed by corpus path, so two corpora do not overwrite each other - the same
reason traces are keyed by module.

