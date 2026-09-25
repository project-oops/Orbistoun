# D713 - pad fields are placed by the SDK layout titles navigate with on hardware

**Status:** assumed
**Date:** 2026-09-24

## The question

D345, D671 and D704 kept live pad state out of `scePadReadState`. The 120-byte extent and the
at-rest image are measured, but which offset carries which field is not, and orbistoun would not
publish an inference as though it were a measurement. REQ-20260910T0650Z-d1c4 asks obSCEne for a
run with a button held to settle it. That request is still open, and it needs a person at a console
with a controller. Meanwhile a launcher (SCSH00001) runs in the window and cannot be driven. Is
there evidence for the field positions other than that probe?

## The choice

**Yes: the collection's own SDK.** `oops-sdk/src/input/pad_layout.h` places:

- the button word at 0;
- the sticks at 4 and 6 (one byte per axis, 0x80 centre);
- the analogue triggers at 8;
- the connected flag at 76.

`include/oops/input.h` gives the button bits. Titles built on it, SeaShell and Neverball among them,
read their buttons through exactly those positions and **navigate on a console with a pad**. That is
attested by the person who runs them there. A title working on hardware through a layout is
evidence about the layout. It is not an inference from one at-rest image. So `scePadReadState` and
`scePadRead` now write the window's pad into those fields, over the measured at-rest bytes. Every
byte the SDK does not place keeps its measured value.

`known_by` for the placement is **guest-observed**: established by what a guest does when given
it, on hardware, not by a byte-level measurement. REQ-d1c4 stays open. When its answer lands, it
either confirms these positions or replaces them, and the placement function is the one place that
changes.

## What it costs

- The button bits are the SDK's. A retail title that reads a bit the SDK never exercised, for
  example the create/touchpad pair the SDK itself notes as ambiguous, may still see the wrong one.
- `Select` is written as the touchpad bit, and that is the weakest part. No title here has
  exercised it.
- The connected flag is set only when the window has actually sent a pad. With no pad, the guest
  still gets the measured at-rest image, unchanged.
