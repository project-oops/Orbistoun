# D891 - An asynchronous compute stream waiting on a fence steps aside

**Status:** decided
**Date:** 2026-10-18

A stream handed over with `sceAgcDriverSubmitAcb` runs on an asynchronous compute ring of its own.
orbistoun carries it out on the one command processor, in order with the graphics streams, until
it reaches a wait not yet satisfied. There it steps aside, kept with the byte it stopped at, and
the graphics ring goes on. After every graphics submission the streams stepped aside are carried
on from where they stopped, oldest first, each until it waits again; a compute stream submitted
while one waits goes in behind it, as its ring would keep it. A graphics stream that stops at a
wait is carried on again whenever a compute stream stepped aside goes further. A compute stream
that stops for any other reason stops the queue, as a graphics one does (D710).

**Why:** PPSA04263 submits compute work that waits on a fence the graphics ring writes later: its
first compute stream waits for the frame counter to reach the value a graphics submission queued
after it releases. On the console the two rings run side by side and the compute ring simply
waits. Carried out in submission order on one queue, the compute stream's wait was never
satisfied and every graphics submission behind it waited forever.

**Rejected:**
- A command processor thread per ring. Closest to the console, but every preparation and
  write-back is ordered against one queue (D821, D876), and a second thread would need all of that
  done twice.
- Carrying a compute stream out ahead of the graphics ring, or after it at the end of each frame.
  Either order puts a fence write on the wrong side of the wait that reads it in some title.
