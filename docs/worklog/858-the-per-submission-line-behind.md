# 858. The per-submission line behind ORBISTOUN_TRACE_SUBMITS

**2026-09-25**. Neverball runs at 15-16 flips a second.

The executor wrote "a submission's draws ran at submit: N command(s)..." to stderr for every
submission: ~50 a frame, ~800 a second. Nothing parses it. Its span measured ~21 ms/s, but the
device thread's time went from ~210 to ~180 ms/s without it: stderr writes cost more than the span
around the format showed.

It now prints only under `ORBISTOUN_TRACE_SUBMITS=1` (declared in `orbistoun-env`), which gives
5,776 lines in an 8-second run. A refusal is printed whether or not it is set, because a draw that
did not run is never quiet. This fits the logging service's levels once there is one.
