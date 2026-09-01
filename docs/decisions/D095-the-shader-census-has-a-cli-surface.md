# D095 - The shader census has a CLI surface

**assumed** · 2026-08-19

`orbistoun-cli shaders <dir> [--top N]`, rendering through
`orbistoun_shader::report`.

Principle 13 - the command holds no logic, because what a coverage report says is a
property of the analysis and this command and the run report must not be able to
disagree about it.

It reads only files carrying the corpus extension. The first version read everything in
the directory, decoded the reference-text files alongside the binaries, and reported
eighteen shaders where there were nine. Plausible output, entirely wrong - the same
failure mode this crate exists to catch in guest code, arriving in the tool itself.
Skipped files are counted and reported rather than silently ignored, so a corpus using
a different extension does not read as empty.

