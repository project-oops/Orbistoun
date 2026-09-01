# The streamed report needed no change, and now there is a test saying so


Two behavioural changes upstream: `report` streams its full record set over the socket
rather than a summary, and a serving build is interactive-first - it listens immediately and
runs the suite only when asked.

Neither needed a code change. Records between `ack` and `done` were always collected, and
`orbistoun session` asks for the report rather than expecting one on connect. But "no change
needed" is a claim, and an untested claim about a path that has just become the *primary*
one is worth converting into a test.

`a_streamed_report_arrives_between_the_acknowledgement_and_the_answer` drives a full stream
- section, try, res, sym, sectiontally, tally - through the client, replays the wire into
the same reader a committed corpus goes through, and asserts sections, findings and symbols
all come out. It also asserts that `tally`, a kind this version does not model, survives
verbatim; that is the property letting obSCEne add records without breaking this.

`nothing_is_expected_to_arrive_before_a_command_asks_for_it` pins the other half: a client
assuming a report on connect would block forever against a probe behaving correctly.

### Surprises

**The valuable part of "no change needed" was writing the test anyway.** Both changes landed
on paths that already worked, so the honest options were to say so and move on, or to make
the claim checkable. The second cost fifteen minutes and turned an assumption about the
newly-primary path into evidence - and this session has spent most of its time discovering
that untested claims were wrong.

