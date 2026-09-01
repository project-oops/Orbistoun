# The corpus is records all the way down, and the reader was looking in the wrong place


Read the rest of obSCEne's documentation, checked its claims against this project, and gave
`orbistoun-probe` a way to be reached.

**`HANDOVER-ORBISTOUN.md` is addressed to this project and every warning in it checks out.**
Unresolved imports resolve to `None` and are counted rather than pointed at a plausible
address, so a probe guarding with `address != NULL` is never fooled. Thunks are real
functions that announce being called rather than a page that silently succeeds. And the
output path - the one it calls the difference between a conformance run and a run that
produced no evidence - is already implemented, with D170 recording the exact failure it
warns about. Nothing needed fixing; the value was in checking rather than assuming.

**`orbistoun probe <path>` exists.** It reads a transcript or a corpus and reports what the
run establishes, graded. No socket, no hardware, files only.

### Surprises

**A corpus contains no commands, and the reader dropped every result because of it.** The
session transcript is the interface - `CMD|` and replies - but the artefact that gets
committed is the report a run produced, and that is records standing on their own. Records
were only collected when they arrived inside an exchange, so pointing the tool at a real
report gave `0 of 0`. Found by running it against real output rather than against the
fixtures it was written from, which is the whole argument for having a command-line surface
at all: a library nothing calls cannot be wrong in a way anyone notices.

**And what it says now is the useful answer.** That report reads `0 of 47 are facts`, with
all forty-seven ungraded because it predates the provenance field. A run that looks like
forty-seven results has established nothing until it is graded, and a summary reporting
"47 results" would have been describing effort rather than evidence.

**The heredoc ate a string continuation for the third time this session.** A `\` at the end
of a Rust string line reached the file as nothing at all, so the padding became part of the
message and the output had twenty spaces mid-sentence. It is the same trap as the `\n`
confusion earlier and the `class Sample` deletion before that: multi-line content through a
heredoc is unreliable here, and the Write tool is the answer.

**One anchored insertion landed inside an enum.** The doc comment I anchored on appears
twice - once on the `Shaders` command variant and once on the function that implements it -
and the first match was the wrong one. Identical to the `buffer_memory` doc-comment split
earlier today. Anchoring on prose that reads naturally is anchoring on something that
repeats.

