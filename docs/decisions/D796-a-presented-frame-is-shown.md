# D796 - A presented frame is shown opaque

**Status:** decided
**Date:** 2026-10-11

The GUI shows a frame the guest presented as opaque: each pixel's colour as the buffer holds it,
its alpha ignored. A console scans its display buffer out to the screen and blends it with
nothing, so the alpha a title leaves there never reaches the picture.

**Why:** PPSA28061 draws its game through a CRT filter that leaves alpha zero across the whole
screen, its logo included. Shown with its alpha, the GUI drew the logo and the black around it as
transparent - the panel's own background where the console shows a black screen and a logo.

**Rejected:**
- Keeping the alpha for titles that write one: no title's alpha is ever composited on the
  hardware, so there is no title for which it would be right.
