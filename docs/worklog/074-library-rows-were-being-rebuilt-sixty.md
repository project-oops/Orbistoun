# Library rows were being rebuilt sixty times a second


Each row shows a summary of the title's last run, read from its trace file. The first
version built the rows inside the draw call - so a file read and a JSON parse per title,
per repaint, and immediate mode repaints whenever the pointer moves (D164).

The icon cache in the same crate opens with a comment warning about precisely this. Having
written the rule down an hour earlier did not stop me breaking it; it only made it quick to
recognise on a re-read.

Rows are state now, rebuilt when the library is rescanned and when a run finishes - the
only two events that can change what one says. The second half matters as much as the
first: a cache that never refreshes is the same bug wearing the opposite mask.

Still open: an instance exited by itself with status 1, no output, while idle and untouched.
It has not reproduced across two attempts since. Not attributed to this fix - there is no
evidence they are related, and calling it closed would hide it.

