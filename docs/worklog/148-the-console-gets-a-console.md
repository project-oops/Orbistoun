# The console gets a console


The triage loop exists now. When this emulator cannot say what a function does, the console
can be asked - from a command line or from a window - and the answer comes back labelled
rather than interpreted.

**Protocol.** `hello` carries the session secret as an appended fourth field, and
`unauthorised` is a named refusal. A probe that generated no secret still accepts the
three-field form, so nothing that worked stopped working; one that did answers with a clean
refusal rather than a broken wire, which matters because the secret is replaced by every
restart and a stale key is the common mistake.

**`orbistoun ask`.** One question, one answer, printed and not interpreted - `returned 0x2`,
`died`, `refused unauthorised`. No grading, no knowledge entry, no judgement about whether
the value is usable. Those have rules attached and a command whose job is "ask the console"
should not make them quietly.

**A probe console in the GUI.** Address and key, a command line, quick buttons for the verbs
this probe announced, and the target's self-report with its three states kept distinct. The
connection lives on a worker thread: a socket read blocks for its whole budget, and a frame
that waited on one would freeze the application on a probe that has gone quiet - which is
the exact condition worth being able to watch.

Recorded as D225: **ask, record the answer with its caveat, return it unless it is a
handle.**

### Surprises

**The purity judgement dissolved into a field that already existed.** The first shape of
the rule needed to know whether a function was pure before trusting a live answer, which is
a per-function judgement nobody makes reliably and which fails silently. Keying on the
*return kind* instead - status-like passes through, handle-like is recorded but not returned
- is checkable, and `Returns` was already load-bearing for exactly this reason (D125). The
better rule was a property the knowledge base had been carrying all along.

**`D208` collided, and the collision was mine to fix.** Another session had taken it while
this one was writing. Renumbered to D225. Third numbering collision this project has had and
the check caught it immediately, which is the whole reason it exists.

**The GUI needed a repaint request, and would have looked broken without it.** A live session
produces events on its own schedule rather than in response to input, so an answer sits
unseen until the pointer happens to move. Immediate mode makes that a one-line fix and an
easy omission - the symptom is a window that appears to have hung while working perfectly.

**Four crates appeared that this thread had never heard of.** `propose`, `llm`, `env`, `gen`.
Reading `propose` first was right and changed what got built: it already owns the
propose-dispose-keep shape, and its stub-semantics proposer is deferred *on oracle cost* -
"one boot, one bit". The probe is that oracle arriving. What nearly got built here was a
parallel path beside a framework designed for it.

