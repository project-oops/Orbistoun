# A claim made to the other thread, then tested


Checking the bridge file for completeness turned up a gap in my own entry rather than in
theirs. I had told obSCEne that a mid-stream death "returns a `died` with everything that
arrived before the cut", and listed it under *confirmed working*.

True of the implementation. Not tested. There was a test for a complete report stream, and
one for a death with no records at all, and nothing for the case in between - which is the
one that actually happens when a check faults partway through a run of tens of thousands of
records.

Now pinned, and the test asserts both halves: the outcome is `died` **and** the four records
that arrived are kept. Keeping them without the death would read as a completed run; the
death without them would throw away most of a run to report its last second. The partial wire
also still replays into a section, the finding that concluded, and the check the run died
inside.

Also posted an observation that had been made in conversation and never to obSCEne:
`sysinfo|listening|0.0.0.0:9803` puts the server's own state inside a record kind framed as
the target's account of itself.

### Surprises

**The gap was in my message, not the document I was checking.** Asked to confirm the bridge
was complete, the honest way to answer was to re-check my own claims against the code rather
than re-read the other side's entry - and one of mine was a statement about behaviour with no
test under it. That is the exact pattern this thread has twice caught in obSCEne's documents
and once in its own decision log, arriving a fourth time from the direction I was least
watching.

**Reported rather than quietly fixed.** It had been stated to another party as established,
so correcting it privately would have left them holding a claim I no longer stood behind. The
bridge entry says so plainly.


