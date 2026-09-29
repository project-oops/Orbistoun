# D737 - Register state carries across submissions on a queue

**Status:** decided
**Date:** 2026-09-29

A live guest's submissions are prepared on top of the register state the ones before them left.
The pipeline keeps the last value of every register the queue's submissions wrote and places
those writes ahead of each new stream's own, so a draw that relies on a register an earlier
submission set finds it, and a stream's own writes still win. `CLEAR_STATE` resets register state
to its defaults, so a stream that clears starts from its own writes after the clear, and the state
carried on from it is only those.

State advances once per submission, after the command processor has carried it out: every
preparation of one submission, its draw segments included (D729), sees the state from before it.
It is opt-in, for the live pipeline; a pipeline preparing streams by hand still prepares each on
its own.

**Why:** register state lives in the GPU, not in a command buffer. A submission leaves every
register it wrote as it wrote it for the next, which radeonsi relies on: it chains a command
buffer that outgrows its space to the next with an `INDIRECT_BUFFER`, and oops-mesa's winsys
submits each link on its own because the command processor fetches nothing outside the submitted
range (obSCEne `REQ-20260922T2230Z-6e81`). A later link's draws then run the shaders an earlier
link bound. SuperTuxKart does exactly that and plays on hardware; here those draws had no
primitive shader, so nothing after them retired.

**Rejected:**
- Asking oops-mesa to repeat the state at every link: it would hide a hardware behaviour other
  titles rely on without saying so.
- Carrying state into every pipeline: streams prepared by hand, as the tests and captures are, are
  each whole, and would change meaning with the order they were prepared in.
- Keeping state from `CLEAR_STATE`'s defaults as values: the defaults are not modelled, and a
  register left unwritten already reads as not set.
