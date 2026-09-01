# D164 - The library list was doing file I/O every frame


**decided** · 2026-08-20

Each library row carries a summary of that title's last run, read from its trace file. The
first version built the rows inside the draw call, which in immediate mode means **a file
read and a JSON parse per title, per repaint** - and a repaint happens whenever the pointer
moves across the window.

The icon cache, two files away, opens with a comment saying exactly this:

> Immediate mode redraws whenever the pointer moves, so anything done while drawing is
> done sixty times a second.

Writing that down did not stop me doing it. Which is the part worth recording: the rule was
known, stated, and in the same crate, and it still needed the code to be re-read to catch.
An immediate-mode draw call is a loop body, and anything expensive belongs outside it -
there is no framework here that will notice on your behalf.

Rows are now state, rebuilt in exactly two places: when the library is rescanned, and when
a run finishes. Those are the only two events that can change what a row says, and the
second is easy to forget - a cache that never refreshes is a different bug with the same
cause.

**Unrelated and unexplained:** an earlier instance exited on its own with status 1 and no
output, while idle and untouched. It has not reproduced since. Recorded here rather than
attributed to this fix, because there is no evidence they are the same thing and claiming
otherwise would make a real bug look closed.

