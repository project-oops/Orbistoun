# D326 - The window owns host input

**Status:** decided
**Date:** 2026-08-27

Host controllers and the keyboard are read by the window, on up to four configurable ports.
Buttons are named by position, key names are text, and the shell button is taken by the window
before anything reaches a title. Tap and hold are decided by a pure type over elapsed time; a
hold fires once, while still held.

**Why:** the shell button must be seen by something other than the title, and a worker reading
pads directly could not hold one button back. A keyboard has no glyphs and a host pad reports
positions, so naming by symbol would translate twice and bring vendor marks into the tree.
Text key names keep the window toolkit out of the input contract.

**Rejected:**
- Reading input in the worker: cannot withhold the shell button.
- Buttons named by glyph: a double translation and vendor marks.
