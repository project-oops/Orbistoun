# D201 - Decision numbers are checked, because more than one session assigns them


**Status:** decided · 2026-08-21

Fifteen decision numbers appeared twice, and thirteen of them are **cited from source
code**. A reader following `D125` in a comment lands on two different decisions and cannot
tell which the code meant. `CLAUDE.md` calls this log the project's durable memory; an
ambiguous citation is exactly that memory degrading.

**The cause is structural, not carelessness.** More than one session appends here. Each
reads the last number and adds one. Neither sees the other's unlanded writes, so two
sessions working the same afternoon assign the same number without either doing anything
wrong. Being careful does not prevent it - which is why this is a check rather than a
convention, the same reasoning as enforcing crate boundaries with `cargo` rather than with
review (principle 12).

`./orbistoun.sh check` now fails on a duplicate.

**The existing fifteen are recorded as a ceiling, not an allowance.**
`docs/decision-number-backlog.txt` lists them; the check fails on any duplicate *not* in
that file, and fails again on any entry in it that has stopped duplicating. So the list can
only shrink, and it cannot quietly become permanent by being ignored.

They are not fixed in a batch, for a reason worth stating: each is cited from source, so
clearing one means renumbering an entry *and* its citations, and most of the pairs are
another session's work. Renumbering someone else's decisions mid-flight would conflict with
edits in progress and is not a thing to do unilaterally. Take one when already in that
area.

**This decision was numbered twice while being written**, which is the shortest possible
demonstration of the problem: the first choice collided with an entry that had landed
between reading the file and writing to it, and the second was picked well clear of the
other session's range rather than at the next free number - because "next free" is the race
itself.


