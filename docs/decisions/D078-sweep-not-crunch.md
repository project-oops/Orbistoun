# D078 - `sweep`, not `crunch`

**decided** · 2026-08-19 · prompted by the user

The loop verb was called `crunch`, and that word already means something else here: a
long unattended development session. Giving a fifteen-minute measurement the same name
as a six-hour working session makes two quite different things share a word, and the
ambiguity would have cost more than the rename does.

`sweep` says what it does - go over every module available and collect what comes back.

Two things surfaced while fixing it, both the same kind of drift:

- **`provenance` was a working verb missing from the help text.** A verb nobody can
  discover is a verb nobody runs.
- **`paths` was documented and did not exist.** WORKFLOW.md told a reader to run it to
  find where artifacts go. Written rather than deleted, because "where did my data go"
  is a real question and portable mode moves every answer at once.

The general point: a script and its own help are two representations of the same thing,
and they drift in both directions. Worth checking each against the other when either
changes.

