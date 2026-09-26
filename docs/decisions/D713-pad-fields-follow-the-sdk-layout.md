# D713 - Pad fields follow the SDK layout

**Status:** assumed
**Date:** 2026-09-24

The pad-read calls write the window's pad into the fields the collection's SDK places, over the
measured 120-byte at-rest image: the button word at 0, sticks at 4 and 6 with `0x80` centre,
analogue triggers at 8, and the connected flag at 76. Bytes the SDK does not place keep their
measured values, and the placement is recorded as guest-observed.

**Why:** titles built on the SDK read their buttons through those offsets and navigate with a
pad on the hardware, which is evidence about the layout rather than an inference from one
at-rest image. A byte-level probe with a button held stays open to confirm or replace it, and
the placement function is the one place that would change. Bits the SDK never exercises, such as
the touchpad pair, may still be wrong.

**Rejected:**
- Keeping live input out until the probe lands: no title can be driven.
- Laying out fields from the at-rest image alone: publishes an inference as a measurement.
