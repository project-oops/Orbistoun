# D728 - A command-buffer flip is carried out where the stream releases its label

**Status:** assumed
**Date:** 2026-09-27

`sceAgcDcbSetFlip` writes the packet hardware writes and queues the flip with the display
under the interrupt context id that packet carries. When the command processor reaches the
packet's `RELEASE_MEM` to the buffer's flip label, it reports the label and context id, and the
display performs the flip through the path `sceVideoOutSubmitFlip` uses, then clears the label
of the buffer that left the screen. The labels live at the console's own address,
`0xC_8000_40A0` plus eight bytes per buffer, mapped on the first queued flip.

**Why:** obSCEne `-1d54` measured the packet and `-354a` the wait that pairs with it: a flip is
an end-of-pipe release of `1` into the buffer's label, and rendering into a buffer waits for its
label to read `0`. Carrying the flip out at the release puts it where the hardware does - after
the work before it in the stream, and only if the stream got that far (D705).

**Rejected:**
- Flipping when the builder is called: presents a frame whose draws have not run, and flips for
  a command buffer that is never submitted.
- Decoding the flip from the register writes: the register word's layout is one observation of
  one port, while the context id is the packet's own name for the flip.
- A no-op builder, as before: sweep 20260917-124503 measured a writer with no port open, which
  writes nothing on hardware too; with a port open the packet is real.
