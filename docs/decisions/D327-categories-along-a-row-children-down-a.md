# D327 - Categories along a row, children down a column


**decided** · 2026-08-27 · asked for by name, built as a shape

A row of headings with the selected one's children underneath is how console shells have
presented a library for twenty years. It is a **shape** rather than a design - it falls out
of having a directional pad and more things than fit on a screen - and it earns its place
here for a reason beyond familiarity: the whole shell becomes reachable with four directions
and one button, which is what makes a controller a way to *use* this rather than something
the emulator merely reads.

What is somebody's design is the artwork, the motion, the sounds and the proportions, and
none of that is copied. The name is not used either: D313 settled that this tree says
"shell", and adopting a vendor's name for its own presentation would give back the position
the clean-room work exists to hold.

The navigation is a pure type over a *shape* - one item count per category - because all four
rules are edges: the ends of the row, a column shorter than the one beside it, a category
holding nothing, and a shape that shrinks underneath the highlight when a rescan finds fewer
titles. Nothing wraps, deliberately: somebody navigating by feel counts presses, and one
press too many should rest against the end rather than start a journey back around.

**One highlight, moved by both.** A pointer and a pad move the same value, and confirming
runs the same match a click runs - so neither can reach something the other cannot.

Verified by driving it: right moved to `settings` and drew its three children; left and three
downs landed exactly on the fourth title.


