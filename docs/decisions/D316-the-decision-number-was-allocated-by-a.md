# D316 - The decision number was allocated by a convention that races


**decided** · 2026-08-27 · after colliding twice in one afternoon

A decision is cited from source by number, so two entries sharing one makes every citation
of it ambiguous - a reader following `(D313)` lands on two decisions and cannot tell which
the code meant.

The convention was *read the highest number, add one*. That races, and not through
carelessness: more than one session appends to this file, each reads the last number, and
neither sees the other's unlanded writes. It happened twice in an afternoon, both times
because a number was chosen, several minutes were spent writing the body, and by the time
the entry landed somebody else had taken it.

**The window is the bug, so `./orbistoun.sh decide "<title>"` closes it.** The number is
claimed the instant it is chosen - under a `mkdir` lock, atomic everywhere this runs - and a
reservation is appended before a word of the body exists. Whatever a second session does
next, it reads a higher number.

The gate already caught duplicates and that stays: it is the backstop for anyone who edits
the file by hand, which is still allowed and always will be. What changes is that catching
one is no longer the *only* mechanism, and by the time it fires the damage is already in the
source comments citing it.

**A reservation nobody spends is now refused too.** A claimed-and-abandoned number reads in
the log like a recorded decision, and leaving one behind is the easy failure - a session
ends, a context is lost. The guard was watched refusing one before this entry replaced it.

The general shape is the one this log keeps arriving at from different directions: a rule
held by a person is a rule that holds until two people apply it at once. The fix is almost
never to be more careful.

