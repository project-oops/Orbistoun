# The run now says what to do about itself


Reframed by a good question: orbistoun's end goal is to be loud and clever about what it
dumps, and eventually to propose its own fixes. That consumer is not a person reading a
terminal, so the diagnostics built this session were right in spirit and wrong in shape -
each built ad hoc for one bug, each rendered as prose (D179).

A run now produces **findings**: a routable kind, a subject, evidence from the trace, a
suggested action, a weight. Ranked by confidence before weight, because a heavy guess must
never outrank a light certainty.

Confidence is the field that matters. A confidently wrong suggestion is worse than none -
it gets acted on - and this project has produced three of those already: an entry
convention that looked right, a stub policy that looked wired, a name sweep whose
vocabulary could not hold the answer. Nothing reports `Certain` unless the trace shows it.

`Unimplemented` needed one new fact - whether a called import has a handler behind it -
because without it a trace cannot tell "used this and it worked" from "used this and got a
placeholder".

**It reproduced a day of manual reading in one pass.** Pointed at the corpus it classified
all three failure shapes found by hand today: the spin on `sceKernelDirectMemoryQuery`, the
placeholder dereferenced by PPSA21564, and the deliberate abort in two titles - with the
error-reporting call before the abort surfaced as evidence.

Also caught a formatting bug worth noting: `cargo fmt` collapsed a line-continued string
literal and baked the indentation into it, so the rendered action had runs of spaces in the
middle. Output meant to be consumed cannot afford that.

