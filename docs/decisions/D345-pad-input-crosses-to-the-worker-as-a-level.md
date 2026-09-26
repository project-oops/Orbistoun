# D345 - Pad input crosses to the worker as a level

**Status:** decided
**Date:** 2026-08-27

`Request::Input` carries pad state to the worker, where `orbistoun_input::latest` holds the most
recent state per port; an unchanged pad sends nothing and an absent port reads as neutral. The
window decides what a title may see: the shell button is always stripped and an unfocused title
sees an idle pad. A pad update that reaches no guest is counted.

**Why:** the transport is ours and can be built and asserted before the guest structure is
measured. A queue would replay presses that finished seconds ago. A title enumerating four pads
with two configured should find two idle ones, as on real hardware. The latest-state rule and
focus make `Focus` observable, and a count shows a transport waiting rather than broken.

**Rejected:**
- A queue of events: replays stale input.
- An error for an unconfigured port: unlike any real machine.
- Deferring the transport until the layout is measured: the same mechanism-versus-encoding split the event queue already makes.
