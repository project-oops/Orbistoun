# D553 - An export writes a fragment output variable

**Status:** decided
**Date:** 2026-09-04

A translated `exp` becomes a store of four bitcast lane-zero components to a `Location 0` output
of a `Fragment` module, whose epilogue skips the observation window. A module with no colour
output, any target other than `mrt0`, and the compressed, done and write-mask fields are refused.

**Why:** a fragment module's oracle is the attachment, so the observation window is redundant
there. A vector register holds the colour's bit pattern, so an arithmetic conversion would scale
every channel. Which attachment another target selects is guest register state that needs a
capture, and fields the decoder has not solved cannot be claimed as honoured.

**Rejected:**
- Mapping the export onto the storage buffer: invents a destination, and a plausible frame hides it.
- Translating every target onto attachment zero: appears to work while drawing to the wrong place.
- Converting components numerically: scales every channel.
