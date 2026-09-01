# D184 - Guards for the instrumentation, because the tests were faithful to the mistake


**decided** · 2026-08-21

Four ranking and rendering bugs landed in one day and **every one had passing unit tests**.
That is not a coverage gap. A test written by whoever chose an ordering asserts the ordering
they chose, so it cannot see that the ordering is wrong - the tests were faithful to a
mistaken belief. More of them would have been more faithful.

What actually caught two of the four was **rendering real data and reading it**: a spin on
four imports sitting at the top of the frontier is obvious in a table and invisible in an
`assert!(a.beats(&b))`. The other two were caught by the tool refusing an action and saying
why. Neither mechanism is a unit test.

### Division of labour, since this overlaps a sibling project

obSCEne is the conformance oracle - the blargg of this project - and would not have caught
any of the four, because none was in the emulation. They were in the code that *ranks* and
*renders* what the emulation observed. A perfectly accurate thermometer with a broken
readout is still useless, and a guest-side test suite cannot see the readout.

So: obSCEne tests the emulation, these test the instrumentation. Neither substitutes.

### A golden frontier over `compat/`, not over `titles/`

The first framing was a snapshot over the corpus, which is untracked and machine-specific -
it could not be committed and could not run for anybody else. `compat/` is tracked, holds no
guest material, and exercises exactly the ranking and rendering that broke. The table is
committed and regenerated deliberately; **it is expected to change**, and the diff is the
artefact.

Ranking and rendering moved out of the command that prints them, which is where they should
have been - principle 13, and the same reason `compare` already sits below the shims (D160).
The order is made total by breaking ties on the title, or the two abort-at-53 entries swap
places between runs and the golden file churns until nobody reads its diffs.

### A mechanical check for the formatting trap

`\` at the end of a line inside a string literal is collapsed by `cargo fmt`, which bakes
the source indentation into the rendered text. It has shipped garbled output three times,
always at format time rather than when the line was written - which is why it is a check
rather than a habit.

**The check was broken when first written**, and matched nothing: the escaped backslash did
not survive the shell and YAML quoting layers. Caught only because it was tested against a
deliberate offender before being trusted - the project's own rule that a guard which never
fires is indistinguishable from one nobody wired up (D175), applied to a guard. A bracket
expression is the portable spelling.

Eight existing files carry the construct, seven of them another session's crates. They are
recorded in `docs/prose-continuation-backlog.txt` as a ceiling that can only shrink, the
same mechanism the duplicate decision numbers use (D201) and for the same reason: rewriting
another session's strings mid-flight conflicts with edits in progress.

> **Correction (D199).** "Eight" was wrong, and wrong because the guard could not see: it
> searched the index in a repository with no commits. The true count was twenty-one. The
> guard had failed in exactly the way this entry congratulates itself for avoiding.

